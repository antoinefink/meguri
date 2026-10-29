//! Adapter state shares the tracking connection, transaction and access gate.
use super::{
    adapter::Durable,
    store::{decode, encode},
    *,
};
use rusqlite::{OptionalExtension, params};
impl TrackingStore {
    pub(super) fn adapter_install(&mut self) -> Result<(), Error> {
        self.connection.execute_batch("CREATE TABLE IF NOT EXISTS cloud_adapter_state(binding BLOB PRIMARY KEY REFERENCES bindings(id), content TEXT NOT NULL CHECK(json_valid(content)))")?;
        Ok(())
    }
    pub(super) fn adapter_durable(&self, binding: BindingId) -> Result<Durable, Error> {
        let text: Option<String> = self
            .connection
            .query_row(
                "SELECT content FROM cloud_adapter_state WHERE binding=?1",
                [binding.0.as_slice()],
                |r| r.get(0),
            )
            .optional()?;
        text.map(|s| decode(&s)).transpose().map(|v| v.unwrap_or_default())
    }
    pub(super) fn adapter_commit(
        &mut self,
        binding: BindingId,
        entries: &[InboxEntry],
        refused: &[super::incoming::RefusedEntry],
        state: Option<&[u8]>,
        durable: &Durable,
        at_ms: i64,
        fail: bool,
    ) -> Result<(), Error> {
        if self.binding(binding)?.ok_or(Error::BindingNotFound)?.facts.state != BindingState::Active {
            return Err(Error::IsolatedBinding);
        }
        let mut facts = self.binding(binding)?.ok_or(Error::BindingNotFound)?.facts;
        let mut runtime = self.stored_runtime_state(binding)?;
        if durable.active_deleted {
            facts.state = BindingState::Isolated(IsolationReason::ActiveMissing);
            if let Some(runtime) = runtime.as_mut() {
                runtime.root_ack = None;
            }
        }
        let tx = self.connection.transaction()?;
        if durable.active_deleted {
            tx.execute(
                "UPDATE bindings SET facts=?2 WHERE id=?1",
                params![binding.0.as_slice(), encode(&facts)?],
            )?;
            if let Some(runtime) = runtime {
                tx.execute(
                    "UPDATE cloud_runtime_bindings SET content=?2 WHERE binding=?1",
                    params![binding.0.as_slice(), encode(&runtime)?],
                )?;
            }
        }
        super::incoming_store::receive(&tx, binding, entries, refused)?;
        if !entries.is_empty() || !refused.is_empty() {
            meguri_model::faultpoint!("cloud.tracking.commit.inbox")?;
        }
        if let Some(state) = state {
            tx.execute("INSERT INTO engine_state(binding,serialization,saved_at_ms) VALUES (?1,?2,?3) ON CONFLICT(binding) DO UPDATE SET serialization=excluded.serialization,saved_at_ms=excluded.saved_at_ms",params![binding.0.as_slice(),state,at_ms])?;
        }
        tx.execute("INSERT INTO cloud_adapter_state(binding,content) VALUES (?1,?2) ON CONFLICT(binding) DO UPDATE SET content=excluded.content", params![binding.0.as_slice(),encode(durable)?])?;
        // Fixture faults are consumed by Runtime only after preparation and
        // fail this same transaction before commit, rolling every write back.
        if fail {
            return Err(Error::Io(std::io::ErrorKind::Other));
        }
        if state.is_some() {
            meguri_model::faultpoint!("cloud.tracking.commit.engine_state")?;
        }
        meguri_model::faultpoint!("cloud.adapter.before_commit")?;
        tx.commit()?;
        Ok(())
    }
}

impl TrackingStore {
    pub(super) fn adapter_record_exchange(&mut self, binding: BindingId, durable: &Durable) -> Result<(), Error> {
        self.binding(binding)?.ok_or(Error::BindingNotFound)?;
        let tx = self.connection.transaction()?;
        tx.execute("INSERT INTO cloud_adapter_state(binding,content) VALUES (?1,?2) ON CONFLICT(binding) DO UPDATE SET content=excluded.content", params![binding.0.as_slice(),encode(durable)?])?;
        meguri_model::faultpoint!("cloud.adapter.before_commit")?;
        tx.commit()?;
        Ok(())
    }
}
