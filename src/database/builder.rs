//! The query builder (F07, Q37): the tables and views of a connection with their columns and
//! keys, the ones chosen in the diagram, and the `SELECT` they make — joined by their foreign
//! keys, with totals, order and filters as in a desktop query designer.

use std::collections::HashSet;

use anyhow::{Result, bail};
use sqlx::AnyPool;

use super::{
    model::{DatabaseValue, Driver},
    store::run_query,
};

/// Tables and views in one query at most: more is rarely meant and slow to draw.
pub const MAX_OBJECTS: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DbColumn {
    pub name: String,
    pub kind: String,
    pub key: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DbObject {
    pub name: String,
    pub view: bool,
    pub columns: Vec<DbColumn>,
}

/// `from.from_column` points at `to.to_column`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForeignKey {
    pub from: String,
    pub from_column: String,
    pub to: String,
    pub to_column: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalog {
    pub objects: Vec<DbObject>,
    pub keys: Vec<ForeignKey>,
}

impl Catalog {
    pub fn object(&self, name: &str) -> Option<&DbObject> {
        self.objects.iter().find(|object| object.name == name)
    }

    /// The foreign keys between `a` and `b`, either way.
    pub fn keys_between<'a>(
        &'a self,
        a: &'a str,
        b: &'a str,
    ) -> impl Iterator<Item = &'a ForeignKey> + 'a {
        self.keys
            .iter()
            .filter(move |key| (key.from == a && key.to == b) || (key.from == b && key.to == a))
    }
}

/// Reads the tables and views of the database the pool is on, their columns in order, the
/// primary key columns and the foreign keys.
pub async fn load_catalog(pool: &AnyPool, driver: Driver) -> Result<Catalog> {
    let mut catalog = Catalog::default();
    match driver {
        Driver::SQLite => {
            let objects = rows(
                pool,
                "SELECT name, type FROM sqlite_master WHERE type IN ('table', 'view') \
                 AND name NOT LIKE 'sqlite_%' ORDER BY type, name",
            )
            .await?;
            for object in objects {
                let name = &object[0];
                let quoted = name.replace('\'', "''");
                let columns = rows(
                    pool,
                    &format!("SELECT name, type, pk FROM pragma_table_info('{quoted}')"),
                )
                .await?;
                catalog.objects.push(DbObject {
                    name: name.clone(),
                    view: object[1] == "view",
                    columns: columns
                        .into_iter()
                        .map(|column| DbColumn {
                            name: column[0].clone(),
                            kind: column[1].clone(),
                            key: column[2] != "0",
                        })
                        .collect(),
                });
                for key in rows(
                    pool,
                    &format!(
                        "SELECT \"table\", \"from\", \"to\" FROM pragma_foreign_key_list('{quoted}')"
                    ),
                )
                .await?
                {
                    catalog.keys.push(ForeignKey {
                        from: name.clone(),
                        from_column: key[1].clone(),
                        to: key[0].clone(),
                        to_column: key[2].clone(),
                    });
                }
            }
            // A key to the referenced table's primary key leaves `to` empty.
            for index in 0..catalog.keys.len() {
                if catalog.keys[index].to_column.is_empty()
                    || catalog.keys[index].to_column == "NULL"
                {
                    let primary = catalog
                        .object(&catalog.keys[index].to)
                        .and_then(|object| object.columns.iter().find(|column| column.key))
                        .map(|column| column.name.clone())
                        .unwrap_or_default();
                    catalog.keys[index].to_column = primary;
                }
            }
        }
        Driver::PostgreSQL | Driver::MySQL => {
            let schema = if driver == Driver::PostgreSQL {
                "current_schema()"
            } else {
                "DATABASE()"
            };
            let objects = rows(
                pool,
                &format!(
                    "SELECT table_name, table_type FROM information_schema.tables \
                     WHERE table_schema = {schema} ORDER BY table_type, table_name"
                ),
            )
            .await?;
            let columns = rows(
                pool,
                &format!(
                    "SELECT table_name, column_name, data_type FROM information_schema.columns \
                     WHERE table_schema = {schema} ORDER BY table_name, ordinal_position"
                ),
            )
            .await?;
            let primary = rows(
                pool,
                &format!(
                    "SELECT k.table_name, k.column_name FROM information_schema.table_constraints c \
                     JOIN information_schema.key_column_usage k ON k.constraint_name = c.constraint_name \
                     AND k.table_schema = c.table_schema AND k.table_name = c.table_name \
                     WHERE c.constraint_type = 'PRIMARY KEY' AND c.table_schema = {schema}"
                ),
            )
            .await?;
            let foreign = if driver == Driver::PostgreSQL {
                "SELECT k.table_name, k.column_name, u.table_name, u.column_name \
                 FROM information_schema.table_constraints c \
                 JOIN information_schema.key_column_usage k ON k.constraint_name = c.constraint_name \
                 AND k.table_schema = c.table_schema \
                 JOIN information_schema.constraint_column_usage u ON u.constraint_name = c.constraint_name \
                 AND u.table_schema = c.table_schema \
                 WHERE c.constraint_type = 'FOREIGN KEY' AND c.table_schema = current_schema()"
            } else {
                "SELECT table_name, column_name, referenced_table_name, referenced_column_name \
                 FROM information_schema.key_column_usage \
                 WHERE table_schema = DATABASE() AND referenced_table_name IS NOT NULL"
            };
            for object in objects {
                let name = object[0].clone();
                catalog.objects.push(DbObject {
                    view: object[1].to_ascii_uppercase().contains("VIEW"),
                    columns: columns
                        .iter()
                        .filter(|column| column[0] == name)
                        .map(|column| DbColumn {
                            name: column[1].clone(),
                            kind: column[2].clone(),
                            key: primary
                                .iter()
                                .any(|key| key[0] == name && key[1] == column[1]),
                        })
                        .collect(),
                    name,
                });
            }
            for key in rows(pool, foreign).await? {
                catalog.keys.push(ForeignKey {
                    from: key[0].clone(),
                    from_column: key[1].clone(),
                    to: key[2].clone(),
                    to_column: key[3].clone(),
                });
            }
        }
    }
    Ok(catalog)
}

/// Each row of `query` as text; names that a driver returns as bytes are read as UTF-8.
async fn rows(pool: &AnyPool, query: &str) -> Result<Vec<Vec<String>>> {
    let result = run_query(pool, query).await?;
    Ok(result
        .rows
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|value| match value {
                    DatabaseValue::Bytes(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                    DatabaseValue::Null => String::new(),
                    other => other.to_string(),
                })
                .collect()
        })
        .collect())
}

/// What a field adds up to: nothing, a group, or an aggregate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Total {
    #[default]
    None,
    Group,
    Count,
    Sum,
    Avg,
    Min,
    Max,
}

impl Total {
    pub const ALL: [Self; 7] = [
        Self::None,
        Self::Group,
        Self::Count,
        Self::Sum,
        Self::Avg,
        Self::Min,
        Self::Max,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "-",
            Self::Group => "Group by",
            Self::Count => "Count",
            Self::Sum => "Sum",
            Self::Avg => "Average",
            Self::Min => "Min",
            Self::Max => "Max",
        }
    }

    fn function(self) -> Option<&'static str> {
        match self {
            Self::Count => Some("COUNT"),
            Self::Sum => Some("SUM"),
            Self::Avg => Some("AVG"),
            Self::Min => Some("MIN"),
            Self::Max => Some("MAX"),
            Self::None | Self::Group => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    #[default]
    None,
    Ascending,
    Descending,
}

impl Sort {
    pub const ALL: [Self; 3] = [Self::None, Self::Ascending, Self::Descending];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "-",
            Self::Ascending => "Ascending",
            Self::Descending => "Descending",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Operator {
    #[default]
    None,
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
    Contains,
    StartsWith,
    Empty,
    NotEmpty,
}

impl Operator {
    pub const ALL: [Self; 11] = [
        Self::None,
        Self::Equal,
        Self::NotEqual,
        Self::Less,
        Self::LessOrEqual,
        Self::Greater,
        Self::GreaterOrEqual,
        Self::Contains,
        Self::StartsWith,
        Self::Empty,
        Self::NotEmpty,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "-",
            Self::Equal => "=",
            Self::NotEqual => "<>",
            Self::Less => "<",
            Self::LessOrEqual => "<=",
            Self::Greater => ">",
            Self::GreaterOrEqual => ">=",
            Self::Contains => "contains",
            Self::StartsWith => "starts with",
            Self::Empty => "is empty",
            Self::NotEmpty => "is not empty",
        }
    }

    /// Whether the filter needs a value.
    pub fn takes_value(self) -> bool {
        !matches!(self, Self::None | Self::Empty | Self::NotEmpty)
    }
}

/// The next of `all` after `value`, around.
pub fn cycle<T: Copy + PartialEq>(all: &[T], value: T, forward: bool) -> T {
    let index = all.iter().position(|item| *item == value).unwrap_or(0);
    let next = if forward {
        (index + 1) % all.len()
    } else {
        (index + all.len() - 1) % all.len()
    };
    all[next]
}

/// One column chosen in the diagram, as a row of the builder grid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub object: String,
    pub column: String,
    pub show: bool,
    pub total: Total,
    pub sort: Sort,
    pub operator: Operator,
    pub value: String,
}

impl Field {
    pub fn new(object: &str, column: &str) -> Self {
        Self {
            object: object.to_owned(),
            column: column.to_owned(),
            show: true,
            total: Total::None,
            sort: Sort::None,
            operator: Operator::None,
            value: String::new(),
        }
    }
}

/// The tables and views in the diagram, in the order they came in, and the fields chosen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Design {
    pub objects: Vec<String>,
    pub fields: Vec<Field>,
}

impl Design {
    /// Puts `object` in the diagram or takes it out with its fields.
    pub fn toggle_object(&mut self, object: &str) -> Result<()> {
        if let Some(index) = self.objects.iter().position(|name| name == object) {
            self.objects.remove(index);
            self.fields.retain(|field| field.object != object);
        } else {
            if self.objects.len() >= MAX_OBJECTS {
                bail!("At most {MAX_OBJECTS} tables and views in one query");
            }
            self.objects.push(object.to_owned());
        }
        Ok(())
    }

    pub fn field(&self, object: &str, column: &str) -> Option<usize> {
        self.fields
            .iter()
            .position(|field| field.object == object && field.column == column)
    }

    /// Adds the column to the builder grid or takes it away.
    pub fn toggle_field(&mut self, object: &str, column: &str) {
        match self.field(object, column) {
            Some(index) => {
                self.fields.remove(index);
            }
            None => self.fields.push(Field::new(object, column)),
        }
    }
}

/// `name` as an identifier of `driver`, quoted only when it has to be.
fn identifier(name: &str, driver: Driver) -> String {
    let plain = name
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_');
    if plain {
        name.to_owned()
    } else if driver == Driver::MySQL {
        format!("`{}`", name.replace('`', "``"))
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

fn numeric(kind: &str) -> bool {
    let kind = kind.to_ascii_lowercase();
    [
        "int", "real", "double", "float", "numeric", "decimal", "number", "serial", "money",
    ]
    .iter()
    .any(|word| kind.contains(word))
}

/// `value` as a literal: a number in a numeric column, text in quotes otherwise.
fn literal(value: &str, kind: &str) -> String {
    let value = value.trim();
    if numeric(kind) && value.parse::<f64>().is_ok() {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "''"))
    }
}

/// The `SELECT` of `design`: the first table, then each one joined by a foreign key to those
/// already in; group by the shown columns without a total when any field has one; `WHERE`
/// for filters on plain columns, `HAVING` on totals.
pub fn build_query(catalog: &Catalog, design: &Design, driver: Driver) -> Result<String> {
    let Some(first) = design.objects.first() else {
        bail!("Choose a table or a view on the left");
    };
    let name = |object: &str| identifier(object, driver);
    let column = |field: &Field| {
        format!(
            "{}.{}",
            name(&field.object),
            identifier(&field.column, driver)
        )
    };
    for object in &design.objects {
        if catalog.object(object).is_none() {
            bail!("{object} is not in this database");
        }
    }
    let mut from = name(first);
    let mut inside = HashSet::from([first.as_str()]);
    while inside.len() < design.objects.len() {
        let next = design.objects.iter().find(|object| {
            !inside.contains(object.as_str())
                && inside
                    .iter()
                    .any(|other| catalog.keys_between(object, other).next().is_some())
        });
        let Some(next) = next else {
            let alone = design
                .objects
                .iter()
                .find(|object| !inside.contains(object.as_str()))
                .map_or("", String::as_str);
            bail!("{alone} has no foreign key to the other tables: take it out of the diagram");
        };
        let conditions = inside
            .iter()
            .flat_map(|other| catalog.keys_between(next, other))
            .map(|key| {
                format!(
                    "{}.{} = {}.{}",
                    name(&key.from),
                    identifier(&key.from_column, driver),
                    name(&key.to),
                    identifier(&key.to_column, driver)
                )
            })
            .collect::<Vec<_>>();
        from.push_str(&format!(
            "\n  JOIN {} ON {}",
            name(next),
            conditions.join("\n    AND ")
        ));
        inside.insert(next);
    }

    let grouped = design.fields.iter().any(|field| field.total != Total::None);
    let (mut select, mut group, mut order) = (Vec::new(), Vec::new(), Vec::new());
    let (mut filters, mut having) = (Vec::new(), Vec::new());
    for field in &design.fields {
        let Some(kind) = catalog
            .object(&field.object)
            .and_then(|object| {
                object
                    .columns
                    .iter()
                    .find(|column| column.name == field.column)
            })
            .map(|column| column.kind.as_str())
        else {
            bail!("{} has no column {}", field.object, field.column);
        };
        if matches!(field.total, Total::Sum | Total::Avg) && !numeric(kind) && !kind.is_empty() {
            bail!(
                "{} needs a column of numbers: {} is {kind}",
                field.total.label(),
                field.column
            );
        }
        let plain = column(field);
        let expression = match field.total.function() {
            Some(function) => format!("{function}({plain})"),
            None => plain.clone(),
        };
        if field.show {
            select.push(match field.total.function() {
                Some(function) => format!(
                    "{expression} AS {}",
                    identifier(
                        &format!("{}_{}", function.to_ascii_lowercase(), field.column),
                        driver
                    )
                ),
                None => expression.clone(),
            });
        }
        if grouped && field.total.function().is_none() && (field.show || field.sort != Sort::None) {
            group.push(plain.clone());
        }
        match field.sort {
            Sort::Ascending => order.push(expression.clone()),
            Sort::Descending => order.push(format!("{expression} DESC")),
            Sort::None => {}
        }
        if field.operator == Operator::None {
            continue;
        }
        if field.operator.takes_value() && field.value.trim().is_empty() {
            bail!("The filter of {} needs a value", field.column);
        }
        let condition = match field.operator {
            Operator::Empty => format!("{expression} IS NULL"),
            Operator::NotEmpty => format!("{expression} IS NOT NULL"),
            Operator::Contains => format!(
                "{expression} LIKE {}",
                literal(&format!("%{}%", field.value.trim()), "")
            ),
            Operator::StartsWith => format!(
                "{expression} LIKE {}",
                literal(&format!("{}%", field.value.trim()), "")
            ),
            operator => format!(
                "{expression} {} {}",
                operator.label(),
                literal(&field.value, kind)
            ),
        };
        if field.total.function().is_some() {
            having.push(condition);
        } else {
            filters.push(condition);
        }
    }
    if select.is_empty() {
        bail!("Tick at least one column to show in the diagram or the builder");
    }
    let mut sql = format!("SELECT {}\nFROM {from}", select.join(",\n       "));
    if !filters.is_empty() {
        sql.push_str(&format!("\nWHERE {}", filters.join("\n  AND ")));
    }
    if !group.is_empty() {
        sql.push_str(&format!("\nGROUP BY {}", group.join(", ")));
    }
    if !having.is_empty() {
        sql.push_str(&format!("\nHAVING {}", having.join("\n   AND ")));
    }
    if !order.is_empty() {
        sql.push_str(&format!("\nORDER BY {}", order.join(", ")));
    }
    Ok(sql)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Catalog {
        let column = |name: &str, kind: &str, key| DbColumn {
            name: name.to_owned(),
            kind: kind.to_owned(),
            key,
        };
        Catalog {
            objects: vec![
                DbObject {
                    name: "customers".into(),
                    view: false,
                    columns: vec![column("id", "INTEGER", true), column("name", "TEXT", false)],
                },
                DbObject {
                    name: "orders".into(),
                    view: false,
                    columns: vec![
                        column("id", "INTEGER", true),
                        column("customer_id", "INTEGER", false),
                        column("total", "REAL", false),
                    ],
                },
                DbObject {
                    name: "order items".into(),
                    view: false,
                    columns: vec![column("order_id", "INTEGER", false)],
                },
                DbObject {
                    name: "notes".into(),
                    view: true,
                    columns: vec![column("text", "TEXT", false)],
                },
            ],
            keys: vec![
                ForeignKey {
                    from: "orders".into(),
                    from_column: "customer_id".into(),
                    to: "customers".into(),
                    to_column: "id".into(),
                },
                ForeignKey {
                    from: "order items".into(),
                    from_column: "order_id".into(),
                    to: "orders".into(),
                    to_column: "id".into(),
                },
            ],
        }
    }

    #[test]
    fn the_builder_joins_by_foreign_keys_groups_sorts_and_filters() {
        let catalog = catalog();
        let mut design = Design::default();
        design.toggle_object("customers").unwrap();
        design.toggle_object("orders").unwrap();
        design.toggle_field("customers", "name");
        design.toggle_field("orders", "total");
        design.fields[1].total = Total::Sum;
        design.fields[1].sort = Sort::Descending;
        design.fields[1].operator = Operator::Greater;
        design.fields[1].value = "100".into();
        design.toggle_field("customers", "name");
        design.toggle_field("customers", "name");
        design.fields[1].operator = Operator::StartsWith;
        design.fields[1].value = "O'B".into();
        assert_eq!(
            build_query(&catalog, &design, Driver::SQLite).unwrap(),
            "SELECT SUM(orders.total) AS sum_total,\n       customers.name\n\
             FROM customers\n  JOIN orders ON orders.customer_id = customers.id\n\
             WHERE customers.name LIKE 'O''B%'\n\
             GROUP BY customers.name\n\
             HAVING SUM(orders.total) > 100\n\
             ORDER BY SUM(orders.total) DESC"
        );

        // Names that need quotes get the driver's; a table with no key to the others is refused.
        design.toggle_object("order items").unwrap();
        design.toggle_field("order items", "order_id");
        let sql = build_query(&catalog, &design, Driver::MySQL).unwrap();
        assert!(
            sql.contains("\n  JOIN `order items` ON `order items`.order_id = orders.id"),
            "{sql}"
        );
        assert!(
            build_query(&catalog, &design, Driver::PostgreSQL)
                .unwrap()
                .contains("\"order items\"")
        );
        design.toggle_object("notes").unwrap();
        let error = build_query(&catalog, &design, Driver::SQLite).unwrap_err();
        assert!(
            error.to_string().starts_with("notes has no foreign key"),
            "{error}"
        );
        design.toggle_object("notes").unwrap();
        design.toggle_object("order items").unwrap();
        assert_eq!(design.fields.len(), 2);

        // Sums need numbers, filters need values, and something has to be shown.
        design.fields[1].total = Total::Sum;
        assert!(build_query(&catalog, &design, Driver::SQLite).is_err());
        design.fields[1].total = Total::None;
        design.fields[1].value.clear();
        assert!(build_query(&catalog, &design, Driver::SQLite).is_err());
        design.fields[1].operator = Operator::NotEmpty;
        assert!(
            build_query(&catalog, &design, Driver::SQLite)
                .unwrap()
                .contains("WHERE customers.name IS NOT NULL")
        );
        for field in &mut design.fields {
            field.show = false;
        }
        assert!(build_query(&catalog, &design, Driver::SQLite).is_err());
        assert!(build_query(&catalog, &Design::default(), Driver::SQLite).is_err());
        assert_eq!(cycle(&Sort::ALL, Sort::Descending, true), Sort::None);
        assert_eq!(cycle(&Total::ALL, Total::None, false), Total::Max);
    }
}
