//! PostgreSQL and MySQL proved for real (TODO 4.4.1): the catalog, the Builder, the types, edits
//! with parameters and TLS, against the servers of the CI job `Databases` (containers of the
//! GitHub runner). Only with `--features databases`; the URLs (without password) come from
//! `SOZO_TEST_POSTGRES` and `SOZO_TEST_MYSQL` and the password of those throwaway containers from
//! `SOZO_TEST_PASSWORD`; a missing one fails: a test that skips would pass without proving.

use super::{
    builder::{Design, build_query, load_catalog},
    model::{ConnectionForm, DatabaseValue, Driver, Tls},
    more::{insert_row, run_statement, update_cell},
    store::{open_pool, run_page, run_query},
};

fn form(variable: &str, driver: Driver) -> ConnectionForm {
    let url = std::env::var(variable)
        .unwrap_or_else(|_| panic!("{variable} is not set: see TESTING.md §4"));
    let mut form = ConnectionForm::new(driver);
    form.name = "real".into();
    form.url = url;
    form.password = std::env::var("SOZO_TEST_PASSWORD").expect("SOZO_TEST_PASSWORD is not set");
    form
}

async fn prove(driver: Driver, variable: &str) {
    let pool = open_pool(&form(variable, driver)).await.unwrap();
    for statement in [
        "DROP TABLE IF EXISTS sozo_orders",
        "DROP TABLE IF EXISTS sozo_people",
        "CREATE TABLE sozo_people (id INTEGER PRIMARY KEY, name VARCHAR(200), score DOUBLE PRECISION, active BOOLEAN)",
        "CREATE TABLE sozo_orders (id INTEGER PRIMARY KEY, person_id INTEGER, total INTEGER, \
         FOREIGN KEY (person_id) REFERENCES sozo_people(id))",
        "INSERT INTO sozo_people VALUES (1, 'Ana', 1.5, TRUE), (2, NULL, NULL, FALSE)",
        "INSERT INTO sozo_orders VALUES (1, 1, 5), (2, 1, 20)",
    ] {
        run_query(&pool, statement)
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }
    // Types come back as themselves; NULL stays NULL.
    let rows = run_query(
        &pool,
        "SELECT id, name, score, active FROM sozo_people ORDER BY id",
    )
    .await
    .unwrap()
    .rows;
    assert_eq!(rows[0][0], DatabaseValue::Integer(1));
    assert_eq!(rows[0][1], DatabaseValue::Text("Ana".into()));
    assert_eq!(rows[0][2], DatabaseValue::Float(1.5));
    assert_eq!(rows[1][1], DatabaseValue::Null);
    // The catalog sees both tables, their key and the foreign key; the Builder joins them.
    let catalog = load_catalog(&pool, driver).await.unwrap();
    let people = catalog.object("sozo_people").unwrap();
    assert!(
        people
            .columns
            .iter()
            .any(|column| column.name == "id" && column.key)
    );
    assert!(
        catalog
            .keys_between("sozo_orders", "sozo_people")
            .next()
            .is_some()
    );
    let mut design = Design::default();
    design.toggle_object("sozo_people").unwrap();
    design.toggle_object("sozo_orders").unwrap();
    design.toggle_field("sozo_people", "name");
    design.toggle_field("sozo_orders", "total");
    let sql = build_query(&catalog, &design, driver).unwrap();
    let joined = run_query(&pool, &sql).await.unwrap();
    assert_eq!(joined.rows.len(), 2, "{sql}");
    // Edits with parameters: hostile text goes in as a value.
    let hostile = "x'); DROP TABLE sozo_people; -- é \"q\"";
    let people_rows = run_query(&pool, "SELECT id, name FROM sozo_people ORDER BY id")
        .await
        .unwrap();
    let update = update_cell(
        driver,
        &catalog,
        "sozo_people",
        &people_rows,
        1,
        "name",
        DatabaseValue::Text(hostile.into()),
    )
    .unwrap();
    assert_eq!(run_statement(&pool, &update).await.unwrap(), 1);
    let insert = insert_row(
        driver,
        "sozo_people",
        &[
            ("id".into(), DatabaseValue::Text("3".into())),
            ("name".into(), DatabaseValue::Null),
        ],
    )
    .unwrap();
    assert_eq!(run_statement(&pool, &insert).await.unwrap(), 1);
    let back = run_query(&pool, "SELECT name FROM sozo_people WHERE id = 2")
        .await
        .unwrap();
    assert_eq!(back.rows[0][0], DatabaseValue::Text(hostile.into()));
    // Pages: the first rows and the word that there are more.
    let page = run_page(&pool, "SELECT * FROM sozo_people ORDER BY id", 2)
        .await
        .unwrap();
    assert_eq!((page.rows.len(), page.more), (2, true));
    run_query(&pool, "DROP TABLE sozo_orders").await.unwrap();
    run_query(&pool, "DROP TABLE sozo_people").await.unwrap();
    pool.close().await;

    // The service has no certificate: TLS required and checked fails and says so, never
    // falling back to plain text.
    let mut verified = form(variable, driver);
    verified.url = verified.url.split('?').next().unwrap().to_owned();
    verified.tls = Tls::Verify;
    let error = match open_pool(&verified).await {
        Ok(_) => panic!("{driver}: a server without TLS was accepted with TLS verified"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("connection failed"), "{error}");
    let password = std::env::var("SOZO_TEST_PASSWORD").unwrap();
    assert!(
        !error.contains(&password),
        "the password never shows: {error}"
    );
}

#[tokio::test]
async fn postgres_for_real() {
    prove(Driver::PostgreSQL, "SOZO_TEST_POSTGRES").await;
}

#[tokio::test]
async fn mysql_for_real() {
    prove(Driver::MySQL, "SOZO_TEST_MYSQL").await;
}
