use std::{
    fmt,
    str::FromStr,
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use super::store::{
    add_url_password, new_connection_id, nonempty, percent_encode, reject_url_password,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Driver {
    #[default]
    SQLite,
    PostgreSQL,
    MySQL,
}

impl Driver {
    pub const ALL: [Self; 3] = [Self::SQLite, Self::PostgreSQL, Self::MySQL];

    pub fn label(self) -> &'static str {
        match self {
            Self::SQLite => "SQLite",
            Self::PostgreSQL => "PostgreSQL",
            Self::MySQL => "MySQL",
        }
    }

    pub fn default_port(self) -> Option<u16> {
        match self {
            Self::SQLite => None,
            Self::PostgreSQL => Some(5432),
            Self::MySQL => Some(3306),
        }
    }

    fn scheme(self) -> &'static str {
        match self {
            Self::SQLite => "sqlite",
            Self::PostgreSQL => "postgres",
            Self::MySQL => "mysql",
        }
    }
}

impl fmt::Display for Driver {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

impl FromStr for Driver {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "sqlite" | "sqlite3" => Ok(Self::SQLite),
            "postgres" | "postgresql" => Ok(Self::PostgreSQL),
            "mysql" => Ok(Self::MySQL),
            _ => bail!("Unsupported database driver; use SQLite, PostgreSQL, or MySQL"),
        }
    }
}

/// Serializable connection metadata. Passwords are deliberately not part of this type.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionConfig {
    #[serde(default = "new_connection_id")]
    pub id: String,
    pub name: String,
    pub driver: Driver,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
}

impl fmt::Debug for ConnectionConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectionConfig")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("driver", &self.driver)
            .field("url", &self.url.as_ref().map(|_| "<redacted>"))
            .field("database", &self.database)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConnectionField {
    #[default]
    Name,
    Driver,
    Url,
    Database,
    Host,
    Port,
    Username,
    Password,
}

impl ConnectionField {
    pub const ALL: [Self; 8] = [
        Self::Name,
        Self::Driver,
        Self::Url,
        Self::Database,
        Self::Host,
        Self::Port,
        Self::Username,
        Self::Password,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Driver => "Driver",
            Self::Url => "URL",
            Self::Database => "Database",
            Self::Host => "Host",
            Self::Port => "Port",
            Self::Username => "Username",
            Self::Password => "Password",
        }
    }

    pub fn is_secret(self) -> bool {
        self == Self::Password
    }
}

#[derive(Clone)]
pub struct ConnectionForm {
    pub name: String,
    pub driver: Driver,
    pub url: String,
    pub database: String,
    pub host: String,
    pub port: String,
    pub username: String,
    pub password: String,
    pub field: ConnectionField,
    pub cursor: usize,
    editing_id: Option<String>,
}

impl fmt::Debug for ConnectionForm {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectionForm")
            .field("name", &self.name)
            .field("driver", &self.driver)
            .field("url", &"<redacted>")
            .field("database", &self.database)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("field", &self.field)
            .field("cursor", &self.cursor)
            .field("editing_id", &self.editing_id)
            .finish()
    }
}

impl Default for ConnectionForm {
    fn default() -> Self {
        Self::new(Driver::SQLite)
    }
}

impl ConnectionForm {
    pub fn new(driver: Driver) -> Self {
        let mut form = Self {
            name: String::new(),
            driver,
            url: String::new(),
            database: String::new(),
            host: String::new(),
            port: String::new(),
            username: String::new(),
            password: String::new(),
            field: ConnectionField::Name,
            cursor: 0,
            editing_id: None,
        };
        form.set_driver(driver);
        form
    }

    pub fn from_config(config: &ConnectionConfig, password: String) -> Self {
        Self {
            name: config.name.clone(),
            driver: config.driver,
            url: config.url.clone().unwrap_or_default(),
            database: config.database.clone().unwrap_or_default(),
            host: config.host.clone().unwrap_or_default(),
            port: config
                .port
                .map_or_else(String::new, |port| port.to_string()),
            username: config.username.clone().unwrap_or_default(),
            password,
            field: ConnectionField::Name,
            cursor: 0,
            editing_id: Some(config.id.clone()),
        }
    }

    pub fn editing_id(&self) -> Option<&str> {
        self.editing_id.as_deref()
    }

    pub fn set_driver(&mut self, driver: Driver) {
        let old_default = self.driver.default_port();
        let entered_port = self.port.parse::<u16>().ok();
        self.driver = driver;
        if self.field == ConnectionField::Driver {
            self.cursor = 0;
        }
        if self.host.is_empty() && driver != Driver::SQLite {
            self.host = "localhost".into();
        }
        if entered_port == old_default || self.port.is_empty() {
            self.port = driver
                .default_port()
                .map_or_else(String::new, |port| port.to_string());
        }
        self.clamp_cursor();
    }

    pub fn cycle_driver(&mut self, forward: bool) {
        let index = Driver::ALL
            .iter()
            .position(|driver| *driver == self.driver)
            .unwrap_or(0);
        let next = if forward {
            (index + 1) % Driver::ALL.len()
        } else {
            (index + Driver::ALL.len() - 1) % Driver::ALL.len()
        };
        self.set_driver(Driver::ALL[next]);
    }

    pub fn select_field(&mut self, field: ConnectionField) {
        self.field = field;
        self.cursor = self.selected_value().len();
    }

    pub fn next_field(&mut self) {
        let index = ConnectionField::ALL
            .iter()
            .position(|field| *field == self.field)
            .unwrap_or(0);
        self.select_field(ConnectionField::ALL[(index + 1) % ConnectionField::ALL.len()]);
    }

    pub fn previous_field(&mut self) {
        let index = ConnectionField::ALL
            .iter()
            .position(|field| *field == self.field)
            .unwrap_or(0);
        self.select_field(
            ConnectionField::ALL
                [(index + ConnectionField::ALL.len() - 1) % ConnectionField::ALL.len()],
        );
    }

    pub fn selected_value(&self) -> &str {
        match self.field {
            ConnectionField::Name => &self.name,
            ConnectionField::Driver => self.driver.label(),
            ConnectionField::Url => &self.url,
            ConnectionField::Database => &self.database,
            ConnectionField::Host => &self.host,
            ConnectionField::Port => &self.port,
            ConnectionField::Username => &self.username,
            ConnectionField::Password => &self.password,
        }
    }

    fn selected_value_mut(&mut self) -> Option<&mut String> {
        match self.field {
            ConnectionField::Name => Some(&mut self.name),
            ConnectionField::Driver => None,
            ConnectionField::Url => Some(&mut self.url),
            ConnectionField::Database => Some(&mut self.database),
            ConnectionField::Host => Some(&mut self.host),
            ConnectionField::Port => Some(&mut self.port),
            ConnectionField::Username => Some(&mut self.username),
            ConnectionField::Password => Some(&mut self.password),
        }
    }

    pub fn insert(&mut self, text: &str) {
        self.clamp_cursor();
        let cursor = self.cursor;
        if let Some(value) = self.selected_value_mut() {
            value.insert_str(cursor, text);
            self.cursor += text.len();
        }
    }

    pub fn backspace(&mut self) {
        self.move_left();
        self.delete();
    }

    pub fn delete(&mut self) {
        self.clamp_cursor();
        let cursor = self.cursor;
        let Some(value) = self.selected_value_mut() else {
            return;
        };
        if cursor == value.len() {
            return;
        }
        let end = next_char_boundary(value, cursor);
        value.replace_range(cursor..end, "");
    }

    pub fn move_left(&mut self) {
        self.clamp_cursor();
        self.cursor = previous_char_boundary(self.selected_value(), self.cursor);
    }

    pub fn move_right(&mut self) {
        self.clamp_cursor();
        self.cursor = next_char_boundary(self.selected_value(), self.cursor);
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.selected_value().len();
    }

    fn clamp_cursor(&mut self) {
        self.cursor = self.cursor.min(self.selected_value().len());
        while !self.selected_value().is_char_boundary(self.cursor) {
            self.cursor -= 1;
        }
    }

    pub fn to_config(&self) -> Result<ConnectionConfig> {
        let name = self.name.trim();
        if name.is_empty() {
            bail!("Connection name is required");
        }
        if !self.url.trim().is_empty() {
            reject_url_password(self.url.trim())?;
        }
        if self.driver == Driver::SQLite
            && self.url.trim().is_empty()
            && self.database.trim().is_empty()
        {
            bail!("A SQLite URL or database path is required");
        }
        if self.driver != Driver::SQLite && self.url.trim().is_empty() {
            if self.host.trim().is_empty() || self.database.trim().is_empty() {
                bail!("Host and database are required when no URL is supplied");
            }
            if !self.password.is_empty() && self.username.trim().is_empty() {
                bail!("Username is required when a password is supplied");
            }
        }
        let port = if self.port.trim().is_empty() {
            self.driver.default_port()
        } else {
            Some(
                self.port
                    .trim()
                    .parse::<u16>()
                    .context("Port must be a number between 1 and 65535")?,
            )
        };
        if self.driver != Driver::SQLite && port == Some(0) {
            bail!("Port must be between 1 and 65535");
        }

        Ok(ConnectionConfig {
            id: self.editing_id.clone().unwrap_or_else(new_connection_id),
            name: name.into(),
            driver: self.driver,
            url: nonempty(&self.url),
            database: nonempty(&self.database),
            host: nonempty(&self.host),
            port,
            username: nonempty(&self.username),
        })
    }

    pub(crate) fn connection_url(&self) -> Result<String> {
        let config = self.to_config()?;
        if let Some(url) = config.url.clone() {
            if self.password.is_empty() || config.driver == Driver::SQLite {
                return Ok(url);
            }
            return add_url_password(&url, config.username.as_deref(), &self.password);
        }
        if config.driver == Driver::SQLite {
            let database = config.database.unwrap_or_else(|| ":memory:".into());
            if database == ":memory:" {
                return Ok("sqlite://?mode=memory".into());
            }
            if database.starts_with("sqlite:") {
                return Ok(database);
            }
            let database = database.replace('\\', "/");
            let windows_absolute = database.as_bytes().get(1) == Some(&b':');
            return Ok(format!(
                "sqlite://{}{}?mode=rwc",
                if windows_absolute { "/" } else { "" },
                database
            ));
        }

        let mut url = format!("{}://", config.driver.scheme());
        if let Some(username) = config.username {
            url.push_str(&percent_encode(&username));
            if !self.password.is_empty() {
                url.push(':');
                url.push_str(&percent_encode(&self.password));
            }
            url.push('@');
        }
        let host = config.host.unwrap_or_else(|| "localhost".into());
        if host.contains(':') && !host.starts_with('[') {
            url.push('[');
            url.push_str(&host);
            url.push(']');
        } else {
            url.push_str(&host);
        }
        if let Some(port) = config.port {
            url.push(':');
            url.push_str(&port.to_string());
        }
        url.push('/');
        url.push_str(&percent_encode(&config.database.unwrap_or_default()));
        Ok(url)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DatabaseTab {
    #[default]
    Connections,
    Query,
    Results,
}

impl DatabaseTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::Connections => "Connections",
            Self::Query => "Query",
            Self::Results => "Results",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Connections => Self::Query,
            Self::Query => Self::Results,
            Self::Results => Self::Connections,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::Connections => Self::Results,
            Self::Query => Self::Connections,
            Self::Results => Self::Query,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DatabaseValue {
    Null,
    Bool(bool),
    Integer(i64),
    Float(f64),
    Text(String),
    Bytes(Vec<u8>),
}

impl fmt::Display for DatabaseValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => formatter.write_str("NULL"),
            Self::Bool(value) => value.fmt(formatter),
            Self::Integer(value) => value.fmt(formatter),
            Self::Float(value) => value.fmt(formatter),
            Self::Text(value) => formatter.write_str(value),
            Self::Bytes(value) => {
                for byte in value {
                    write!(formatter, "{byte:02X}")?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<DatabaseValue>>,
    pub affected_rows: u64,
    pub duration: Duration,
}

impl QueryResult {
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn cell(&self, row: usize, column: usize) -> Option<&DatabaseValue> {
        self.rows.get(row)?.get(column)
    }
}

#[derive(Clone, Debug)]
pub struct QueryHistoryEntry {
    pub connection_id: String,
    pub connection_name: String,
    pub query: String,
    pub executed_at: SystemTime,
    pub duration: Duration,
    pub result: Result<usize, String>,
}

#[derive(Clone, Debug)]
pub struct QueryCancellation {
    pub(crate) sender: watch::Sender<bool>,
}

impl QueryCancellation {
    pub fn new() -> Self {
        let (sender, _) = watch::channel(false);
        Self { sender }
    }

    pub fn cancel(&self) {
        self.sender.send_replace(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.sender.borrow()
    }
}

impl Default for QueryCancellation {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn previous_char_boundary(value: &str, cursor: usize) -> usize {
    if cursor == 0 {
        return 0;
    }
    value[..cursor]
        .char_indices()
        .next_back()
        .map_or(0, |(index, _)| index)
}

pub(crate) fn next_char_boundary(value: &str, cursor: usize) -> usize {
    if cursor >= value.len() {
        return value.len();
    }
    value[cursor..]
        .chars()
        .next()
        .map_or(value.len(), |character| cursor + character.len_utf8())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_and_debug_do_not_contain_passwords() {
        let mut form = ConnectionForm::new(Driver::PostgreSQL);
        form.name = "Application".into();
        form.database = "app".into();
        form.username = "developer".into();
        form.password = "very-secret".into();
        let config = form.to_config().unwrap();
        let serialized = toml::to_string_pretty(&config).unwrap();
        assert!(!serialized.contains("very-secret"));
        assert!(!serialized.contains("password"));
        assert!(!format!("{config:?}").contains("very-secret"));
        let form_debug = format!("{form:?}");
        assert!(!form_debug.contains("very-secret"));
        assert!(form_debug.contains("<redacted>"));

        form.url = "postgres://developer:url-secret@localhost/app".into();
        assert!(!format!("{form:?}").contains("url-secret"));
        assert!(form.connection_url().is_err());
    }

    #[test]
    fn separate_password_is_encoded_into_a_url_without_persisting_it() {
        let mut form = ConnectionForm::new(Driver::PostgreSQL);
        form.name = "Application".into();
        form.url = "postgres://developer@localhost/app".into();
        form.password = "p@:ss word".into();
        assert_eq!(
            form.connection_url().unwrap(),
            "postgres://developer:p%40%3Ass%20word@localhost/app"
        );
        assert!(!form.to_config().unwrap().url.unwrap().contains("p%40"));
    }

    #[test]
    fn form_navigation_and_unicode_editing_work() {
        let mut form = ConnectionForm::default();
        form.insert("aéb");
        form.move_left();
        form.backspace();
        assert_eq!(form.name, "ab");
        form.move_home();
        form.delete();
        assert_eq!(form.name, "b");
        form.next_field();
        assert_eq!(form.field, ConnectionField::Driver);
        form.cycle_driver(true);
        assert_eq!(form.driver, Driver::PostgreSQL);
        assert_eq!(form.port, "5432");
        form.previous_field();
        assert_eq!(form.field, ConnectionField::Name);
    }
}
