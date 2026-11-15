use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Context, Result, anyhow, bail};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use super::model::{ConnectionConfig, ConnectionForm, DatabaseValue, Driver, QueryResult};

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

    /// The saved connections; a file that cannot be read is kept aside and SozoIDE opens
    /// with none, instead of not opening at all.
    pub(crate) fn load(&self) -> Vec<ConnectionConfig> {
        crate::storage::read::<ConnectionsFile>(&self.path).connections
    }

    fn save(&self, connections: &[ConnectionConfig]) -> Result<()> {
        let contents = toml::to_string_pretty(&ConnectionsFile {
            connections: connections.to_vec(),
        })
        .context("Could not serialize connection metadata")?;
        crate::storage::write(&self.path, &contents)
            .with_context(|| format!("Could not write {}", self.path.display()))
    }
}

/// An open database, each through its own driver (Q98): `rusqlite`, `tokio-postgres` and
/// `mysql_async`. One connection each, as before.
#[derive(Clone)]
pub(crate) enum Pool {
    Sqlite(Arc<Sqlite>),
    Postgres(Arc<tokio_postgres::Client>),
    Mysql(Arc<tokio::sync::Mutex<Option<mysql_async::Conn>>>),
}

pub(crate) struct Sqlite {
    pub(super) connection: Mutex<rusqlite::Connection>,
    interrupt: rusqlite::InterruptHandle,
}

impl Pool {
    pub(crate) async fn close(&self) {
        if let Self::Mysql(connection) = self
            && let Some(connection) = connection.lock().await.take()
        {
            let _ = connection.disconnect().await;
        }
    }
}

pub(crate) async fn open_pool(form: &ConnectionForm) -> Result<Pool> {
    let url = form.connection_url()?;
    if form.driver == Driver::SQLite {
        return tokio::task::spawn_blocking(move || open_sqlite(&url))
            .await
            .map_err(|_| anyhow!("SQLite open task failed"))?;
    }
    let opened = tokio::time::timeout(CONNECT_TIMEOUT, async {
        match form.driver {
            Driver::PostgreSQL => open_postgres(&url).await,
            _ => open_mysql(&url).await,
        }
    })
    .await
    .unwrap_or_else(|_| Err(anyhow!("no answer in {} s", CONNECT_TIMEOUT.as_secs())));
    opened.map_err(|error| {
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

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// The URL without its TLS parameter (`sslmode` or `ssl-mode`, any case) and that value in
/// lower case: each driver reads TLS from SozoIDE, not from its own URL parser.
fn take_tls_mode(url: &str) -> (String, Option<String>) {
    let Some((base, query)) = url.split_once('?') else {
        return (url.to_owned(), None);
    };
    let mut mode = None;
    let rest: Vec<&str> = query
        .split('&')
        .filter(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let tls = matches!(key.to_ascii_lowercase().as_str(), "sslmode" | "ssl-mode");
            if tls {
                mode = Some(value.to_ascii_lowercase().replace('_', "-"));
            }
            !tls && !pair.is_empty()
        })
        .collect();
    let url = if rest.is_empty() {
        base.to_owned()
    } else {
        format!("{base}?{}", rest.join("&"))
    };
    (url, mode)
}

/// rustls with ring: certificates checked against the web roots when `verify`; otherwise
/// encrypted without checking who answers, as libpq's `prefer` and `require` do.
fn tls_config(verify: bool) -> Result<rustls::ClientConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|error| anyhow!("TLS: {error}"))?;
    Ok(if verify {
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        builder.with_root_certificates(roots).with_no_client_auth()
    } else {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(EncryptOnly(provider)))
            .with_no_client_auth()
    })
}

/// Checks the signatures of the handshake but not the certificate (`sslmode=require`).
#[derive(Debug)]
struct EncryptOnly(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for EncryptOnly {
    fn verify_server_cert(
        &self,
        _: &rustls::pki_types::CertificateDer<'_>,
        _: &[rustls::pki_types::CertificateDer<'_>],
        _: &rustls::pki_types::ServerName<'_>,
        _: &[u8],
        _: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &rustls::pki_types::CertificateDer<'_>,
        signature: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            certificate,
            signature,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &rustls::pki_types::CertificateDer<'_>,
        signature: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            certificate,
            signature,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

async fn open_postgres(url: &str) -> Result<Pool> {
    use tokio_postgres::config::SslMode;
    let (url, mode) = take_tls_mode(url);
    let mut config: tokio_postgres::Config = url.parse().map_err(|error| anyhow!("{error}"))?;
    let (ssl, verify) = match mode.as_deref().unwrap_or("prefer") {
        "disable" => (SslMode::Disable, false),
        "allow" | "prefer" => (SslMode::Prefer, false),
        "require" => (SslMode::Require, false),
        "verify-ca" | "verify-full" => (SslMode::Require, true),
        other => bail!("unknown sslmode `{other}`"),
    };
    config.ssl_mode(ssl).connect_timeout(CONNECT_TIMEOUT);
    let tls = tokio_postgres_rustls::MakeRustlsConnect::new(tls_config(verify)?);
    let (client, connection) = config
        .connect(tls)
        .await
        .map_err(|error| anyhow!("{error}"))?;
    tokio::spawn(connection);
    Ok(Pool::Postgres(Arc::new(client)))
}

async fn open_mysql(url: &str) -> Result<Pool> {
    use mysql_async::{Conn, Opts, OptsBuilder, SslOpts};
    let (url, mode) = take_tls_mode(url);
    let opts = Opts::from_url(&url).map_err(|error| anyhow!("{error}"))?;
    let connect =
        |ssl: Option<SslOpts>| Conn::new(OptsBuilder::from_opts(opts.clone()).ssl_opts(ssl));
    let unchecked = SslOpts::default().with_danger_accept_invalid_certs(true);
    let connection = match mode.as_deref().unwrap_or("preferred") {
        "disabled" => connect(None).await,
        "required" => connect(Some(unchecked)).await,
        "verify-ca" => {
            connect(Some(
                SslOpts::default().with_danger_skip_domain_validation(true),
            ))
            .await
        }
        "verify-identity" => connect(Some(SslOpts::default())).await,
        // TLS when the server has it, as the MySQL client does.
        "preferred" => match connect(Some(unchecked)).await {
            Err(mysql_async::Error::Driver(
                mysql_async::DriverError::NoClientSslFlagFromServer,
            )) => connect(None).await,
            other => other,
        },
        other => bail!("unknown ssl-mode `{other}`"),
    }
    .map_err(|error| anyhow!("{error}"))?;
    Ok(Pool::Mysql(Arc::new(tokio::sync::Mutex::new(Some(
        connection,
    )))))
}

/// Where a `sqlite:` URL points and how it opens, read as `sqlx` read it: `sqlite::memory:`,
/// `sqlite://<path>` or `sqlite:<path>`, with `?mode=ro|rw|rwc|memory` (`rw` when absent).
fn sqlite_target(url: &str) -> Result<(Option<String>, rusqlite::OpenFlags)> {
    use rusqlite::OpenFlags;
    let rest = url
        .strip_prefix("sqlite:")
        .context("SQLite URLs start with sqlite:")?;
    let rest = rest.strip_prefix("//").unwrap_or(rest);
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let mode = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("mode="))
        .unwrap_or("rw");
    let flags = OpenFlags::SQLITE_OPEN_NO_MUTEX
        | match mode {
            "ro" => OpenFlags::SQLITE_OPEN_READ_ONLY,
            "rw" => OpenFlags::SQLITE_OPEN_READ_WRITE,
            "rwc" | "memory" => OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
            other => bail!("Unknown SQLite mode `{other}`: use ro, rw, rwc or memory"),
        };
    if mode == "memory" || path.is_empty() || path == ":memory:" {
        return Ok((None, flags));
    }
    // `sqlite:///C:/x` on Windows: the slash before the drive is not part of the path.
    let path = match path.as_bytes() {
        [b'/', _, b':', ..] => &path[1..],
        _ => path,
    };
    Ok((Some(path.to_owned()), flags))
}

fn open_sqlite(url: &str) -> Result<Pool> {
    let (path, flags) = sqlite_target(url)?;
    let connection = match &path {
        Some(path) => rusqlite::Connection::open_with_flags(path, flags),
        None => rusqlite::Connection::open_in_memory_with_flags(flags),
    }
    .map_err(|error| anyhow!("SQLite connection failed: {error}"))?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(|error| anyhow!("SQLite connection failed: {error}"))?;
    let interrupt = connection.get_interrupt_handle();
    Ok(Pool::Sqlite(Arc::new(Sqlite {
        connection: Mutex::new(connection),
        interrupt,
    })))
}

/// Whether `error` came from a connection to a server that dropped: it opens again once.
/// ponytail: MySQL by the words of its errors; a typed check if a driver changes them.
pub(crate) fn lost(pool: &Pool, error: &anyhow::Error) -> bool {
    match pool {
        Pool::Sqlite(_) => false,
        Pool::Postgres(client) => client.is_closed(),
        Pool::Mysql(_) => {
            let text = error.to_string().to_lowercase();
            [
                "broken pipe",
                "connection reset",
                "connection closed",
                "gone away",
                "lost connection",
                "unexpected eof",
            ]
            .iter()
            .any(|words| text.contains(words))
        }
    }
}

pub(crate) async fn run_query(pool: &Pool, query: &str) -> Result<QueryResult> {
    run_page(pool, query, usize::MAX).await
}

fn page(mut result: QueryResult, limit: usize) -> QueryResult {
    if result.rows.len() > limit {
        result.rows.truncate(limit);
        result.more = true;
    }
    result
}

/// `query` with at most `limit` rows; `more` says there were others. SQLite stops reading
/// there; PostgreSQL and MySQL send everything and the rest is left out (ponytail: a server-side
/// cursor if tables of millions of rows over the network matter).
pub(crate) async fn run_page(pool: &Pool, query: &str, limit: usize) -> Result<QueryResult> {
    match pool {
        Pool::Postgres(client) => postgres_query(client, query)
            .await
            .map(|result| page(result, limit)),
        Pool::Mysql(connection) => {
            let mut connection = connection.lock().await;
            let connection = connection
                .as_mut()
                .context("The connection is closed; connect again")?;
            mysql_query(connection, query)
                .await
                .map(|result| page(result, limit))
        }
        Pool::Sqlite(database) => {
            /// A query dropped half way (cancelled) stops in SQLite too.
            struct StopOnDrop(Option<Arc<Sqlite>>);
            impl Drop for StopOnDrop {
                fn drop(&mut self) {
                    if let Some(database) = self.0.take() {
                        database.interrupt.interrupt();
                    }
                }
            }
            let mut stop = StopOnDrop(Some(database.clone()));
            let (database, query) = (database.clone(), query.to_owned());
            let result = tokio::task::spawn_blocking(move || {
                let connection = database
                    .connection
                    .lock()
                    .map_err(|_| anyhow!("The SQLite connection stopped; connect again"))?;
                sqlite_query(&connection, &query, limit)
            })
            .await
            .map_err(|_| anyhow!("SQLite query task failed"));
            stop.0 = None;
            result?
        }
    }
}

/// Every statement of `query` in turn; the last one that returns columns is the result.
fn sqlite_query(
    connection: &rusqlite::Connection,
    query: &str,
    limit: usize,
) -> Result<QueryResult> {
    use rusqlite::{fallible_iterator::FallibleIterator, types::ValueRef};
    let started = Instant::now();
    let failed = |error: rusqlite::Error| anyhow!("Query failed: {error}");
    let mut result = QueryResult {
        columns: Vec::new(),
        rows: Vec::new(),
        affected_rows: 0,
        duration: Duration::ZERO,
        more: false,
    };
    let mut batch = rusqlite::Batch::new(connection, query);
    while let Some(mut statement) = batch.next().map_err(failed)? {
        let columns: Vec<String> = statement.column_names().map(str::to_owned).collect();
        if columns.is_empty() {
            result.affected_rows += statement.raw_execute().map_err(failed)? as u64;
            continue;
        }
        let booleans: Vec<bool> = statement
            .columns()
            .map(|column| {
                column
                    .decl_type()
                    .is_some_and(|kind| kind.to_ascii_uppercase().contains("BOOL"))
            })
            .collect();
        let mut rows = Vec::new();
        let mut cursor = statement.raw_query();
        while let Some(row) = cursor.next().map_err(failed)? {
            // A page of rows: the rest are not read (TODO 4.1.2).
            if rows.len() == limit {
                result.more = true;
                break;
            }
            let values = (0..columns.len())
                .map(|index| {
                    Ok(match row.get_ref(index).map_err(failed)? {
                        ValueRef::Null => DatabaseValue::Null,
                        ValueRef::Integer(value) if booleans[index] => {
                            DatabaseValue::Bool(value != 0)
                        }
                        ValueRef::Integer(value) => DatabaseValue::Integer(value),
                        ValueRef::Real(value) => DatabaseValue::Float(value),
                        ValueRef::Text(text) => {
                            DatabaseValue::Text(String::from_utf8_lossy(text).into_owned())
                        }
                        ValueRef::Blob(bytes) => DatabaseValue::Bytes(bytes.to_vec()),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            rows.push(values);
        }
        result.columns = columns;
        result.rows = rows;
    }
    result.duration = started.elapsed();
    Ok(result)
}

/// PostgreSQL by the simple protocol, which runs several statements and sends every value as
/// text; the types come from preparing a single statement, so numbers and booleans stay typed.
async fn postgres_query(client: &tokio_postgres::Client, query: &str) -> Result<QueryResult> {
    use tokio_postgres::{SimpleQueryMessage, types::Type};
    let started = Instant::now();
    let types: Vec<Type> = match client.prepare(query).await {
        Ok(statement) => statement
            .columns()
            .iter()
            .map(|column| column.type_().clone())
            .collect(),
        Err(_) => Vec::new(),
    };
    let messages = client
        .simple_query(query)
        .await
        .map_err(|error| anyhow!("Query failed: {}", postgres_error(&error)))?;
    let mut result = QueryResult {
        columns: Vec::new(),
        rows: Vec::new(),
        affected_rows: 0,
        duration: Duration::ZERO,
        more: false,
    };
    for message in messages {
        match message {
            SimpleQueryMessage::RowDescription(columns) => {
                result.columns = columns
                    .iter()
                    .map(|column| column.name().to_owned())
                    .collect();
                result.rows.clear();
            }
            SimpleQueryMessage::Row(row) => result.rows.push(
                (0..row.len())
                    .map(|index| match row.get(index) {
                        None => DatabaseValue::Null,
                        Some(text) => postgres_value(types.get(index), text),
                    })
                    .collect(),
            ),
            SimpleQueryMessage::CommandComplete(count) if result.columns.is_empty() => {
                result.affected_rows = count;
            }
            _ => {}
        }
    }
    result.duration = started.elapsed();
    Ok(result)
}

/// The server's own message and detail, not the driver's "db error".
fn postgres_error(error: &tokio_postgres::Error) -> String {
    match error.as_db_error() {
        Some(db) => match db.detail() {
            Some(detail) => format!("{}: {detail}", db.message()),
            None => db.message().to_owned(),
        },
        None => error.to_string(),
    }
}

fn postgres_value(kind: Option<&tokio_postgres::types::Type>, text: &str) -> DatabaseValue {
    use tokio_postgres::types::Type;
    let typed = match kind {
        Some(&Type::BOOL) => Some(DatabaseValue::Bool(text == "t")),
        Some(&Type::INT2 | &Type::INT4 | &Type::INT8 | &Type::OID) => {
            text.parse().ok().map(DatabaseValue::Integer)
        }
        Some(&Type::FLOAT4 | &Type::FLOAT8) => text.parse().ok().map(DatabaseValue::Float),
        Some(&Type::BYTEA) => text
            .strip_prefix("\\x")
            .and_then(hex)
            .map(DatabaseValue::Bytes),
        _ => None,
    };
    typed.unwrap_or_else(|| DatabaseValue::Text(text.to_owned()))
}

fn hex(text: &str) -> Option<Vec<u8>> {
    text.len().is_multiple_of(2).then_some(())?;
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(text.get(index..index + 2)?, 16).ok())
        .collect()
}

/// MySQL by the text protocol (several statements at once); the column types say what each
/// value is. The last result set with columns is the one shown.
async fn mysql_query(connection: &mut mysql_async::Conn, query: &str) -> Result<QueryResult> {
    use mysql_async::{Row, Value, consts::ColumnType, prelude::Queryable};
    let started = Instant::now();
    let failed = |error: mysql_async::Error| anyhow!("Query failed: {error}");
    let mut sets = connection.query_iter(query).await.map_err(failed)?;
    let mut result = QueryResult {
        columns: Vec::new(),
        rows: Vec::new(),
        affected_rows: 0,
        duration: Duration::ZERO,
        more: false,
    };
    while !sets.is_empty() {
        let columns = sets.columns().unwrap_or_else(|| Vec::new().into());
        let rows: Vec<Row> = sets.collect().await.map_err(failed)?;
        if columns.is_empty() {
            result.affected_rows += sets.affected_rows();
            continue;
        }
        result.columns = columns
            .iter()
            .map(|column| column.name_str().into_owned())
            .collect();
        result.rows =
            rows.iter()
                .map(|row| {
                    columns
                        .iter()
                        .enumerate()
                        .map(|(index, column)| match row.as_ref(index) {
                            None | Some(Value::NULL) => DatabaseValue::Null,
                            Some(Value::Int(value)) => DatabaseValue::Integer(*value),
                            Some(Value::UInt(value)) => i64::try_from(*value).map_or_else(
                                |_| DatabaseValue::Text(value.to_string()),
                                DatabaseValue::Integer,
                            ),
                            Some(Value::Float(value)) => DatabaseValue::Float(f64::from(*value)),
                            Some(Value::Double(value)) => DatabaseValue::Float(*value),
                            Some(Value::Bytes(bytes)) => {
                                let text = || String::from_utf8_lossy(bytes).into_owned();
                                match column.column_type() {
                                    ColumnType::MYSQL_TYPE_TINY
                                    | ColumnType::MYSQL_TYPE_SHORT
                                    | ColumnType::MYSQL_TYPE_INT24
                                    | ColumnType::MYSQL_TYPE_LONG
                                    | ColumnType::MYSQL_TYPE_LONGLONG
                                    | ColumnType::MYSQL_TYPE_YEAR => text().parse().map_or_else(
                                        |_| DatabaseValue::Text(text()),
                                        DatabaseValue::Integer,
                                    ),
                                    ColumnType::MYSQL_TYPE_FLOAT
                                    | ColumnType::MYSQL_TYPE_DOUBLE => text().parse().map_or_else(
                                        |_| DatabaseValue::Text(text()),
                                        DatabaseValue::Float,
                                    ),
                                    // Character set 63 is `binary`: BLOB, BINARY and VARBINARY.
                                    _ if column.character_set() == 63 => {
                                        DatabaseValue::Bytes(bytes.clone())
                                    }
                                    _ => DatabaseValue::Text(text()),
                                }
                            }
                            Some(other) => DatabaseValue::Text(
                                other.as_sql(true).trim_matches('\'').to_owned(),
                            ),
                        })
                        .collect()
                })
                .collect();
    }
    result.duration = started.elapsed();
    Ok(result)
}

pub(crate) async fn persist_connections(
    store: ConnectionStore,
    connections: Vec<ConnectionConfig>,
) -> Result<()> {
    tokio::task::spawn_blocking(move || store.save(&connections))
        .await
        .map_err(|_| anyhow!("Connection metadata save task failed"))?
}

/// Where this system keeps the passwords: on Linux the Secret Service (GNOME Keyring, KWallet)
/// when one runs, else the kernel keyring, which forgets them at the next start (Q04).
pub(crate) const VAULT: Option<&str> = if cfg!(target_os = "macos") {
    Some("Keychain")
} else if cfg!(windows) {
    Some("Credential Manager")
} else if cfg!(target_os = "linux") {
    Some("system keyring")
} else {
    None
};

/// What the panel says after a connection is saved: where its password went, or that it went
/// nowhere, so nobody finds out when it is asked for again.
pub(crate) fn saved_status(updated: bool, password: bool, vault: Option<&str>) -> String {
    let saved = if updated { "updated" } else { "added" };
    match (password, vault) {
        (false, _) => format!("Connection {saved} and tested"),
        (true, Some(vault)) => {
            format!("Connection {saved} and tested; password saved in the {vault}")
        }
        (true, None) => format!(
            "Connection {saved} and tested; no system keyring here yet: the password is not saved"
        ),
    }
}

/// What to do when Linux has no Secret Service to keep a password in.
const NO_SERVICE: &str = if cfg!(target_os = "linux") {
    ": start GNOME Keyring or KWallet (the Secret Service) and save again"
} else {
    ""
};

fn entry(id: &str) -> Result<keyring::Entry> {
    // The tests never touch the keyring of whoever runs them.
    #[cfg(test)]
    {
        static MOCK: std::sync::Once = std::sync::Once::new();
        MOCK.call_once(|| {
            keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
        });
    }
    keyring::Entry::new(KEYRING_SERVICE, id)
        .map_err(|_| anyhow!("Could not access the system keyring"))
}

/// Whether the error says there is no keyring service on this system (a Linux without GNOME
/// Keyring or KWallet, a container): nothing can be stored there, so nothing is to be read or
/// removed (E82).
fn no_service(error: &keyring::Error) -> bool {
    matches!(
        error,
        keyring::Error::NoStorageAccess(_) | keyring::Error::PlatformFailure(_)
    )
}

pub(crate) async fn load_password(id: String) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        let entry = entry(&id)?;
        match entry.get_password() {
            Ok(password) => Ok(password),
            Err(keyring::Error::NoEntry) => Ok(String::new()),
            Err(error) if no_service(&error) => Ok(String::new()),
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
        let entry = entry(&id)?;
        if password.is_empty() {
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(error) if no_service(&error) => Ok(()),
                Err(_) => Err(anyhow!("Could not remove the old keyring password")),
            }
        } else {
            entry.set_password(&password).map_err(|_| {
                anyhow!("Could not save the password in the system keyring{NO_SERVICE}")
            })
        }
    })
    .await
    .map_err(|_| anyhow!("Keyring write task failed"))?
}

pub(crate) async fn delete_password(id: String) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let entry = entry(&id)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) if no_service(&error) => Ok(()),
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
    use crate::database::DatabasePanel;

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
            tls: Default::default(),
        };
        let store = ConnectionStore { path: path.clone() };
        store.save(std::slice::from_ref(&config)).unwrap();
        let loaded = DatabasePanel::with_config_path(path).unwrap();
        assert_eq!(loaded.connections, vec![config]);
    }

    #[test]
    fn saving_says_where_the_password_went_or_that_it_was_not_saved() {
        assert_eq!(
            saved_status(false, false, VAULT),
            "Connection added and tested"
        );
        assert_eq!(
            saved_status(true, true, Some("Keychain")),
            "Connection updated and tested; password saved in the Keychain"
        );
        let nowhere = saved_status(false, true, None);
        assert!(nowhere.ends_with("the password is not saved"), "{nowhere}");
        // The vault of this system is the one its users know by name.
        assert_eq!(
            VAULT.is_some(),
            cfg!(any(target_os = "macos", windows, target_os = "linux"))
        );
    }

    /// The real vault of this machine: run by hand with `cargo test -- --ignored vault`.
    #[test]
    #[ignore = "writes to the keyring of whoever runs it"]
    fn the_vault_of_this_system_keeps_a_password_for_the_next_entry() {
        let Some(_) = VAULT else { return };
        let id = new_connection_id();
        let entry = || {
            let builder = keyring::default::default_credential_builder();
            let credential = builder.build(None, KEYRING_SERVICE, &id).unwrap();
            keyring::Entry::new_with_credential(credential)
        };
        entry().set_password("s3cret-for-a-test").unwrap();
        assert_eq!(entry().get_password().unwrap(), "s3cret-for-a-test");
        entry().delete_credential().unwrap();
        assert!(matches!(
            entry().get_password(),
            Err(keyring::Error::NoEntry)
        ));
    }

    #[test]
    fn sqlite_urls_open_what_they_say_and_strange_ones_are_refused() {
        use rusqlite::OpenFlags;
        let (path, flags) = sqlite_target("sqlite://?mode=memory").unwrap();
        assert!(path.is_none() && flags.contains(OpenFlags::SQLITE_OPEN_CREATE));
        assert!(sqlite_target("sqlite::memory:").unwrap().0.is_none());
        let (path, flags) = sqlite_target("sqlite:///C:/data/a b.db?mode=rwc").unwrap();
        assert_eq!(path.as_deref(), Some("C:/data/a b.db"));
        assert!(flags.contains(OpenFlags::SQLITE_OPEN_CREATE));
        // Without a mode a file is opened, never created, as before.
        let (path, flags) = sqlite_target("sqlite:/srv/x.db").unwrap();
        assert_eq!(path.as_deref(), Some("/srv/x.db"));
        assert!(!flags.contains(OpenFlags::SQLITE_OPEN_CREATE));
        let read_only = sqlite_target("sqlite://x.db?mode=ro").unwrap().1;
        assert!(read_only.contains(OpenFlags::SQLITE_OPEN_READ_ONLY));
        assert!(sqlite_target("sqlite://x.db?mode=rw;drop").is_err());
        assert!(sqlite_target("postgres://x").is_err());
        assert!(sqlite_target("").is_err());
    }

    #[test]
    fn the_tls_mode_leaves_the_url_and_is_read_by_sozoide() {
        assert_eq!(
            take_tls_mode("postgres://h/db"),
            ("postgres://h/db".into(), None)
        );
        assert_eq!(
            take_tls_mode("postgres://h/db?SSLMode=Verify_Full&application_name=x"),
            (
                "postgres://h/db?application_name=x".into(),
                Some("verify-full".into())
            )
        );
        assert_eq!(
            take_tls_mode("mysql://h/db?ssl-mode=REQUIRED"),
            ("mysql://h/db".into(), Some("required".into()))
        );
        assert_eq!(
            take_tls_mode("mysql://h/db?&&"),
            ("mysql://h/db".into(), None)
        );
        assert!(tls_config(true).is_ok() && tls_config(false).is_ok());
    }

    #[tokio::test]
    async fn an_unknown_tls_mode_or_a_server_that_does_not_answer_says_so() {
        for (driver, url) in [
            (
                Driver::PostgreSQL,
                "postgres://u@127.0.0.1:1/db?sslmode=maybe",
            ),
            (Driver::MySQL, "mysql://u@127.0.0.1:1/db?ssl-mode=maybe"),
        ] {
            let mut form = ConnectionForm::new(driver);
            form.name = "x".into();
            form.url = url.into();
            let error = open_pool(&form).await.err().unwrap().to_string();
            assert!(error.contains("unknown ssl"), "{error}");
        }
        let mut form = ConnectionForm::new(Driver::PostgreSQL);
        form.name = "x".into();
        form.url = "postgres://u@127.0.0.1:1/db".into();
        // Not a real one: what the message must never repeat.
        let typed = "never-in-messages";
        form.password = typed.into();
        let error = open_pool(&form).await.err().unwrap().to_string();
        assert!(error.starts_with("PostgreSQL connection failed"), "{error}");
        assert!(!error.contains(typed), "{error}");
    }

    #[test]
    fn postgres_text_keeps_its_type_and_bad_text_stays_text() {
        use tokio_postgres::types::Type;
        assert_eq!(
            postgres_value(Some(&Type::BOOL), "t"),
            DatabaseValue::Bool(true)
        );
        assert_eq!(
            postgres_value(Some(&Type::INT8), "-7"),
            DatabaseValue::Integer(-7)
        );
        assert_eq!(
            postgres_value(Some(&Type::FLOAT8), "1.5"),
            DatabaseValue::Float(1.5)
        );
        assert_eq!(
            postgres_value(Some(&Type::BYTEA), "\\x00ff"),
            DatabaseValue::Bytes(vec![0, 255])
        );
        for (kind, text) in [
            (Type::INT4, "12x"),
            (Type::BYTEA, "\\x0"),
            (Type::BYTEA, "\\xzz"),
            (Type::BYTEA, "\\xé1"),
        ] {
            assert_eq!(
                postgres_value(Some(&kind), text),
                DatabaseValue::Text(text.into())
            );
        }
        assert_eq!(postgres_value(None, "2"), DatabaseValue::Text("2".into()));
    }

    #[tokio::test]
    async fn sqlite_runs_every_statement_and_a_cancelled_query_stops_in_sqlite() {
        let mut form = ConnectionForm::new(Driver::SQLite);
        form.name = "x".into();
        form.database = ":memory:".into();
        let pool = open_pool(&form).await.unwrap();
        let both = run_query(
            &pool,
            "CREATE TABLE t (ok BOOLEAN, n INTEGER); INSERT INTO t VALUES (1, 2), (0, 3);\
             SELECT ok, n FROM t ORDER BY n",
        )
        .await
        .unwrap();
        assert_eq!(both.columns, ["ok", "n"]);
        assert_eq!(
            both.rows[0],
            [DatabaseValue::Bool(true), DatabaseValue::Integer(2)]
        );
        assert_eq!(
            run_query(&pool, "DELETE FROM t")
                .await
                .unwrap()
                .affected_rows,
            2
        );
        assert!(run_query(&pool, "SELECT * FROM missing").await.is_err());

        // A query of many seconds, dropped after 100 ms as the panel does on cancel: without
        // the interrupt the next query waits for it to end.
        let endless = "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n \
                       WHERE i < 300000000) SELECT count(*) FROM n";
        let cancelled =
            tokio::time::timeout(Duration::from_millis(100), run_query(&pool, endless)).await;
        assert!(cancelled.is_err());
        let started = Instant::now();
        let next = run_query(&pool, "SELECT 1").await.unwrap();
        assert_eq!(next.rows, [[DatabaseValue::Integer(1)]]);
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_broken_connections_file_opens_with_none_and_is_kept_aside() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("connections.toml");
        std::fs::write(&path, "[[connections]]\nid = 7\nname = [").unwrap();
        let loaded = DatabasePanel::with_config_path(path.clone()).unwrap();
        assert!(loaded.connections.is_empty());
        assert!(directory.path().join("connections.toml.broken").exists());
        assert!(!path.exists());
    }

    #[test]
    fn without_a_keyring_service_nothing_is_stored_so_nothing_is_read_or_removed() {
        assert!(no_service(&keyring::Error::NoStorageAccess(
            "no Secret Service".into()
        )));
        assert!(no_service(&keyring::Error::PlatformFailure("dbus".into())));
        assert!(!no_service(&keyring::Error::NoEntry));
        assert!(!no_service(&keyring::Error::BadEncoding(vec![0xff])));
    }
}
