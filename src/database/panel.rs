/// Whether `query` has a statement that creates, changes or removes tables or views.
fn changes_tables(query: &str) -> bool {
    query.split(';').any(|statement| {
        statement.split_whitespace().next().is_some_and(|word| {
            ["create", "alter", "drop", "rename", "truncate"]
                .iter()
                .any(|kind| word.eq_ignore_ascii_case(kind))
        })
    })
}

#[cfg(test)]
use std::path::Path;
use std::{
    collections::HashMap,
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Context, Result, anyhow, bail};
use sqlx::AnyPool;

use super::{
    builder::{Catalog, Design, build_query, load_catalog},
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
    /// The tables and views of the active connection, and the ones in the diagram.
    pub catalog: Option<Catalog>,
    pub design: Design,
    /// Why the design makes no SQL yet, shown in the SQL tab.
    pub builder_error: Option<String>,
    /// The SQL was typed over: the builder no longer replaces it until rebuilt.
    pub sql_edited: bool,
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
            catalog: None,
            design: Design::default(),
            builder_error: None,
            sql_edited: false,
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

    /// Reads the tables and views of the active connection; a new connection starts with an
    /// empty diagram.
    pub async fn load_catalog(&mut self) -> Result<()> {
        let connection = self
            .active_config()
            .context("No active connection")?
            .clone();
        let pool = self.pool_for(&connection).await?;
        let catalog = load_catalog(&pool, connection.driver).await?;
        self.status = Some(format!(
            "{}: {} tables and views",
            connection.name,
            catalog.objects.len()
        ));
        self.catalog = Some(catalog);
        self.design = Design::default();
        self.set_query("");
        self.sql_edited = false;
        self.rebuild_sql();
        Ok(())
    }

    /// After SQL that changes the tables: the tables and views read again, keeping the SQL,
    /// the design and the results (F35).
    pub async fn reload_catalog(&mut self) -> Result<()> {
        let connection = self
            .active_config()
            .context("No active connection")?
            .clone();
        let pool = self.pool_for(&connection).await?;
        self.catalog = Some(load_catalog(&pool, connection.driver).await?);
        Ok(())
    }

    /// The SQL of the design, unless it was typed over; `force` replaces it anyway.
    pub fn rebuild(&mut self, force: bool) {
        if force {
            self.sql_edited = false;
        }
        self.rebuild_sql();
    }

    fn rebuild_sql(&mut self) {
        let (Some(catalog), Some(driver)) = (
            &self.catalog,
            self.active_config().map(|connection| connection.driver),
        ) else {
            return;
        };
        match build_query(catalog, &self.design, driver) {
            Ok(sql) => {
                self.builder_error = None;
                if !self.sql_edited {
                    self.set_query(sql);
                }
            }
            Err(error) => self.builder_error = Some(error.to_string()),
        }
    }

    /// Changes the design and makes its SQL again.
    pub fn change_design(&mut self, change: impl FnOnce(&mut Design) -> Result<()>) {
        match change(&mut self.design) {
            Ok(()) => self.rebuild_sql(),
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query_text = query.into();
        self.query_cursor = self.query_text.len();
    }

    pub fn insert_query(&mut self, text: &str) {
        self.sql_edited = true;
        self.clamp_query_cursor();
        self.query_text.insert_str(self.query_cursor, text);
        self.query_cursor += text.len();
    }

    pub fn query_backspace(&mut self) {
        self.query_left();
        self.query_delete();
    }

    pub fn query_delete(&mut self) {
        self.sql_edited = true;
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

    /// The cursor to the line above or below, at the same column or the line's end.
    pub fn query_vertical(&mut self, down: bool) {
        self.clamp_query_cursor();
        let before = &self.query_text[..self.query_cursor];
        let start = before.rfind('\n').map_or(0, |index| index + 1);
        let column = before[start..].chars().count();
        let target = if down {
            match self.query_text[self.query_cursor..].find('\n') {
                Some(offset) => self.query_cursor + offset + 1,
                None => return,
            }
        } else {
            match start.checked_sub(1) {
                Some(end) => self.query_text[..end]
                    .rfind('\n')
                    .map_or(0, |index| index + 1),
                None => return,
            }
        };
        let line = self.query_text[target..].split('\n').next().unwrap_or("");
        self.query_cursor = target
            + line
                .char_indices()
                .nth(column)
                .map_or(line.len(), |(index, _)| index);
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
                if changes_tables(&query)
                    && let Err(error) = self.reload_catalog().await
                {
                    self.error = Some(format!("Could not read the tables: {error}"));
                }
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
    async fn sql_that_changes_the_tables_reads_them_again_and_keeps_the_query() {
        assert!(changes_tables("select 1; Create table x (a)"));
        assert!(changes_tables("  drop view v"));
        assert!(!changes_tables(
            "SELECT * FROM created; INSERT INTO t VALUES (1)"
        ));
        let directory = TempDir::new().unwrap();
        let mut panel = panel(&directory);
        let form = sqlite_form(&directory, "shop");
        panel.add_connection(&form).await.unwrap();
        panel.load_catalog().await.unwrap();
        assert!(panel.catalog.as_ref().unwrap().objects.is_empty());
        let names = |panel: &DatabasePanel| {
            panel
                .catalog
                .as_ref()
                .unwrap()
                .objects
                .iter()
                .map(|object| object.name.clone())
                .collect::<Vec<_>>()
        };
        panel.set_query("CREATE TABLE crew (id INTEGER PRIMARY KEY, name TEXT)");
        panel.execute_query().await.unwrap();
        assert_eq!(names(&panel), ["crew"]);
        assert_eq!(
            panel.query_text,
            "CREATE TABLE crew (id INTEGER PRIMARY KEY, name TEXT)"
        );
        // A query that only reads does not read the tables again.
        panel.catalog.as_mut().unwrap().objects.clear();
        panel.set_query("SELECT * FROM crew");
        panel.execute_query().await.unwrap();
        assert!(names(&panel).is_empty());
        panel.set_query("DROP TABLE crew");
        panel.execute_query().await.unwrap();
        assert!(names(&panel).is_empty());
        panel.set_query("CREATE VIEW crews AS SELECT 1 AS one");
        panel.execute_query().await.unwrap();
        assert_eq!(names(&panel), ["crews"]);
        panel.close().await;
    }

    #[tokio::test]
    async fn the_catalog_reads_tables_views_keys_and_the_design_makes_sql() {
        let directory = TempDir::new().unwrap();
        let mut panel = panel(&directory);
        let form = sqlite_form(&directory, "shop");
        panel.add_connection(&form).await.unwrap();
        panel.set_query(
            "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL);\
             CREATE TABLE orders (id INTEGER PRIMARY KEY, \
                 customer_id INTEGER REFERENCES customers, total REAL);\
             CREATE VIEW big_orders AS SELECT * FROM orders WHERE total > 10;\
             INSERT INTO customers VALUES (1, 'Ana'), (2, 'Rui');\
             INSERT INTO orders VALUES (1, 1, 5), (2, 1, 20), (3, 2, 7);",
        );
        panel.execute_query().await.unwrap();
        panel.load_catalog().await.unwrap();
        let catalog = panel.catalog.clone().unwrap();
        let names = catalog
            .objects
            .iter()
            .map(|object| (object.name.as_str(), object.view))
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                ("customers", false),
                ("orders", false),
                ("big_orders", true)
            ]
        );
        let orders = catalog.object("orders").unwrap();
        assert!(orders.columns[0].key && !orders.columns[1].key);
        assert_eq!(orders.columns[2].kind, "REAL");
        // A key to the other table's primary key names its column.
        assert_eq!(
            catalog.keys,
            [crate::database::ForeignKey {
                from: "orders".into(),
                from_column: "customer_id".into(),
                to: "customers".into(),
                to_column: "id".into(),
            }]
        );

        panel.change_design(|design| design.toggle_object("customers"));
        panel.change_design(|design| design.toggle_object("orders"));
        panel.change_design(|design| {
            design.toggle_field("customers", "name");
            design.toggle_field("orders", "total");
            design.fields[1].total = crate::database::Total::Sum;
            design.fields[1].sort = crate::database::Sort::Descending;
            Ok(())
        });
        assert!(!panel.sql_edited && panel.builder_error.is_none());
        assert!(
            panel
                .query_text
                .starts_with("SELECT customers.name,\n       SUM(orders.total)")
        );
        let result = panel.execute_query().await.unwrap();
        assert_eq!(result.columns, ["name", "sum_total"]);
        assert_eq!(result.cell(0, 0), Some(&DatabaseValue::Text("Ana".into())));
        assert_eq!(result.cell(0, 1), Some(&DatabaseValue::Float(25.0)));

        // Typed SQL stays until Rebuild.
        panel.insert_query(" LIMIT 1");
        panel.change_design(|design| {
            design.fields[1].sort = crate::database::Sort::None;
            Ok(())
        });
        assert!(panel.sql_edited && panel.query_text.ends_with("LIMIT 1"));
        panel.rebuild(true);
        assert!(!panel.sql_edited && !panel.query_text.contains("ORDER BY"));
        panel.close().await;
    }

    #[test]
    fn the_sql_cursor_moves_between_lines_at_its_column() {
        let directory = TempDir::new().unwrap();
        let mut panel = panel(&directory);
        panel.set_query("SELECT a\nFROM t\nWHERE x = 1");
        panel.query_vertical(false);
        assert_eq!(&panel.query_text[panel.query_cursor..], "\nWHERE x = 1");
        // The column it lands on is the one it moves on with.
        panel.query_vertical(false);
        assert_eq!(
            &panel.query_text[panel.query_cursor..],
            " a\nFROM t\nWHERE x = 1"
        );
        panel.query_vertical(false);
        assert_eq!(panel.query_cursor, 6);
        panel.query_home();
        panel.query_vertical(true);
        assert_eq!(
            &panel.query_text[panel.query_cursor..],
            "FROM t\nWHERE x = 1"
        );
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
