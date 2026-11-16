// SPDX-License-Identifier: MIT
use crate::Result;
pub fn register(connection: &rusqlite::Connection) -> Result<()> {
    connection.create_collation("unicase", |left, right| {
        unicase::UniCase::new(left).cmp(&unicase::UniCase::new(right))
    })?;
    Ok(())
}
