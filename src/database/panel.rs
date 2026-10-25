#[cfg(test)]
use std::path::Path;
use std::{
    collections::HashMap,
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Context, Result, anyhow, bail};
use sqlx::AnyPool;

use super::{
    model::{
        ConnectionConfig, ConnectionForm, DatabaseTab, QueryCancellation, QueryHistoryEntry,
        QueryResult, next_char_boundary, previous_char_boundary,
    },
    store::{
        ConnectionStore, delete_password, load_password, open_pool, persist_connections, run_query,
        save_password,
    },
};

/// UI-independent database engine and all state needed by a Ratatui panel.
pub struct DatabasePanel {
    pub connections: Vec<ConnectionConfig>,
    pub active_connection: Option<String>,
    pub query_text: String,
    pub query_cursor: usize,
    pub results: Option<QueryResult>,
    pub history: Vec<QueryHistoryEntry>,
    pub busy: bool,
    pub error: Option<String>,
    pub status: Option<String>,
    pub tab: DatabaseTab,
    pub selection: usize,
    store: ConnectionStore,
    pools: HashMap<String, AnyPool>,
    active_query: Option<QueryCancellation>,
}

impl DatabasePanel {
    pub fn new() -> Result<Self> {
        Self::with_store(ConnectionStore::discover()?)
    }

    /// Uses a specific metadata file, useful for portable profiles and isolated tests.
    #[cfg(test)]
    pub fn with_config_path(path: impl Into<std::path::PathBuf>) -> Result<Self> {
        Self::with_store(ConnectionStore { path: path.into() })
    }

    fn with_store(store: ConnectionStore) -> Result<Self> {
        let connections = store.load()?;
        Ok(Self {
            connections,
            active_connection: None,
            query_text: String::new(),
            query_cursor: 0,
            results: None,
            history: Vec::new(),
            busy: false,
            error: None,
            status: None,
            tab: DatabaseTab::Connections,
            selection: 0,
            store,
            pools: HashMap::new(),
            active_query: None,
        })
    }

    #[cfg(test)]
    pub fn config_path(&self) -> &Path {
        &self.store.path
    }

    pub fn active_config(&self) -> Option<&ConnectionConfig> {
        let id = self.active_connection.as_deref()?;
        self.connections
            .iter()
            .find(|connection| connection.id == id)
    }

    pub fn selected_config(&self) -> Option<&ConnectionConfig> {
        self.connections.get(self.selection)
    }

    pub fn select_next(&mut self) {
        if !self.connections.is_empty() {
            self.selection = (self.selection + 1) % self.connections.len();
        }
    }

    pub fn select_previous(&mut self) {
        if !self.connections.is_empty() {
            self.selection = (self.selection + self.connections.len() - 1) % self.connections.len();
        }
    }

    pub fn set_active_connection(&mut self, id: &str) -> Result<()> {
        let connection = self
            .connections
            .iter()
            .find(|connection| connection.id == id)
            .context("Unknown connection")?;
        let name = connection.name.clone();
        self.active_connection = Some(id.into());
        self.status = Some(format!("Active connection: {name}"));
        self.error = None;
        Ok(())
    }

    pub async fn test_connection(&self, form: &ConnectionForm) -> Result<Duration> {
        let started = Instant::now();
        let pool = open_pool(form).await?;
        sqlx::query("SELECT 1")
            .fetch_one(&pool)
            .await
            .map_err(|error| anyhow!("Connection test query failed: {error}"))?;
        pool.close().await;
        Ok(started.elapsed())
    }

    /// Tests the form, stores metadata and any password, then makes it active.
    pub async fn add_connection(&mut self, form: &ConnectionForm) -> Result<String> {
        let pool = open_pool(form).await?;
        sqlx::query("SELECT 1")
            .fetch_one(&pool)
            .await
            .map_err(|error| anyhow!("Connection test query failed: {error}"))?;
        let config = form.to_config()?;
        let id = config.id.clone();
        let is_update = self
            .connections
            .iter()
            .any(|connection| connection.id == id);

        save_password(id.clone(), form.password.clone()).await?;
        let mut saved = self.connections.clone();
        if let Some(existing) = saved.iter_mut().find(|connection| connection.id == id) {
            *existing = config.clone();
        } else {
            saved.push(config.clone());
        }
        if let Err(error) = persist_connections(self.store.clone(), saved).await {
            if !is_update {
                let _ = delete_password(id.clone()).await;
            }
            pool.close().await;
            return Err(error);
        }

        if let Some(old_pool) = self.pools.insert(id.clone(), pool) {
            old_pool.close().await;
        }
        if let Some(existing) = self
            .connections
            .iter_mut()
            .find(|connection| connection.id == id)
        {
            *existing = config;
        } else {
            self.connections.push(config);
        }
        self.selection = self
            .connections
            .iter()
            .position(|connection| connection.id == id)
            .unwrap_or(0);
        self.active_connection = Some(id.clone());
        self.status = Some(if is_update {
            "Connection updated and tested".into()
        } else {
            "Connection added and tested".into()
        });
        self.error = None;
        Ok(id)
    }

    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query_text = query.into();
        self.query_cursor = self.query_text.len();
    }

    pub fn insert_query(&mut self, text: &str) {
        self.clamp_query_cursor();
        self.query_text.insert_str(self.query_cursor, text);
        self.query_cursor += text.len();
    }

    pub fn query_backspace(&mut self) {
        self.query_left();
        self.query_delete();
    }

    pub fn query_delete(&mut self) {
        self.clamp_query_cursor();
        if self.query_cursor < self.query_text.len() {
            let end = next_char_boundary(&self.query_text, self.query_cursor);
            self.query_text.replace_range(self.query_cursor..end, "");
        }
    }

    pub fn query_left(&mut self) {
        self.clamp_query_cursor();
        self.query_cursor = previous_char_boundary(&self.query_text, self.query_cursor);
    }

    pub fn query_right(&mut self) {
        self.clamp_query_cursor();
        self.query_cursor = next_char_boundary(&self.query_text, self.query_cursor);
    }

    pub fn query_home(&mut self) {
        self.query_cursor = 0;
    }

    pub fn query_end(&mut self) {
        self.query_cursor = self.query_text.len();
    }

    fn clamp_query_cursor(&mut self) {
        self.query_cursor = self.query_cursor.min(self.query_text.len());
        while !self.query_text.is_char_boundary(self.query_cursor) {
            self.query_cursor -= 1;
        }
    }

    #[cfg(test)]
    pub async fn execute_query(&mut self) -> Result<QueryResult> {
        self.execute_query_cancellable(QueryCancellation::new())
            .await
    }

    /// The cloned cancellation handle can be triggered by another task while this awaits.
    pub async fn execute_query_cancellable(
        &mut self,
        cancellation: QueryCancellation,
    ) -> Result<QueryResult> {
        if self.busy {
            bail!("A query is already running");
        }
        let connection = self
            .active_config()
            .context("No active connection")?
            .clone();
        let query = self.query_text.trim().to_owned();
        if query.is_empty() {
            bail!("Query is empty");
        }
        self.busy = true;
        self.error = None;
        self.status = Some("Running query...".into());
        self.active_query = Some(cancellation.clone());
        let started = Instant::now();

        let pool = match self.pool_for(&connection).await {
            Ok(pool) => pool,
            Err(error) => {
                self.finish_query();
                self.record_history(
                    &connection,
                    &query,
                    started.elapsed(),
                    Err(error.to_string()),
                );
                self.error = Some(error.to_string());
                self.status = Some("Could not open connection".into());
                return Err(error);
            }
        };
        let mut cancelled = cancellation.sender.subscribe();
        let execution = run_query(&pool, &query);
        tokio::pin!(execution);
        let result = if cancellation.is_cancelled() {
            Err(anyhow!("Query cancelled"))
        } else {
            tokio::select! {
                biased;
                changed = cancelled.changed() => {
                    if changed.is_ok() && *cancelled.borrow() {
                        Err(anyhow!("Query cancelled"))
                    } else {
                        execution.await
                    }
                },
                result = &mut execution => result,
            }
        };
        let duration = started.elapsed();
        self.finish_query();

        match result {
            Ok(mut result) => {
                result.duration = duration;
                self.record_history(&connection, &query, duration, Ok(result.rows.len()));
                self.status = Some(format!(
                    "{} row(s), {} affected in {} ms",
                    result.rows.len(),
                    result.affected_rows,
                    duration.as_millis()
                ));
                self.results = Some(result.clone());
                self.tab = DatabaseTab::Results;
                Ok(result)
            }
            Err(error) => {
                let message = error.to_string();
                self.record_history(&connection, &query, duration, Err(message.clone()));
                self.error = Some(message);
                self.status = Some("Query failed".into());
                Err(error)
            }
        }
    }

    pub fn cancel_query(&self) -> bool {
        if let Some(cancellation) = &self.active_query {
            cancellation.cancel();
            true
        } else {
            false
        }
    }

    fn finish_query(&mut self) {
        self.busy = false;
        self.active_query = None;
    }

    fn record_history(
        &mut self,
        connection: &ConnectionConfig,
        query: &str,
        duration: Duration,
        result: Result<usize, String>,
    ) {
        self.history.push(QueryHistoryEntry {
            connection_id: connection.id.clone(),
            connection_name: connection.name.clone(),
            query: query.into(),
            executed_at: SystemTime::now(),
            duration,
            result,
        });
    }

    async fn pool_for(&mut self, config: &ConnectionConfig) -> Result<AnyPool> {
        if let Some(pool) = self.pools.get(&config.id) {
            return Ok(pool.clone());
        }
        let password = load_password(config.id.clone()).await?;
        let form = ConnectionForm::from_config(config, password);
        let pool = open_pool(&form).await?;
        self.pools.insert(config.id.clone(), pool.clone());
        Ok(pool)
    }

    pub async fn close(&mut self) {
        let pools = std::mem::take(&mut self.pools);
        for (_, pool) in pools {
            pool.close().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{DatabaseValue, Driver};
    use std::fs;
    use tempfile::TempDir;

    fn sqlite_form(directory: &TempDir, name: &str) -> ConnectionForm {
        let mut form = ConnectionForm::new(Driver::SQLite);
        form.name = name.into();
        form.database = directory
            .path()
            .join(format!("{name}.sqlite"))
            .to_string_lossy()
            .into_owned();
        form
    }

    fn panel(directory: &TempDir) -> DatabasePanel {
        DatabasePanel::with_config_path(directory.path().join("connections.toml")).unwrap()
    }

    #[test]
    fn query_editor_respects_utf8_boundaries() {
        let directory = TempDir::new().unwrap();
        let mut panel = panel(&directory);
        panel.set_query("aé🙂b");
        panel.query_left();
        panel.query_backspace();
        assert_eq!(panel.query_text, "aéb");
        panel.query_home();
        panel.query_right();
        panel.query_delete();
        assert_eq!(panel.query_text, "ab");
    }

    #[tokio::test]
    async fn sqlite_connection_and_queries_are_end_to_end() {
        let directory = TempDir::new().unwrap();
        let mut panel = panel(&directory);
        let form = sqlite_form(&directory, "workspace");
        let tested = panel.test_connection(&form).await.unwrap();
        assert!(tested <= Duration::from_secs(10));

        let id = panel.add_connection(&form).await.unwrap();
        assert_eq!(panel.active_connection.as_deref(), Some(id.as_str()));
        assert_eq!(panel.connections.len(), 1);
        assert!(panel.config_path().exists());
        let metadata = fs::read_to_string(panel.config_path()).unwrap();
        assert!(!metadata.contains("password"));

        panel.set_query(
            "CREATE TABLE items (\
                id INTEGER PRIMARY KEY, \
                name TEXT NOT NULL, \
                score REAL, \
                payload BLOB, \
                optional TEXT\
            )",
        );
        assert_eq!(panel.execute_query().await.unwrap().affected_rows, 0);

        panel.set_query(
            "INSERT INTO items (name, score, payload, optional) \
             VALUES ('first', 1.5, X'0001FF', NULL)",
        );
        let inserted = panel.execute_query().await.unwrap();
        assert_eq!(inserted.affected_rows, 1);

        panel.set_query("INSERT INTO items (name, score) VALUES ('second', 2.5)");
        panel.execute_query().await.unwrap();
        panel.set_query("SELECT id, name, score, payload, optional FROM items ORDER BY id");
        let result = panel.execute_query().await.unwrap();
        assert_eq!(
            result.columns,
            ["id", "name", "score", "payload", "optional"]
        );
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.cell(0, 0), Some(&DatabaseValue::Integer(1)));
        assert_eq!(
            result.cell(0, 1),
            Some(&DatabaseValue::Text("first".into()))
        );
        assert_eq!(result.cell(0, 2), Some(&DatabaseValue::Float(1.5)));
        assert_eq!(
            result.cell(0, 3),
            Some(&DatabaseValue::Bytes(vec![0, 1, 255]))
        );
        assert_eq!(result.cell(0, 4), Some(&DatabaseValue::Null));
        panel.set_query("SELECT id, name FROM items WHERE 0");
        let empty = panel.execute_query().await.unwrap();
        assert_eq!(empty.columns, ["id", "name"]);
        assert!(empty.rows.is_empty());
        assert_eq!(panel.history.len(), 5);
        assert_eq!(panel.tab, DatabaseTab::Results);
        assert!(panel.error.is_none());

        panel.set_query("UPDATE items SET optional = 'set' WHERE id = 1");
        assert_eq!(panel.execute_query().await.unwrap().affected_rows, 1);
        panel.set_query("SELECT optional FROM items WHERE id = 1");
        assert_eq!(
            panel.execute_query().await.unwrap().cell(0, 0),
            Some(&DatabaseValue::Text("set".into()))
        );
        panel.close().await;
    }

    #[tokio::test]
    async fn sqlite_memory_connections_and_cancellation_work() {
        let directory = TempDir::new().unwrap();
        let mut panel = panel(&directory);
        let mut form = ConnectionForm::new(Driver::SQLite);
        form.name = "Memory".into();
        form.database = ":memory:".into();
        panel.add_connection(&form).await.unwrap();
        panel.set_query("CREATE TABLE values_table (value TEXT)");
        panel.execute_query().await.unwrap();
        panel.set_query("INSERT INTO values_table VALUES ('kept')");
        panel.execute_query().await.unwrap();
        panel.set_query("SELECT value FROM values_table");
        assert_eq!(
            panel.execute_query().await.unwrap().cell(0, 0),
            Some(&DatabaseValue::Text("kept".into()))
        );

        let cancellation = QueryCancellation::new();
        cancellation.cancel();
        panel.set_query("SELECT 1");
        let error = panel
            .execute_query_cancellable(cancellation)
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "Query cancelled");
        assert!(!panel.busy);
        assert!(!panel.cancel_query());
        assert_eq!(
            panel.history.last().unwrap().result,
            Err("Query cancelled".into())
        );
        panel.close().await;
    }

    #[tokio::test]
    async fn query_errors_update_state_without_losing_connection() {
        let directory = TempDir::new().unwrap();
        let mut panel = panel(&directory);
        let form = sqlite_form(&directory, "errors");
        panel.add_connection(&form).await.unwrap();
        panel.set_query("SELECT * FROM missing_table");
        assert!(panel.execute_query().await.is_err());
        assert!(!panel.busy);
        assert!(panel.error.is_some());
        assert_eq!(panel.status.as_deref(), Some("Query failed"));
        assert!(panel.active_config().is_some());
        assert_eq!(panel.history.len(), 1);
        panel.close().await;
    }
}
