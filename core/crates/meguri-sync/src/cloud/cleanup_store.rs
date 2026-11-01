//! P10.3's local computation. Network inventory completion is separate.
use super::*;
impl TrackingStore {
    pub(crate) fn compute_cleanup(&mut self, b: BindingId, at_ms: i64, media: bool) -> Result<(), Error> {
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let facts: Option<String> = tx
            .query_row(
                "SELECT facts FROM bindings WHERE id=?1 AND container=?2 AND environment=?3",
                rusqlite::params![
                    b.0.as_slice(),
                    self.scope.container.as_str(),
                    self.scope.environment.code()
                ],
                |r| r.get(0),
            )
            .optional()?;
        let facts: BindingFacts = store::decode(&facts.ok_or(Error::BindingNotFound)?)?;
        if facts.state != BindingState::Active {
            return Err(Error::IsolatedBinding);
        }
        publication_store::mark_zones_due(&tx, b, at_ms)?;
        conflict_expiry::expire_in_transaction(&tx, &self.directory, at_ms)?;
        let mut ledger = media::cleanup::ledger(&tx, b)?;
        ledger.cleanup_drain_ms = Some(at_ms);
        let eligible = media::cleanup::eligible(&tx, b, &ledger, at_ms, media)?;
        for (name, blob) in &mut ledger.blobs {
            blob.deletion_due_ms = eligible.contains(name).then_some(at_ms);
        }
        media::store::put_ledger(&tx, b, &ledger)?;
        // A9b remains false: no entity purge or absence deletion is computed.
        tx.commit()?;
        Ok(())
    }
}
use rusqlite::OptionalExtension;
