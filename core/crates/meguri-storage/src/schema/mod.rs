//! The schema-18 layout (`storage/schema-18`), the version probe and
//! refusals of the open gate (`storage/open-and-connections` C1, C2.1,
//! C2.2), the expected-object check (S6) and the upgrade of schemas 11 to 17
//! (`storage/schema-upgrade`).

pub mod expected_objects;
pub(crate) mod refuse;
pub(crate) mod upgrade;
pub(crate) mod version;

/// The schema version Meguri reads and writes.
pub const SCHEMA_VERSION: i64 = 18;

const DDL18: &str = include_str!("ddl18.sql");

/// The schema-18 statement that creates the table or index `name`, in the
/// normalized form of S2 and S3 (without the trailing `;`).
///
/// # Panics
///
/// When `ddl18.sql` has no such object: the names are this crate's own
/// constants.
pub(crate) fn ddl18(name: &str) -> &'static str {
    DDL18
        .lines()
        .filter(|line| line.starts_with("CREATE "))
        .find(|line| object_name(line) == Some(name))
        .map(|line| line.trim_end_matches(';'))
        .unwrap_or_else(|| panic!("ddl18.sql has no statement for {name}"))
}

/// The object a `CREATE TABLE`, `CREATE INDEX` or `CREATE UNIQUE INDEX`
/// statement names.
fn object_name(statement: &str) -> Option<&str> {
    let rest = statement
        .strip_prefix("CREATE TABLE ")
        .or_else(|| statement.strip_prefix("CREATE INDEX "))
        .or_else(|| statement.strip_prefix("CREATE UNIQUE INDEX "))?;
    rest.split_whitespace().next()
}

/// Every statement of `ddl18.sql`, in file order.
pub fn ddl18_statements() -> impl Iterator<Item = &'static str> {
    DDL18
        .lines()
        .filter(|line| line.starts_with("CREATE "))
        .map(|line| line.trim_end_matches(';'))
}
