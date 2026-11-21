// SPDX-License-Identifier: MIT
use super::{DataRoot, root::unavailable};
use crate::Result;
impl DataRoot {
    pub(crate) fn upgrade_oauth(&self) -> Result<()> {
        self.check_initialized()?;
        let mut c = super::open(self.database(), false)?;
        let version: i64 = c.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if matches!(version, 4 | 5) { return Ok(()); }
        if version != 3 { return Err(unavailable()); }
        let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute_batch(include_str!("oauth.sql"))?;
        crate::accounts::phase("oauth_migration_before_commit")?;
        tx.commit()?;
        self.check_initialized()
    }
}
