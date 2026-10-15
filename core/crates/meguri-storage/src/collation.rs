//! The `unicase` collation (`storage/schema-18` S4, R1451): every connection
//! to a collection file registers it before touching any table, because the
//! name indexes and the `tags` key are ordered by it.

use rusqlite::Connection;

/// The collation's name, as the schema's `COLLATE unicase` clauses use it.
pub const UNICASE: &str = "unicase";

/// Registers `unicase` with the comparison of `text/unicode-primitives#U2`
/// (the pinned `unicase` crate's full case folding).
pub(crate) fn register(conn: &Connection) -> Result<(), String> {
    meguri_model::faultpoint!("storage.conn_init.unicase").map_err(|e| e.to_string())?;
    conn.create_collation(UNICASE, meguri_text::fold::compare_folded)
        .map_err(|e| e.to_string())
}
