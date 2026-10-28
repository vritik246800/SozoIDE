use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Context, Result, anyhow, bail};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use sqlx::{
    AnyPool, Column, Executor, Row, ValueRef,
    any::{AnyPoolOptions, AnyRow, AnyTypeInfoKind},
};

use super::model::{ConnectionConfig, ConnectionForm, DatabaseValue, QueryResult};

const KEYRING_SERVICE: &str = "SozoIDE";
static NEXT_CONNECTION_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Default, Serialize, Deserialize)]
struct ConnectionsFile {
    #[serde(default)]
    connections: Vec<ConnectionConfig>,
}

#[derive(Clone, Debug)]
pub(crate) struct ConnectionStore {
    pub(crate) path: PathBuf,
}

impl ConnectionStore {
    pub(crate) fn discover() -> Result<Self> {
        let directories = ProjectDirs::from("", "", KEYRING_SERVICE)
            .context("Could not determine the user configuration directory")?;
        Ok(Self {
            path: directories.config_dir().join("connections.toml"),
        })
    }

    pub(crate) fn load(&self) -> Result<Vec<ConnectionConfig>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let contents = fs::read_to_string(&self.path)
            .with_context(|| format!("Could not read {}", self.path.display()))?;
        toml::from_str::<ConnectionsFile>(&contents)
            .map(|file| file.connections)
            .with_context(|| format!("Could not parse {}", self.path.display()))
    }

    fn save(&self, connections: &[ConnectionConfig]) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "Could not create configuration directory {}",
                    parent.display()
                )
            })?;
        }
        let contents = toml::to_string_pretty(&ConnectionsFile {
            connections: connections.to_vec(),
        })
        .context("Could not serialize connection metadata")?;
        fs::write(&self.path, contents)
            .with_context(|| format!("Could not write {}", self.path.display()))
    }
}

pub(crate) async fn open_pool(form: &ConnectionForm) -> Result<AnyPool> {
    sqlx::any::install_default_drivers();
    let url = form.connection_url()?;
    let options = AnyPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(10));
    options.connect(&url).await.map_err(|error| {
        let mut message = error.to_string();
        if !url.is_empty() {
            message = message.replace(&url, "<connection URL>");
        }
        if !form.password.is_empty() {
            message = message.replace(&form.password, "<redacted>");
        }
        anyhow!("{} connection failed: {message}", form.driver)
    })
}

pub(crate) async fn run_query(pool: &AnyPool, query: &str) -> Result<QueryResult> {
    let started = Instant::now();
    if query_returns_rows(query) {
        let rows = sqlx::query(query)
            .fetch_all(pool)
            .await
            .map_err(|error| anyhow!("Query failed: {error}"))?;
        // The names come with the rows; only an empty result needs `describe`, which the
        // SQLite driver cannot do for table-valued functions such as `pragma_table_info` (E26).
        let columns = match rows.first() {
            Some(row) => row
                .columns()
                .iter()
                .map(|column| column.name().to_owned())
                .collect(),
            None => pool
                .describe(query)
                .await
                .map(|description| {
                    description
                        .columns()
                        .iter()
                        .map(|column| column.name().to_owned())
                        .collect()
                })
                .unwrap_or_default(),
        };
        let rows = rows.iter().map(decode_row).collect::<Result<Vec<_>>>()?;
        Ok(QueryResult {
            columns,
            rows,
            affected_rows: 0,
            duration: started.elapsed(),
        })
    } else {
        let result = sqlx::query(query)
            .execute(pool)
            .await
            .map_err(|error| anyhow!("Query failed: {error}"))?;
        Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            affected_rows: result.rows_affected(),
            duration: started.elapsed(),
        })
    }
}

fn query_returns_rows(query: &str) -> bool {
    let normalized = query.trim_start().to_ascii_lowercase();
    let first_word = normalized
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .next()
        .unwrap_or_default();
    matches!(
        first_word,
        "select" | "with" | "pragma" | "explain" | "show" | "describe" | "desc" | "values"
    ) || normalized.contains(" returning ")
}

fn decode_row(row: &AnyRow) -> Result<Vec<DatabaseValue>> {
    (0..row.columns().len())
        .map(|index| {
            let value = row.try_get_raw(index)?;
            if value.is_null() || value.type_info().kind() == AnyTypeInfoKind::Null {
                return Ok(DatabaseValue::Null);
            }
            let name = row.column(index).name();
            let error = || anyhow!("Could not decode column `{name}`");
            match value.type_info().kind() {
                AnyTypeInfoKind::Null => Ok(DatabaseValue::Null),
                AnyTypeInfoKind::Bool => row
                    .try_get::<bool, _>(index)
                    .map(DatabaseValue::Bool)
                    .map_err(|_| error()),
                AnyTypeInfoKind::SmallInt | AnyTypeInfoKind::Integer | AnyTypeInfoKind::BigInt => {
                    row.try_get::<i64, _>(index)
                        .map(DatabaseValue::Integer)
                        .map_err(|_| error())
                }
                AnyTypeInfoKind::Real | AnyTypeInfoKind::Double => row
                    .try_get::<f64, _>(index)
                    .map(DatabaseValue::Float)
                    .map_err(|_| error()),
                AnyTypeInfoKind::Text => row
                    .try_get::<String, _>(index)
                    .map(DatabaseValue::Text)
                    .map_err(|_| error()),
                AnyTypeInfoKind::Blob => row
                    .try_get::<Vec<u8>, _>(index)
                    .map(DatabaseValue::Bytes)
                    .map_err(|_| error()),
            }
        })
        .collect()
}

pub(crate) async fn persist_connections(
    store: ConnectionStore,
    connections: Vec<ConnectionConfig>,
) -> Result<()> {
    tokio::task::spawn_blocking(move || store.save(&connections))
        .await
        .map_err(|_| anyhow!("Connection metadata save task failed"))?
}

pub(crate) async fn load_password(id: String) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        let entry = keyring::Entry::new(KEYRING_SERVICE, &id)
            .map_err(|_| anyhow!("Could not access the system keyring"))?;
        match entry.get_password() {
            Ok(password) => Ok(password),
            Err(keyring::Error::NoEntry) => Ok(String::new()),
            Err(_) => Err(anyhow!(
                "Could not read the password from the system keyring"
            )),
        }
    })
    .await
    .map_err(|_| anyhow!("Keyring read task failed"))?
}

pub(crate) async fn save_password(id: String, password: String) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let entry = keyring::Entry::new(KEYRING_SERVICE, &id)
            .map_err(|_| anyhow!("Could not access the system keyring"))?;
        if password.is_empty() {
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(_) => Err(anyhow!("Could not remove the old keyring password")),
            }
        } else {
            entry
                .set_password(&password)
                .map_err(|_| anyhow!("Could not save the password in the system keyring"))
        }
    })
    .await
    .map_err(|_| anyhow!("Keyring write task failed"))?
}

pub(crate) async fn delete_password(id: String) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let entry = keyring::Entry::new(KEYRING_SERVICE, &id)
            .map_err(|_| anyhow!("Could not access the system keyring"))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(anyhow!(
                "Could not delete the password from the system keyring"
            )),
        }
    })
    .await
    .map_err(|_| anyhow!("Keyring delete task failed"))?
}

pub(crate) fn new_connection_id() -> String {
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = NEXT_CONNECTION_ID.fetch_add(1, Ordering::Relaxed);
    format!("connection-{timestamp:x}-{sequence:x}")
}

pub(crate) fn nonempty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.into())
}

pub(crate) fn reject_url_password(url: &str) -> Result<()> {
    if let Some(scheme_end) = url.find("://") {
        let authority = url[scheme_end + 3..]
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default();
        if let Some((userinfo, _)) = authority.rsplit_once('@')
            && userinfo.contains(':')
        {
            bail!("Connection URLs must not contain passwords; use the password field");
        }
    }
    Ok(())
}

pub(crate) fn add_url_password(
    url: &str,
    username: Option<&str>,
    password: &str,
) -> Result<String> {
    let scheme_end = url
        .find("://")
        .context("Connection URL must include a scheme and authority")?;
    let authority_start = scheme_end + 3;
    let authority_end = url[authority_start..]
        .find(['/', '?', '#'])
        .map_or(url.len(), |offset| authority_start + offset);
    let authority = &url[authority_start..authority_end];
    let (userinfo, host) = authority
        .rsplit_once('@')
        .map_or((username.unwrap_or_default(), authority), |(user, host)| {
            (user, host)
        });
    if userinfo.is_empty() {
        bail!("Username is required when a password is supplied");
    }
    Ok(format!(
        "{}{}:{}@{}{}",
        &url[..authority_start],
        userinfo,
        percent_encode(password),
        host,
        &url[authority_end..]
    ))
}

pub(crate) fn percent_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{DatabasePanel, Driver};

    #[test]
    fn persisted_metadata_can_be_loaded_without_a_password() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("connections.toml");
        let config = ConnectionConfig {
            id: "local-sqlite".into(),
            name: "Local SQLite".into(),
            driver: Driver::SQLite,
            url: Some("sqlite::memory:".into()),
            database: None,
            host: None,
            port: None,
            username: None,
        };
        let store = ConnectionStore { path: path.clone() };
        store.save(std::slice::from_ref(&config)).unwrap();
        let loaded = DatabasePanel::with_config_path(path).unwrap();
        assert_eq!(loaded.connections, vec![config]);
    }
}
