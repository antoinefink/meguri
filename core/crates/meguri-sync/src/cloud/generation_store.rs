//! Explicit additive admission and transactional storage for the standalone
//! local generation ledger. Existing open/runtime installers do not invoke it.
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use super::{
    generation::*,
    store::{decode, encode},
    *,
};

const MARKER_TABLE: &str = "CREATE TABLE cloud_generation_schema(version INTEGER NOT NULL CHECK(version=1))";
const LEDGER_TABLE: &str = "CREATE TABLE cloud_generation_ledgers(
 binding BLOB PRIMARY KEY REFERENCES bindings(id) ON DELETE CASCADE,
 content TEXT NOT NULL CHECK(json_valid(content))
)";

fn admitted(c: &Connection) -> Result<bool, Error> {
    let base: i64 = c.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if base != super::open::SCHEMA_VERSION {
        return Err(Error::RuntimeSchemaUnsupported);
    }
    let exists: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='cloud_generation_schema' AND type='table')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        let orphan:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name IN ('cloud_generation_schema','cloud_generation_ledgers'))",[],|r| r.get(0))?;
        return if orphan { Err(Error::InvalidData) } else { Ok(false) };
    }
    let mut q = c.prepare("SELECT version FROM cloud_generation_schema")?;
    let versions = q
        .query_map([], |r| r.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if versions != [1] {
        return Err(Error::RuntimeSchemaUnsupported);
    }
    // Shape admission is exact; never repair markerless/damaged/future tables.
    for (name, expected) in [
        ("cloud_generation_schema", MARKER_TABLE),
        ("cloud_generation_ledgers", LEDGER_TABLE),
    ] {
        let sql: Option<String> = c
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name=?1",
                [name],
                |r| r.get(0),
            )
            .optional()?;
        let normalize = |sql: &str| sql.split_whitespace().collect::<String>();
        if sql.as_deref().map(normalize) != Some(normalize(expected)) {
            return Err(Error::InvalidData);
        }
    }
    Ok(true)
}

fn authority(c: &Connection, scope: &BuildScope, b: BindingId, write: bool) -> Result<(), Error> {
    let facts: Option<String> = c
        .query_row(
            "SELECT facts FROM bindings WHERE id=?1 AND container=?2 AND environment=?3 AND salvage=0",
            params![b.0.as_slice(), scope.container.as_str(), scope.environment.code()],
            |r| r.get(0),
        )
        .optional()?;
    let facts: BindingFacts = decode(&facts.ok_or(Error::BindingNotFound)?)?;
    if write && facts.state != BindingState::Active {
        return Err(Error::IsolatedBinding);
    }
    Ok(())
}
fn load(c: &Connection, b: BindingId) -> Result<GenerationLedger, Error> {
    if !admitted(c)? {
        return Err(Error::RuntimeSchemaUnsupported);
    }
    let content: Option<String> = c
        .query_row(
            "SELECT content FROM cloud_generation_ledgers WHERE binding=?1",
            [b.0.as_slice()],
            |r| r.get(0),
        )
        .optional()?;
    decode_ledger(&content.ok_or(Error::InvalidData)?)
}
fn decode_ledger(content: &str) -> Result<GenerationLedger, Error> {
    let ledger: GenerationLedger = decode(content)?;
    ledger.validate()?;
    // BTreeMap's deserializer alone would silently accept duplicate keys.
    // Persisted state must match the one canonical encoding we write.
    if encode(&ledger)? != content {
        return Err(Error::InvalidData);
    }
    Ok(ledger)
}
fn put(c: &Connection, b: BindingId, ledger: &GenerationLedger) -> Result<(), Error> {
    ledger.validate()?;
    if c.execute(
        "UPDATE cloud_generation_ledgers SET content=?2 WHERE binding=?1",
        params![b.0.as_slice(), encode(ledger)?],
    )? != 1
    {
        return Err(Error::InvalidData);
    }
    Ok(())
}

impl TrackingStore {
    /// Standalone component admission. Seeds are authoritative caller facts for
    /// a new ledger only; pass [] to admit an existing ledger without reseeding.
    /// Never invoked by TrackingStore::open or an existing runtime path.
    pub fn initialize_generation_ledger(&mut self, b: BindingId, seeds: &[LayoutSeed]) -> Result<(), Error> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        authority(&tx, &self.scope, b, true)?;
        if !admitted(&tx)? {
            tx.execute_batch(MARKER_TABLE)?;
            tx.execute("INSERT INTO cloud_generation_schema VALUES(1)", [])?;
            tx.execute_batch(LEDGER_TABLE)?;
        }
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM cloud_generation_ledgers WHERE binding=?1)",
            [b.0.as_slice()],
            |r| r.get(0),
        )?;
        if exists {
            let ledger = load(&tx, b)?;
            if !seeds.is_empty() && GenerationLedger::new(seeds)?.seeds != ledger.seeds {
                return Err(Error::InvalidData);
            }
        } else {
            let ledger = GenerationLedger::new(seeds)?;
            ledger.validate()?;
            tx.execute(
                "INSERT INTO cloud_generation_ledgers(binding,content) VALUES(?1,?2)",
                params![b.0.as_slice(), encode(&ledger)?],
            )?;
        }
        // Validate only this build's non-salvage owners. Other scopes' opaque
        // contents are neither decoded nor overwritten by this admission.
        {
            let mut q=tx.prepare("SELECT l.content FROM cloud_generation_ledgers l JOIN bindings b ON b.id=l.binding WHERE b.container=?1 AND b.environment=?2 AND b.salvage=0")?;
            for text in q.query_map(
                params![self.scope.container.as_str(), self.scope.environment.code()],
                |r| r.get::<_, String>(0),
            )? {
                decode_ledger(&text?)?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn generation_ledger(&self, b: BindingId) -> Result<GenerationLedger, Error> {
        authority(&self.connection, &self.scope, b, false)?;
        load(&self.connection, b)
    }

    /// The future writer supplies all currently readable anchor observations,
    /// including already-inboxed ones, in this same tracking commit. This API
    /// neither reads collection files nor asserts observer/WAL integration.
    pub fn commit_local_generation(
        &mut self,
        b: BindingId,
        operation: LocalOperation,
        facts: &[AnchorFact],
    ) -> Result<LocalGeneration, Error> {
        self.with_generation_ledger(b, |ledger| ledger.local(operation, facts))
    }

    /// Ledger-side sent transition only. The future claim builder must commit
    /// its real sent marks atomically with this transition before SDK handoff.
    /// This standalone API does not construct or hand a claim batch.
    pub fn begin_generation_claim(
        &mut self,
        b: BindingId,
        id: GenerationId,
        last_order: u64,
    ) -> Result<ClaimTicket, Error> {
        self.with_generation_ledger(b, |ledger| ledger.begin(id, last_order))
    }

    /// No-manifest decisions only. Waiting successors and whole joined maps
    /// persist in the same transaction as the predecessor's retry/lost result.
    pub fn record_generation_claim_result(
        &mut self,
        b: BindingId,
        claim_attempt: ClaimTicket,
        result: ClaimResult,
        facts: &[AnchorFact],
    ) -> Result<LocalGeneration, Error> {
        self.with_generation_ledger(b, |ledger| ledger.result(claim_attempt, result, facts))
    }

    /// Semantic check for validated caller observations; no inbox insertion,
    /// cursor persistence, asset decoding or remote adoption is implemented.
    pub fn check_generation_anchors(&mut self, b: BindingId, facts: &[AnchorFact]) -> Result<(), Error> {
        self.with_generation_ledger(b, |ledger| ledger.check_anchors(facts))
    }

    fn with_generation_ledger<T>(
        &mut self,
        b: BindingId,
        update: impl FnOnce(&mut GenerationLedger) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        authority(&tx, &self.scope, b, true)?;
        let mut ledger = load(&tx, b)?;
        let result = update(&mut ledger)?;
        put(&tx, b, &ledger)?;
        tx.commit()?;
        Ok(result)
    }
}
