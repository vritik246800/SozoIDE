//! The Database beyond running SQL (F07, TODO 4.1.9–4.1.13): results exported as CSV, JSON or
//! Markdown, a cell, a row added or a row deleted as a statement with parameters (shown before
//! it runs), the history without secrets, `EXPLAIN`, the names of tables and columns to complete
//! what is typed, and whether a connection is encrypted.

use anyhow::{Result, bail};

use super::{
    builder::{Catalog, identifier},
    model::{DatabaseValue, Driver, QueryResult},
    store::Pool,
};

/// How results leave the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Export {
    Csv,
    Json,
    Markdown,
}

impl Export {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
            Self::Markdown => "md",
        }
    }
}

fn text(value: &DatabaseValue) -> String {
    match value {
        DatabaseValue::Null => String::new(),
        DatabaseValue::Bytes(bytes) => bytes.iter().map(|byte| format!("{byte:02x}")).collect(),
        other => other.to_string(),
    }
}

/// The results as `format`, with the quotes each one needs.
pub fn export(result: &QueryResult, format: Export) -> String {
    match format {
        Export::Csv => {
            // RFC 4180: a field with a comma, a quote or a line end goes in quotes, its quotes
            // doubled.
            let field = |value: String| {
                if value.contains([',', '"', '\n', '\r']) {
                    format!("\"{}\"", value.replace('"', "\"\""))
                } else {
                    value
                }
            };
            let mut out = result
                .columns
                .iter()
                .cloned()
                .map(field)
                .collect::<Vec<_>>()
                .join(",");
            out.push_str("\r\n");
            for row in &result.rows {
                out.push_str(
                    &row.iter()
                        .map(|value| field(text(value)))
                        .collect::<Vec<_>>()
                        .join(","),
                );
                out.push_str("\r\n");
            }
            out
        }
        Export::Json => {
            let rows: Vec<serde_json::Value> = result
                .rows
                .iter()
                .map(|row| {
                    let object: serde_json::Map<String, serde_json::Value> = result
                        .columns
                        .iter()
                        .zip(row)
                        .map(|(column, value)| {
                            let value = match value {
                                DatabaseValue::Null => serde_json::Value::Null,
                                DatabaseValue::Bool(value) => (*value).into(),
                                DatabaseValue::Integer(value) => (*value).into(),
                                DatabaseValue::Float(value) => serde_json::Number::from_f64(*value)
                                    .map_or(serde_json::Value::Null, serde_json::Value::Number),
                                other => text(other).into(),
                            };
                            (column.clone(), value)
                        })
                        .collect();
                    serde_json::Value::Object(object)
                })
                .collect();
            serde_json::to_string_pretty(&rows).unwrap_or_default()
        }
        Export::Markdown => {
            // A `|` in a value is escaped; a line end becomes `<br>` so the row stays one line.
            let cell = |value: String| {
                value
                    .replace('|', "\\|")
                    .replace("\r\n", "<br>")
                    .replace('\n', "<br>")
            };
            let mut out = format!(
                "| {} |\n",
                result
                    .columns
                    .iter()
                    .cloned()
                    .map(cell)
                    .collect::<Vec<_>>()
                    .join(" | ")
            );
            out.push_str(&format!("|{}\n", " --- |".repeat(result.columns.len())));
            for row in &result.rows {
                out.push_str(&format!(
                    "| {} |\n",
                    row.iter()
                        .map(|value| cell(text(value)))
                        .collect::<Vec<_>>()
                        .join(" | ")
                ));
            }
            out
        }
    }
}

/// A statement and its parameters, as it runs: values never go into the text (CLAUDE.md §7.2).
#[derive(Clone, Debug, PartialEq)]
pub struct Statement {
    pub sql: String,
    pub parameters: Vec<DatabaseValue>,
}

fn placeholder(driver: Driver, index: usize) -> String {
    match driver {
        Driver::PostgreSQL => format!("${index}"),
        _ => "?".to_owned(),
    }
}

/// The primary key of `table` with its values in `row` (the columns of `result`); none when the
/// table has no key or the result does not hold all of it.
fn key_of(
    catalog: &Catalog,
    table: &str,
    result: &QueryResult,
    row: usize,
) -> Result<Vec<(String, DatabaseValue)>> {
    let object = catalog.object(table).filter(|object| !object.view);
    let Some(object) = object else {
        bail!("Only rows of a table can be changed here");
    };
    let keys: Vec<&str> = object
        .columns
        .iter()
        .filter(|column| column.key)
        .map(|column| column.name.as_str())
        .collect();
    if keys.is_empty() {
        bail!("{table} has no primary key: change its rows in SQL");
    }
    keys.iter()
        .map(|key| {
            let index = result
                .columns
                .iter()
                .position(|column| column == key)
                .ok_or_else(|| anyhow::anyhow!("The results do not have the key column {key}"))?;
            Ok((
                (*key).to_owned(),
                result
                    .rows
                    .get(row)
                    .and_then(|row| row.get(index))
                    .cloned()
                    .unwrap_or(DatabaseValue::Null),
            ))
        })
        .collect()
}

/// `UPDATE table SET column = ? WHERE key = ?` for one cell of the results.
pub fn update_cell(
    driver: Driver,
    catalog: &Catalog,
    table: &str,
    result: &QueryResult,
    row: usize,
    column: &str,
    value: DatabaseValue,
) -> Result<Statement> {
    let keys = key_of(catalog, table, result, row)?;
    let mut parameters = vec![value];
    let conditions: Vec<String> = keys
        .into_iter()
        .map(|(name, value)| {
            parameters.push(value);
            format!(
                "{} = {}",
                identifier(&name, driver),
                placeholder(driver, parameters.len())
            )
        })
        .collect();
    Ok(Statement {
        sql: format!(
            "UPDATE {} SET {} = {} WHERE {}",
            identifier(table, driver),
            identifier(column, driver),
            placeholder(driver, 1),
            conditions.join(" AND ")
        ),
        parameters,
    })
}

/// `DELETE FROM table WHERE key = ?` for one row of the results.
pub fn delete_row(
    driver: Driver,
    catalog: &Catalog,
    table: &str,
    result: &QueryResult,
    row: usize,
) -> Result<Statement> {
    let keys = key_of(catalog, table, result, row)?;
    let mut parameters = Vec::new();
    let conditions: Vec<String> = keys
        .into_iter()
        .map(|(name, value)| {
            parameters.push(value);
            format!(
                "{} = {}",
                identifier(&name, driver),
                placeholder(driver, parameters.len())
            )
        })
        .collect();
    Ok(Statement {
        sql: format!(
            "DELETE FROM {} WHERE {}",
            identifier(table, driver),
            conditions.join(" AND ")
        ),
        parameters,
    })
}

/// `INSERT INTO table (columns) VALUES (?, …)`.
pub fn insert_row(
    driver: Driver,
    table: &str,
    values: &[(String, DatabaseValue)],
) -> Result<Statement> {
    if values.is_empty() {
        bail!("Give at least one column a value");
    }
    let columns: Vec<String> = values
        .iter()
        .map(|(name, _)| identifier(name, driver))
        .collect();
    let marks: Vec<String> = (1..=values.len())
        .map(|index| placeholder(driver, index))
        .collect();
    Ok(Statement {
        sql: format!(
            "INSERT INTO {} ({}) VALUES ({})",
            identifier(table, driver),
            columns.join(", "),
            marks.join(", ")
        ),
        parameters: values.iter().map(|(_, value)| value.clone()).collect(),
    })
}

/// Runs a statement with its parameters; the number of rows it changed.
pub async fn run_statement(pool: &Pool, statement: &Statement) -> Result<u64> {
    match pool {
        Pool::Sqlite(database) => {
            let database = database.clone();
            let statement = statement.clone();
            tokio::task::spawn_blocking(move || {
                let connection = database
                    .connection
                    .lock()
                    .map_err(|_| anyhow::anyhow!("The SQLite connection stopped; connect again"))?;
                let parameters: Vec<rusqlite::types::Value> =
                    statement.parameters.iter().map(sqlite_value).collect();
                connection
                    .execute(&statement.sql, rusqlite::params_from_iter(parameters))
                    .map(|count| count as u64)
                    .map_err(|error| anyhow::anyhow!("{error}"))
            })
            .await
            .map_err(|_| anyhow::anyhow!("SQLite task failed"))?
        }
        Pool::Postgres(client) => {
            // The server says the type of each parameter; the value goes as that type.
            let prepared = client
                .prepare(&statement.sql)
                .await
                .map_err(|error| anyhow::anyhow!("{error}"))?;
            let values: Vec<Box<dyn tokio_postgres::types::ToSql + Sync + Send>> = prepared
                .params()
                .iter()
                .zip(&statement.parameters)
                .map(|(kind, value)| postgres_value(kind, value))
                .collect::<Result<_>>()?;
            let references: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = values
                .iter()
                .map(|value| value.as_ref() as &(dyn tokio_postgres::types::ToSql + Sync))
                .collect();
            client
                .execute(&prepared, &references)
                .await
                .map_err(|error| anyhow::anyhow!("{error}"))
        }
        Pool::Mysql(connection) => {
            use mysql_async::prelude::Queryable;
            let mut connection = connection.lock().await;
            let connection = connection
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("The connection is closed"))?;
            let parameters: Vec<mysql_async::Value> =
                statement.parameters.iter().map(mysql_value).collect();
            connection
                .exec_drop(&statement.sql, parameters)
                .await
                .map_err(|error| anyhow::anyhow!("{error}"))?;
            Ok(connection.affected_rows())
        }
    }
}

fn sqlite_value(value: &DatabaseValue) -> rusqlite::types::Value {
    use rusqlite::types::Value;
    match value {
        DatabaseValue::Null => Value::Null,
        DatabaseValue::Bool(value) => Value::Integer(i64::from(*value)),
        DatabaseValue::Integer(value) => Value::Integer(*value),
        DatabaseValue::Float(value) => Value::Real(*value),
        DatabaseValue::Text(value) => Value::Text(value.clone()),
        DatabaseValue::Bytes(value) => Value::Blob(value.clone()),
    }
}

fn mysql_value(value: &DatabaseValue) -> mysql_async::Value {
    use mysql_async::Value;
    match value {
        DatabaseValue::Null => Value::NULL,
        DatabaseValue::Bool(value) => Value::Int(i64::from(*value)),
        DatabaseValue::Integer(value) => Value::Int(*value),
        DatabaseValue::Float(value) => Value::Double(*value),
        DatabaseValue::Text(value) => Value::Bytes(value.clone().into_bytes()),
        DatabaseValue::Bytes(value) => Value::Bytes(value.clone()),
    }
}

/// A value as the PostgreSQL type of its parameter; a type SozoIDE does not convert says so.
fn postgres_value(
    kind: &tokio_postgres::types::Type,
    value: &DatabaseValue,
) -> Result<Box<dyn tokio_postgres::types::ToSql + Sync + Send>> {
    use tokio_postgres::types::Type;
    let as_text = || match value {
        DatabaseValue::Text(text) => text.clone(),
        other => other.to_string(),
    };
    if *value == DatabaseValue::Null {
        return Ok(Box::new(Option::<String>::None));
    }
    Ok(match *kind {
        Type::BOOL => Box::new(
            matches!(value, DatabaseValue::Bool(true)) || as_text() == "true" || as_text() == "t",
        ),
        Type::INT2 => Box::new(as_text().parse::<i16>()?),
        Type::INT4 => Box::new(as_text().parse::<i32>()?),
        Type::INT8 => Box::new(as_text().parse::<i64>()?),
        Type::FLOAT4 => Box::new(as_text().parse::<f32>()?),
        Type::FLOAT8 => Box::new(as_text().parse::<f64>()?),
        Type::BYTEA => Box::new(match value {
            DatabaseValue::Bytes(bytes) => bytes.clone(),
            _ => as_text().into_bytes(),
        }),
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME => Box::new(as_text()),
        _ => bail!("A column of type {kind} is changed in SQL"),
    })
}

/// Whether a query may hold a secret (a password, a key): it never goes into the history.
pub fn secret(query: &str) -> bool {
    let lower = query.to_lowercase();
    [
        "password",
        "identified by",
        "pwd",
        "secret",
        "token",
        "api_key",
        "apikey",
    ]
    .iter()
    .any(|word| lower.contains(word))
}

/// The plan of `query`: `EXPLAIN QUERY PLAN` on SQLite, `EXPLAIN` elsewhere.
pub fn explain(driver: Driver, query: &str) -> String {
    let query = query.trim().trim_end_matches(';');
    match driver {
        Driver::SQLite => format!("EXPLAIN QUERY PLAN {query}"),
        _ => format!("EXPLAIN {query}"),
    }
}

/// How many rows `query` gives, without reading them.
pub fn count_query(query: &str) -> String {
    format!(
        "SELECT count(*) FROM ({}) AS counted",
        query.trim().trim_end_matches(';')
    )
}

/// The names of tables and columns that start with the word before the cursor (TODO 4.1.13);
/// after `table.` only that table's columns.
pub fn complete(catalog: &Catalog, before: &str) -> Vec<String> {
    let word: String = before
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if word.is_empty() {
        return Vec::new();
    }
    let lower = word.to_lowercase();
    let mut found: Vec<String> = match lower.split_once('.') {
        Some((table, column)) => catalog
            .objects
            .iter()
            .filter(|object| object.name.to_lowercase() == table)
            .flat_map(|object| &object.columns)
            .filter(|candidate| candidate.name.to_lowercase().starts_with(column))
            .map(|candidate| candidate.name.clone())
            .collect(),
        None => catalog
            .objects
            .iter()
            .map(|object| object.name.clone())
            .chain(
                catalog
                    .objects
                    .iter()
                    .flat_map(|object| object.columns.iter().map(|column| column.name.clone())),
            )
            .filter(|name| name.to_lowercase().starts_with(&lower) && name.to_lowercase() != lower)
            .collect(),
    };
    found.sort();
    found.dedup();
    found.truncate(50);
    found
}

/// The table a query reads when it reads only one (`SELECT … FROM t [WHERE …]`), as the
/// catalog names it; none with a join, several tables or a subquery.
pub fn table_of(query: &str, catalog: &Catalog) -> Option<String> {
    let lower = query.to_lowercase();
    if !lower.trim_start().starts_with("select")
        || lower.contains(" join ")
        || lower.matches("select").count() > 1
    {
        return None;
    }
    let at = lower.find(" from ")? + " from ".len();
    let rest = query[at..].trim_start();
    let name: String = match rest.chars().next()? {
        quote @ ('"' | '`' | '[') => {
            let close = if quote == '[' { ']' } else { quote };
            rest[1..].split(close).next()?.to_owned()
        }
        _ => rest
            .split(|c: char| c.is_whitespace() || c == ';' || c == ',')
            .next()?
            .to_owned(),
    };
    let after = rest[rest.find(&name)? + name.len()..]
        .trim_start()
        .trim_start_matches(['"', '`', ']'])
        .trim_start();
    if after.starts_with(',') {
        return None;
    }
    catalog
        .objects
        .iter()
        .find(|object| {
            !object.view && (object.name == name || object.name.eq_ignore_ascii_case(&name))
        })
        .map(|object| object.name.clone())
}

/// Whether a host is this machine: a connection without TLS to any other is said.
pub fn local(host: &str) -> bool {
    let host = host
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_lowercase();
    host.is_empty()
        || host == "localhost"
        || host == "::1"
        || host.starts_with("127.")
        || host.ends_with(".localhost")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::builder::{DbColumn, DbObject};

    fn result() -> QueryResult {
        QueryResult {
            columns: vec!["id".into(), "name".into()],
            rows: vec![
                vec![
                    DatabaseValue::Integer(1),
                    DatabaseValue::Text("a,\"b\"\nc".into()),
                ],
                vec![DatabaseValue::Integer(2), DatabaseValue::Null],
            ],
            affected_rows: 0,
            duration: std::time::Duration::ZERO,
            more: false,
        }
    }

    fn catalog() -> Catalog {
        Catalog {
            objects: vec![DbObject {
                name: "my \"table\"".into(),
                view: false,
                columns: vec![
                    DbColumn {
                        name: "id".into(),
                        kind: "INTEGER".into(),
                        key: true,
                    },
                    DbColumn {
                        name: "name".into(),
                        kind: "TEXT".into(),
                        key: false,
                    },
                ],
            }],
            keys: Vec::new(),
        }
    }

    #[test]
    fn exports_quote_what_each_format_needs() {
        let csv = export(&result(), Export::Csv);
        assert_eq!(csv, "id,name\r\n1,\"a,\"\"b\"\"\nc\"\r\n2,\r\n");
        let json: serde_json::Value =
            serde_json::from_str(&export(&result(), Export::Json)).unwrap();
        assert_eq!(json[0]["name"], "a,\"b\"\nc");
        assert_eq!(json[1]["name"], serde_json::Value::Null);
        let markdown = export(&result(), Export::Markdown);
        assert!(markdown.contains("| 1 | a,\"b\"<br>c |"), "{markdown}");
        let pipe = QueryResult {
            rows: vec![vec![
                DatabaseValue::Integer(3),
                DatabaseValue::Text("x|y".into()),
            ]],
            ..result()
        };
        assert!(export(&pipe, Export::Markdown).contains("x\\|y"));
    }

    #[test]
    fn edits_are_statements_with_parameters_and_quoted_names() {
        let hostile = DatabaseValue::Text("x'; DROP TABLE t; --\0é".into());
        let update = update_cell(
            Driver::PostgreSQL,
            &catalog(),
            "my \"table\"",
            &result(),
            0,
            "name",
            hostile.clone(),
        )
        .unwrap();
        assert_eq!(
            update.sql,
            "UPDATE \"my \"\"table\"\"\" SET name = $1 WHERE id = $2"
        );
        assert_eq!(
            update.parameters,
            [hostile.clone(), DatabaseValue::Integer(1)]
        );
        assert!(!update.sql.contains("DROP"));
        let delete = delete_row(Driver::MySQL, &catalog(), "my \"table\"", &result(), 1).unwrap();
        assert_eq!(delete.sql, "DELETE FROM `my \"table\"` WHERE id = ?");
        let insert = insert_row(Driver::SQLite, "t", &[("name".into(), hostile)]).unwrap();
        assert_eq!(insert.sql, "INSERT INTO t (name) VALUES (?)");
        // No key, a view, or a result without the key column: refused.
        let mut keyless = catalog();
        keyless.objects[0].columns[0].key = false;
        assert!(
            update_cell(
                Driver::SQLite,
                &keyless,
                "my \"table\"",
                &result(),
                0,
                "name",
                DatabaseValue::Null
            )
            .is_err()
        );
        let without = QueryResult {
            columns: vec!["name".into()],
            ..result()
        };
        assert!(delete_row(Driver::SQLite, &catalog(), "my \"table\"", &without, 0).is_err());
        assert!(insert_row(Driver::SQLite, "t", &[]).is_err());
    }

    #[tokio::test]
    async fn a_statement_runs_in_sqlite_with_its_parameters() {
        let mut form = crate::database::model::ConnectionForm::new(Driver::SQLite);
        form.name = "x".into();
        form.database = ":memory:".into();
        let pool = crate::database::store::open_pool(&form).await.unwrap();
        crate::database::store::run_query(
            &pool,
            "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)",
        )
        .await
        .unwrap();
        let hostile = "x'); DROP TABLE t; --";
        let insert = insert_row(
            Driver::SQLite,
            "t",
            &[("name".into(), DatabaseValue::Text(hostile.into()))],
        )
        .unwrap();
        assert_eq!(run_statement(&pool, &insert).await.unwrap(), 1);
        let back = crate::database::store::run_query(&pool, "SELECT name FROM t")
            .await
            .unwrap();
        assert_eq!(back.rows[0][0], DatabaseValue::Text(hostile.into()));
    }

    #[test]
    fn secrets_explain_counts_completion_and_local_hosts() {
        assert!(secret("ALTER USER bob WITH PASSWORD 'x'"));
        assert!(secret("create user a identified by 'b'"));
        assert!(!secret("SELECT * FROM orders"));
        assert_eq!(
            explain(Driver::SQLite, "SELECT 1;"),
            "EXPLAIN QUERY PLAN SELECT 1"
        );
        assert_eq!(explain(Driver::MySQL, "SELECT 1"), "EXPLAIN SELECT 1");
        assert_eq!(
            count_query("SELECT * FROM t;"),
            "SELECT count(*) FROM (SELECT * FROM t) AS counted"
        );
        let catalog = catalog();
        assert_eq!(complete(&catalog, "SELECT na"), ["name"]);
        assert_eq!(complete(&catalog, "SELECT \"x\".i"), Vec::<String>::new());
        assert_eq!(complete(&catalog, "SELECT my"), ["my \"table\""]);
        assert!(complete(&catalog, "SELECT ").is_empty());
        for host in ["localhost", "127.0.0.1", "[::1]", "", "db.localhost"] {
            assert!(local(host), "{host}");
        }
        for host in ["db.example.com", "10.0.0.5", "128.0.0.1"] {
            assert!(!local(host), "{host}");
        }
        assert_eq!(
            table_of("SELECT * FROM \"my \"table\"\" WHERE id > 1", &catalog),
            None,
            "an escaped quote is not read"
        );
        let mut plain = catalog.clone();
        plain.objects[0].name = "orders".into();
        assert_eq!(
            table_of("select id from ORDERS where id = 1;", &plain).as_deref(),
            Some("orders")
        );
        assert_eq!(
            table_of("SELECT * FROM `orders`", &plain).as_deref(),
            Some("orders")
        );
        for query in [
            "SELECT * FROM orders JOIN x ON 1",
            "SELECT * FROM orders, x",
            "SELECT * FROM (SELECT 1) AS orders",
            "DELETE FROM orders",
            "SELECT * FROM other",
        ] {
            assert_eq!(table_of(query, &plain), None, "{query}");
        }
    }
}
