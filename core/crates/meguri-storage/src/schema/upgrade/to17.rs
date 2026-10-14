//! Step 17 (`storage/schema-upgrade` U14): `tags` rebuilt with `collapsed`
//! and `config`.

use rusqlite::Connection;

use super::StepResult;
use crate::schema::ddl18;

pub(super) fn run(conn: &Connection) -> StepResult {
    let tags = {
        let mut select = conn.prepare("SELECT tag, usn FROM tags")?;
        let rows = select.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    conn.execute_batch(&format!("DROP TABLE tags;\n{};", ddl18("tags")))?;
    let mut insert = conn.prepare("INSERT INTO tags (tag, usn, collapsed, config) VALUES (?1, ?2, 1, NULL)")?;
    for (tag, usn) in tags {
        insert.execute((&tag, usn))?;
    }
    Ok(())
}
