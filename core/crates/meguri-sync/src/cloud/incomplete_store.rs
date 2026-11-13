//! P12.6.1 facts about ended exchanges. This extension neither resumes a
//! capture nor supplies send/apply authority. Every proof is written by its
//! production source transaction, including proofs arriving before P12.7.
#[cfg(test)]
#[path = "incomplete_store_tests.rs"]
mod tests;

use super::{adapter::CollectionOutcome, policy::ExchangeOutcome, store::{decode, encode}, workflow::{EntityFacts, Flight}, *};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const SCHEMA: &str = "
CREATE TABLE cloud_incomplete_schema(version INTEGER NOT NULL CHECK(version=1));
INSERT INTO cloud_incomplete_schema VALUES(1);
CREATE TABLE cloud_incomplete_bindings(
 binding BLOB PRIMARY KEY REFERENCES bindings(id) ON DELETE CASCADE,
 history_known INTEGER NOT NULL CHECK(history_known IN (0,1))
);
CREATE TABLE cloud_incomplete_legacy(
 binding BLOB NOT NULL REFERENCES bindings(id) ON DELETE CASCADE,
 owner TEXT NOT NULL, exchange INTEGER NOT NULL CHECK(exchange>0), PRIMARY KEY(binding,owner,exchange)
);
CREATE TABLE cloud_incomplete_ends(
 binding BLOB NOT NULL REFERENCES bindings(id) ON DELETE CASCADE,
 native INTEGER NOT NULL CHECK(native>0), content TEXT NOT NULL CHECK(json_valid(content)),
 opened INTEGER NOT NULL CHECK(opened>0),
 active INTEGER NOT NULL CHECK(active IN (0,1)),
 accepted INTEGER NOT NULL CHECK(accepted IN (0,1)),
 success INTEGER NOT NULL CHECK(success IN (0,1)),
 PRIMARY KEY(binding,native)
);
CREATE INDEX cloud_incomplete_active ON cloud_incomplete_ends(binding,opened,native) WHERE active=1;
CREATE INDEX cloud_incomplete_success ON cloud_incomplete_ends(binding,opened DESC,native DESC) WHERE success=1 AND accepted=1;
CREATE TABLE cloud_incomplete_outgoing(
 binding BLOB NOT NULL, name TEXT NOT NULL, native INTEGER NOT NULL,
 revision INTEGER NOT NULL CHECK(revision>0),
 PRIMARY KEY(binding,name,native),
 FOREIGN KEY(binding,native) REFERENCES cloud_incomplete_ends(binding,native) ON DELETE CASCADE
);
CREATE INDEX cloud_incomplete_outgoing_end ON cloud_incomplete_outgoing(binding,native);
CREATE TABLE cloud_incomplete_incoming(
 binding BLOB NOT NULL, inbox INTEGER NOT NULL CHECK(inbox>0), identity BLOB NOT NULL CHECK(length(identity)=32), native INTEGER NOT NULL,
 PRIMARY KEY(binding,inbox,identity,native),
 FOREIGN KEY(binding,native) REFERENCES cloud_incomplete_ends(binding,native) ON DELETE CASCADE
);
CREATE INDEX cloud_incomplete_incoming_end ON cloud_incomplete_incoming(binding,native);
CREATE TABLE cloud_incomplete_results(
 binding BLOB NOT NULL REFERENCES bindings(id) ON DELETE CASCADE,
 owner TEXT NOT NULL, exchange INTEGER NOT NULL CHECK(exchange>0), native INTEGER CHECK(native>0),
 PRIMARY KEY(binding,owner,exchange),
 FOREIGN KEY(binding,native) REFERENCES cloud_incomplete_ends(binding,native)
);
CREATE UNIQUE INDEX cloud_incomplete_result_original ON cloud_incomplete_results(binding,native) WHERE native IS NOT NULL;
CREATE TABLE cloud_incomplete_flights(
 binding BLOB NOT NULL REFERENCES bindings(id) ON DELETE CASCADE,
 name TEXT NOT NULL, identity BLOB NOT NULL CHECK(length(identity)=32),
 opened INTEGER NOT NULL CHECK(opened>0), seq INTEGER NOT NULL CHECK(seq>=0),
 handed INTEGER NOT NULL CHECK(handed IN (0,1)),
 PRIMARY KEY(binding,name,identity)
);";

pub(super) fn check_version(c: &Connection) -> Result<bool, Error> {
    let object: Option<String> = c.query_row(
        "SELECT type FROM sqlite_master WHERE name='cloud_incomplete_schema'", [], |row| row.get(0)).optional()?;
    match object.as_deref() {
        Some("table") => {}
        Some(_) => return Err(Error::InvalidData),
        None => {
            let orphan: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name IN ('cloud_incomplete_bindings','cloud_incomplete_legacy','cloud_incomplete_ends','cloud_incomplete_flights','cloud_incomplete_results','cloud_incomplete_outgoing','cloud_incomplete_incoming'))", [], |r| r.get(0))?;
            return if orphan { Err(Error::InvalidData) } else { Ok(false) };
        }
    }
    let versions = c.prepare("SELECT version FROM cloud_incomplete_schema LIMIT 2")?.query_map([], |r| r.get::<_, i64>(0))?.collect::<Result<Vec<_>, _>>()?;
    if versions != [1] { return Err(Error::RuntimeSchemaUnsupported); }
    c.prepare("SELECT binding,history_known FROM cloud_incomplete_bindings LIMIT 0")?;
    c.prepare("SELECT binding,owner,exchange FROM cloud_incomplete_legacy LIMIT 0")?;
    c.prepare("SELECT binding,native,content,opened,active,accepted,success FROM cloud_incomplete_ends LIMIT 0")?;
    c.prepare("SELECT binding,name,identity,opened,seq,handed FROM cloud_incomplete_flights LIMIT 0")?;
    c.prepare("SELECT binding,owner,exchange,native FROM cloud_incomplete_results LIMIT 0")?;
    c.prepare("SELECT binding,name,native,revision FROM cloud_incomplete_outgoing LIMIT 0")?;
    c.prepare("SELECT binding,inbox,identity,native FROM cloud_incomplete_incoming LIMIT 0")?;
    Ok(true)
}

// Neither field is a clock or result-delivery ordinal. Opens are already
// durable and exclusive; captures within one open are serialized by Runtime.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Order { opened: u64, native: u64 }
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Outgoing { revision: u64, done: bool }
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Incoming { inbox: i64, identity: [u8; 32], done: bool }
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Ended {
    order: Order,
    boundary: u64,
    ended_ms: i64,
    outcome: CollectionOutcome,
    outgoing: BTreeMap<String, Outgoing>,
    incoming: Vec<Incoming>,
    work_known: bool,
    accepted: bool,
    cleared_by_success: bool,
    settled: bool,
}
impl Ended {
    pub(super) fn new(opened: u64, native: u64, ended_ms: i64, completed: &super::adapter::CompletedExchange,
        revisions: &BTreeMap<String, u64>, entries: Vec<StoredInboxEntry>) -> Result<Self, Error> {
        if completed.id != native { return Err(Error::InvalidData); }
        let outgoing: BTreeMap<String, Outgoing> = completed.result.outstanding_names.iter().chain(&completed.result.blocked_names)
            .map(|name| Ok((name.clone(), Outgoing { revision: *revisions.get(name).ok_or(Error::InvalidData)?, done: false })))
            .collect::<Result<_, Error>>()?;
        let incoming: Vec<Incoming> = entries.iter().filter(|e| matches!(e.entry.reason,
            InboxReason::Ready | InboxReason::Format | InboxReason::Asset | InboxReason::ApplyFailed { .. }))
            .map(|e| Ok(Incoming { inbox: e.id, identity: incoming_version_key(&e.entry)?, done: false })).collect::<Result<_, Error>>()?;
        // An incomplete without identifiable work is not a known empty
        // all-done proof (for example, unavailable authority at the end).
        let work_known = completed.collection != CollectionOutcome::Incomplete
            || !outgoing.is_empty() || !incoming.is_empty();
        let result = Self { order: Order { opened, native }, boundary: completed.result.boundary_seq, ended_ms,
            outcome: completed.collection.clone(), outgoing, incoming, work_known, accepted: false, cleared_by_success: false, settled: false };
        result.validate(native)?;
        Ok(result)
    }
    fn validate(&self, native: u64) -> Result<(), Error> {
        if self.order.native != native || native == 0 || self.order.opened == 0
            || self.outgoing.iter().any(|(name, work)| name.is_empty() || work.revision == 0)
            || self.incoming.iter().any(|work| work.inbox <= 0)
            || self.cleared_by_success && !matches!(self.outcome, CollectionOutcome::Incomplete)
        { return Err(Error::InvalidData); }
        super::runtime_store::checked(self.order.opened)?;
        super::runtime_store::checked(native)?;
        super::runtime_store::checked(self.boundary)?;
        for work in self.outgoing.values() { super::runtime_store::checked(work.revision)?; }
        Ok(())
    }
}

// Indexed tables hold unfinished identities. Receipts preserve original counts,
// digest and order, plus the transactionally maintained remaining counts.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    end: Ended,
    outgoing_count: u64,
    incoming_count: u64,
    cause_digest: [u8; 32],
    outgoing_left: u64,
    incoming_left: u64,
}
fn read_end(c: &Connection, b: BindingId, native: i64) -> Result<Receipt, Error> {
    let text: String = c.query_row("SELECT content FROM cloud_incomplete_ends WHERE binding=?1 AND native=?2",
        params![b.0.as_slice(), native], |r| r.get(0))?;
    let receipt: Receipt = decode(&text)?;
    receipt.end.validate(native.try_into().map_err(|_| Error::InvalidData)?)?;
    if !receipt.end.outgoing.is_empty() || !receipt.end.incoming.is_empty()
        || receipt.end.settled && !receipt.end.work_known
        || !receipt.end.work_known && (receipt.outgoing_count != 0 || receipt.incoming_count != 0)
    { return Err(Error::InvalidData); }
    if receipt.outgoing_left > receipt.outgoing_count || receipt.incoming_left > receipt.incoming_count
        || !active(&receipt.end) && (receipt.outgoing_left != 0 || receipt.incoming_left != 0)
        || active(&receipt.end) && receipt.end.work_known && receipt.outgoing_left == 0 && receipt.incoming_left == 0
    { return Err(Error::InvalidData); }
    super::runtime_store::checked(receipt.outgoing_count)?;
    super::runtime_store::checked(receipt.incoming_count)?;
    Ok(receipt)
}
// Used on open and in source tests, never by an ordinary proof callback.
fn ends(c: &Connection, b: BindingId) -> Result<Vec<Ended>, Error> {
    let mut q = c.prepare("SELECT native FROM cloud_incomplete_ends WHERE binding=?1 ORDER BY native")?;
    q.query_map([b.0.as_slice()], |r| r.get::<_, i64>(0))?
        .map(|native| Ok(read_end(c, b, native?)?.end)).collect()
}
fn active(end: &Ended) -> bool {
    end.outcome == CollectionOutcome::Incomplete && !end.settled && !end.cleared_by_success
}
fn write_receipt(tx: &Transaction<'_>, b: BindingId, receipt: &Receipt) -> Result<(), Error> {
    let end = &receipt.end;
    tx.execute("UPDATE cloud_incomplete_ends SET content=?3,active=?4,accepted=?5 WHERE binding=?1 AND native=?2",
        params![b.0.as_slice(), super::runtime_store::checked(end.order.native)?, encode(receipt)?, active(end), end.accepted])?;
    Ok(())
}
fn retire_work(tx: &Transaction<'_>, b: BindingId, native: i64) -> Result<(), Error> {
    tx.execute("DELETE FROM cloud_incomplete_outgoing WHERE binding=?1 AND native=?2", params![b.0.as_slice(), native])?;
    tx.execute("DELETE FROM cloud_incomplete_incoming WHERE binding=?1 AND native=?2", params![b.0.as_slice(), native])?;
    Ok(())
}
// Counters change with the exact cause deletion in the same transaction.
// A missing cause leaves its counter outstanding; it cannot prove completion.
fn finish_cause(tx: &Transaction<'_>, b: BindingId, mut receipt: Receipt, outgoing: bool) -> Result<(), Error> {
    let left = if outgoing { &mut receipt.outgoing_left } else { &mut receipt.incoming_left };
    *left = left.checked_sub(1).ok_or(Error::InvalidData)?;
    receipt.end.settled = receipt.end.work_known && receipt.outgoing_left == 0 && receipt.incoming_left == 0;
    write_receipt(tx, b, &receipt)
}
pub(super) fn record_end(tx: &Transaction<'_>, b: BindingId, end: &Ended) -> Result<(), Error> {
    end.validate(end.order.native)?;
    let native = super::runtime_store::checked(end.order.native)?;
    let mut compact = end.clone();
    compact.outgoing.clear();
    compact.incoming.clear();
    compact.settled = end.work_known && end.outgoing.values().all(|work| work.done)
        && end.incoming.iter().all(|work| work.done);
    let receipt = Receipt { end: compact, outgoing_count: end.outgoing.len() as u64, incoming_count: end.incoming.len() as u64,
        cause_digest: entity_codec::sha256(encode(&(&end.outgoing, &end.incoming))?.as_bytes()),
        outgoing_left: if active(&end) { end.outgoing.values().filter(|w| !w.done).count() as u64 } else { 0 },
        incoming_left: if active(&end) { end.incoming.iter().filter(|w| !w.done).count() as u64 } else { 0 } };
    tx.execute("INSERT INTO cloud_incomplete_ends(binding,native,content,opened,active,accepted,success) VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![b.0.as_slice(), native, encode(&receipt)?, super::runtime_store::checked(end.order.opened)?, active(&receipt.end),
            end.accepted, end.outcome == CollectionOutcome::Succeeded])?;
    if active(&receipt.end) {
        for (name, work) in &end.outgoing {
            if !work.done { tx.execute("INSERT INTO cloud_incomplete_outgoing VALUES(?1,?2,?3,?4)",
                params![b.0.as_slice(), name, native, super::runtime_store::checked(work.revision)?])?; }
        }
        for work in &end.incoming {
            if !work.done { tx.execute("INSERT INTO cloud_incomplete_incoming VALUES(?1,?2,?3,?4)",
                params![b.0.as_slice(), work.inbox, work.identity.as_slice(), native])?; }
        }
    }
    Ok(())
}
pub(super) fn start_binding_ledger(tx: &Transaction<'_>, binding: BindingId) -> Result<(), Error> {
    if !check_version(tx)? { return Ok(()); }
    let inserted = tx.execute(
        "INSERT INTO cloud_incomplete_bindings(history_known,binding) VALUES(1,?1)",
        [binding.0.as_slice()])?;
    if inserted != 1 { return Err(Error::InvalidData); }
    Ok(())
}
pub(super) fn validate_scope(c: &Connection, scope: &BuildScope) -> Result<(), Error> {
    if !check_version(c)? { return Ok(()); }
    let mut q = c.prepare("SELECT id FROM bindings WHERE container=?1 AND environment=?2 AND salvage=0")?;
    for row in q.query_map(params![scope.container.as_str(), scope.environment.code()], |r| r.get::<_, Vec<u8>>(0))? {
        let b = super::store::binding_id(row?)?;
        for sql in [
            "SELECT EXISTS(SELECT 1 FROM cloud_incomplete_ends WHERE binding=?1 AND (typeof(native)!='integer' OR native<=0 OR typeof(opened)!='integer' OR opened<=0 OR typeof(active)!='integer' OR active NOT IN (0,1) OR typeof(accepted)!='integer' OR accepted NOT IN (0,1) OR typeof(success)!='integer' OR success NOT IN (0,1)))",
            "SELECT EXISTS(SELECT 1 FROM cloud_incomplete_results x LEFT JOIN cloud_incomplete_ends e ON e.binding=x.binding AND e.native=x.native WHERE x.binding=?1 AND (typeof(x.owner)!='text' OR length(x.owner)=0 OR typeof(x.exchange)!='integer' OR x.exchange<=0 OR x.native IS NOT NULL AND (typeof(x.native)!='integer' OR x.native<=0 OR e.native IS NULL)))",
            "SELECT EXISTS(SELECT 1 FROM cloud_incomplete_bindings WHERE binding=?1 AND (typeof(history_known)!='integer' OR history_known NOT IN (0,1)))",
            "SELECT EXISTS(SELECT 1 FROM cloud_incomplete_legacy WHERE binding=?1 AND (typeof(owner)!='text' OR length(owner)=0 OR typeof(exchange)!='integer' OR exchange<=0))",
            "SELECT EXISTS(SELECT 1 FROM cloud_incomplete_flights WHERE binding=?1 AND (typeof(opened)!='integer' OR opened<=0 OR typeof(seq)!='integer' OR seq<0 OR typeof(identity)!='blob' OR length(identity)!=32 OR typeof(name)!='text' OR length(name)=0 OR typeof(handed)!='integer' OR handed NOT IN (0,1)))",
            "SELECT EXISTS(SELECT 1 FROM cloud_incomplete_outgoing w LEFT JOIN cloud_incomplete_ends e ON e.binding=w.binding AND e.native=w.native WHERE w.binding=?1 AND (typeof(w.name)!='text' OR length(w.name)=0 OR typeof(w.revision)!='integer' OR w.revision<=0 OR e.native IS NULL OR e.active!=1))",
            "SELECT EXISTS(SELECT 1 FROM cloud_incomplete_incoming w LEFT JOIN cloud_incomplete_ends e ON e.binding=w.binding AND e.native=w.native WHERE w.binding=?1 AND (typeof(w.inbox)!='integer' OR w.inbox<=0 OR typeof(w.identity)!='blob' OR length(w.identity)!=32 OR e.native IS NULL OR e.active!=1))",
        ] {
            if c.query_row(sql, [b.0.as_slice()], |r| r.get::<_, bool>(0))? { return Err(Error::InvalidData); }
        }
        let rows = ends(c, b)?;
        for end in &rows {
            let mirror: (i64, bool, bool, bool) = c.query_row("SELECT opened,active,accepted,success FROM cloud_incomplete_ends WHERE binding=?1 AND native=?2",
                params![b.0.as_slice(), super::runtime_store::checked(end.order.native)?], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
            if mirror != (super::runtime_store::checked(end.order.opened)?, active(end), end.accepted, end.outcome == CollectionOutcome::Succeeded) { return Err(Error::InvalidData); }
            let receipt = read_end(c, b, super::runtime_store::checked(end.order.native)?)?;
            let counts: (i64, i64) = c.query_row("SELECT
                (SELECT count(*) FROM cloud_incomplete_outgoing WHERE binding=?1 AND native=?2),
                (SELECT count(*) FROM cloud_incomplete_incoming WHERE binding=?1 AND native=?2)",
                params![b.0.as_slice(), super::runtime_store::checked(end.order.native)?], |r| Ok((r.get(0)?,r.get(1)?)))?;
            if counts.0 != super::runtime_store::checked(receipt.outgoing_left)?
                || counts.1 != super::runtime_store::checked(receipt.incoming_left)?
                || active(end) && end.work_known && counts == (0, 0)
                || !active(end) && counts != (0, 0)
            { return Err(Error::InvalidData); }
        }

        let mut q = c.prepare("SELECT owner,exchange,native FROM cloud_incomplete_results WHERE binding=?1")?;
        let mut linked = std::collections::BTreeSet::new();
        for row in q.query_map([b.0.as_slice()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<i64>>(2)?)))? {
            let (owner, exchange, native) = row?;
            let text: Option<String> = c.query_row("SELECT content FROM cloud_policy_exchanges WHERE binding=?1 AND owner=?2 AND exchange=?3",
                params![b.0.as_slice(), owner, exchange], |r| r.get(0)).optional()?;
            let record: super::policy::ExchangeRecord = decode(&text.ok_or(Error::InvalidData)?)?;
            if super::runtime_store::checked(record.exchange)? != exchange { return Err(Error::InvalidData); }
            if let Some(native) = native {
                let native = u64::try_from(native).map_err(|_| Error::InvalidData)?;
                let end = rows.iter().find(|e| e.order.native == native).ok_or(Error::InvalidData)?;
                if !end.accepted || !matches_record(end, &record) || !linked.insert(native) {
                    return Err(Error::InvalidData);
                }
            } else if matches!(record.outcome, ExchangeOutcome::Incomplete { .. }) {
                let retained: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM cloud_incomplete_legacy WHERE binding=?1 AND owner=?2 AND exchange=?3)",
                    params![b.0.as_slice(), owner, exchange], |r| r.get(0))?;
                if !retained { return Err(Error::InvalidData); }
            }
        }
        if rows.iter().any(|e| e.accepted && !linked.contains(&e.order.native)) { return Err(Error::InvalidData); }
    }
    Ok(())
}
fn matches_record(end: &Ended, record: &super::policy::ExchangeRecord) -> bool {
    record.ended_ms == end.ended_ms && matches!((&record.outcome, &end.outcome),
        (ExchangeOutcome::Succeeded { .. }, CollectionOutcome::Succeeded)
        | (ExchangeOutcome::Incomplete { .. }, CollectionOutcome::Incomplete)
        | (ExchangeOutcome::Waiting { .. }, CollectionOutcome::WaitingNoAccount)
        | (ExchangeOutcome::Failed { .. }, CollectionOutcome::Failed)
        | (ExchangeOutcome::Cancelled, CollectionOutcome::Cancelled))
}
impl TrackingStore {
    pub(super) fn install_incomplete(&mut self) -> Result<(), Error> {
        let present = check_version(&self.connection)?;
        let tx = self.connection.transaction()?;
        if !present { tx.execute_batch(SCHEMA)?; }
        let ids = {
            let mut q = tx.prepare("SELECT id FROM bindings WHERE container=?1 AND environment=?2 AND salvage=0")?;
            q.query_map(params![self.scope.container.as_str(), self.scope.environment.code()], |r| r.get::<_, Vec<u8>>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        for bytes in ids {
            let b = super::store::binding_id(bytes)?;
            let _ = ends(&tx, b)?;
            tx.execute("INSERT INTO cloud_incomplete_bindings(binding,history_known) VALUES(?1,0) ON CONFLICT(binding) DO NOTHING", [b.0.as_slice()])?;
            // A display row without our atomic receipt has unknown original
            // work/order, including rows a historical build wrote after an
            // earlier upgrade. Inspect only this build's ordinary owners.
            let records = {
                let mut q = tx.prepare("SELECT p.owner,p.exchange,p.content FROM cloud_policy_exchanges p
                    LEFT JOIN cloud_incomplete_results r ON r.binding=p.binding AND r.owner=p.owner AND r.exchange=p.exchange
                    WHERE p.binding=?1 AND r.exchange IS NULL")?;
                q.query_map([b.0.as_slice()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?)))?.collect::<Result<Vec<_>, _>>()?
            };
            for (owner, exchange, text) in records {
                let record: super::policy::ExchangeRecord = decode(&text)?;
                if owner.is_empty() || super::runtime_store::checked(record.exchange)? != exchange { return Err(Error::InvalidData); }
                tx.execute("UPDATE cloud_incomplete_bindings SET history_known=0 WHERE binding=?1", [b.0.as_slice()])?;
                if matches!(record.outcome, ExchangeOutcome::Incomplete { .. }) { unknown(&tx, b, &owner, record.exchange)?; }
                tx.execute("INSERT INTO cloud_incomplete_results(binding,owner,exchange,native) VALUES(?1,?2,?3,NULL)",
                    params![b.0.as_slice(), owner, exchange])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
    // Final guarded batch production, after every fallible conditional read.
    // Preparation admits no proof. At the actual return boundary, attach this
    // handoff's opening and image sequence even for identical retained flights.
    pub(super) fn record_incomplete_handoffs(&mut self, b: BindingId,
        opened: u64, flights: &[(String, super::workflow::FlightIdentity, u64)]) -> Result<(), Error> {
        if flights.is_empty() { return Ok(()); }
        if opened == 0 { return Err(Error::InvalidData); }
        self.binding(b)?.ok_or(Error::BindingNotFound)?;
        if !check_version(&self.connection)? { return Err(Error::InvalidData); }
        let tx = self.connection.transaction()?;
        for (name, identity, seq) in flights {
            let key = entity_codec::sha256(encode(identity)?.as_bytes());
            tx.execute("UPDATE cloud_incomplete_flights SET handed=1,opened=?4,seq=?5 WHERE binding=?1 AND name=?2 AND identity=?3 AND (handed=0 OR opened<?4 OR opened=?4 AND seq<?5)",
                params![b.0.as_slice(), name, key.as_slice(), super::runtime_store::checked(opened)?, super::runtime_store::checked(*seq)?])?;
        }
        tx.commit()?;
        Ok(())
    }
    #[cfg(feature = "test-hooks")]
    pub fn incomplete_pending_for_testing(&self, b: BindingId) -> Result<bool, Error> {
        self.exchange_incomplete(b)
    }
    #[cfg(feature = "test-hooks")]
    pub fn incomplete_history_for_testing(&self, b: BindingId) -> Result<Option<bool>, Error> {
        self.binding(b)?.ok_or(Error::BindingNotFound)?;
        self.connection.query_row("SELECT history_known FROM cloud_incomplete_bindings WHERE binding=?1", [b.0.as_slice()], |r| r.get(0)).optional().map_err(Error::from)
    }
    pub(super) fn exchange_incomplete(&self, b: BindingId) -> Result<bool, Error> {
        self.binding(b)?.ok_or(Error::BindingNotFound)?;
        if !check_version(&self.connection)? {
            // Missing schema is unknown, not an authoritative clean ledger.
            return Ok(false);
        }
        let legacy: bool = self.connection.query_row("SELECT EXISTS(SELECT 1 FROM cloud_incomplete_legacy WHERE binding=?1)", [b.0.as_slice()], |r| r.get(0))?;
        let pending: bool = self.connection.query_row("SELECT EXISTS(SELECT 1 FROM cloud_incomplete_ends WHERE binding=?1 AND active=1 AND accepted=1)", [b.0.as_slice()], |r| r.get(0))?;
        Ok(legacy || pending)
    }
}
fn unknown(tx: &Transaction<'_>, b: BindingId, owner: &str, exchange: u64) -> Result<(), Error> {
    tx.execute("INSERT INTO cloud_incomplete_legacy(binding,owner,exchange) VALUES(?1,?2,?3) ON CONFLICT DO NOTHING",
        params![b.0.as_slice(), owner, super::runtime_store::checked(exchange)?])?;
    Ok(())
}
pub(super) fn accept(tx: &Transaction<'_>, b: BindingId, owner: &str, record: &super::policy::ExchangeRecord,
    native: Option<u64>) -> Result<(), Error> {
    if !check_version(tx)? { return Err(Error::InvalidData); }
    let mut receipt = native.map(|n| read_end(tx, b, super::runtime_store::checked(n)?)).transpose()?;
    if let Some(receipt) = &mut receipt {
        if !matches_record(&receipt.end, record) { return Err(Error::InvalidData); }
        receipt.end.accepted = true;
        write_receipt(tx, b, receipt)?;
    } else if matches!(record.outcome, ExchangeOutcome::Incomplete { .. }) {
        unknown(tx, b, owner, record.exchange)?;
    }
    tx.execute("INSERT INTO cloud_incomplete_results(binding,owner,exchange,native) VALUES(?1,?2,?3,?4)",
        params![b.0.as_slice(), owner, super::runtime_store::checked(record.exchange)?, native.map(super::runtime_store::checked).transpose()?])?;
    let latest: Option<(i64, i64)> = tx.query_row("SELECT opened,native FROM cloud_incomplete_ends WHERE binding=?1 AND success=1 AND accepted=1 ORDER BY opened DESC,native DESC LIMIT 1",
        [b.0.as_slice()], |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
    if let Some((opened, native)) = latest {
        let affected = {
            let mut q = tx.prepare("SELECT native FROM cloud_incomplete_ends WHERE binding=?1 AND active=1 AND (opened,native)<(?2,?3)")?;
            q.query_map(params![b.0.as_slice(), opened, native], |r| r.get::<_, i64>(0))?.collect::<Result<Vec<_>, _>>()?
        };
        for native in affected {
            let mut receipt = read_end(tx, b, native)?;
            receipt.end.cleared_by_success = true;
            receipt.outgoing_left = 0;
            receipt.incoming_left = 0;
            retire_work(tx, b, native)?;
            write_receipt(tx, b, &receipt)?;
        }
    }
    Ok(())
}

fn flight_identity(f: &Flight) -> Result<[u8; 32], Error> {
    Ok(entity_codec::sha256(encode(&f.identity())?.as_bytes()))
}
pub(super) fn stage_flight_origin(tx: &Transaction<'_>, b: BindingId, name: &str, f: &Flight, opened: u64) -> Result<(), Error> {
    if !check_version(tx)? { return Ok(()); }
    let namespace = super::runtime_store::checked(opened)?;
    let image = super::runtime_store::checked(f.image_seq)?;
    let key = flight_identity(f)?;
    tx.execute("INSERT OR IGNORE INTO cloud_incomplete_flights(binding,name,identity,opened,seq,handed)
        SELECT ?1,?2,?3,?4,?5,0", params![b.0.as_slice(), name, key.as_slice(), namespace, image])?;
    Ok(())
}
pub(super) fn ack(tx: &Transaction<'_>, b: BindingId, name: &str, f: &Flight) -> Result<(), Error> {
    if !check_version(tx)? { return Ok(()); }
    let stamp: Option<(i64, i64)> = tx.query_row("SELECT opened,seq FROM cloud_incomplete_flights WHERE binding=?1 AND name=?2 AND identity=?3 AND handed=1",
        params![b.0.as_slice(), name, flight_identity(f)?.as_slice()], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
    let Some((opened, seq)) = stamp else { return Ok(()); };
    let opened = u64::try_from(opened).map_err(|_| Error::InvalidData)?;
    let seq = u64::try_from(seq).map_err(|_| Error::InvalidData)?;
    ack_image(tx, b, name, opened, seq)
}
pub(super) fn ack_image(tx: &Transaction<'_>, b: BindingId, name: &str, opened: u64, seq: u64) -> Result<(), Error> {
    super::runtime_store::checked(opened)?;
    super::runtime_store::checked(seq)?;
    if opened == 0 { return Err(Error::InvalidData); }
    if !check_version(tx)? { return Ok(()); }
    settle_named_work(tx, b, name, |_, end| opened > end.order.opened || opened == end.order.opened && seq >= end.boundary)
}
// Indexed lookup visits only pending work for this name, including original
// completions whose policy result has not been delivered yet.
fn settle_named_work(tx: &Transaction<'_>, binding: BindingId, name: &str,
    accepts: impl Fn(u64, &Ended) -> bool) -> Result<(), Error> {
    let candidates = {
        let mut select = tx.prepare("SELECT native,revision FROM cloud_incomplete_outgoing WHERE name=?2 AND binding=?1")?;
        select.query_map(params![binding.0.as_slice(), name], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?
    };
    for (native, revision) in candidates {
        let original = read_end(tx, binding, native)?;
        let revision = u64::try_from(revision).map_err(|_| Error::InvalidData)?;
        if !accepts(revision, &original.end) { continue; }
        let removed = tx.execute("DELETE FROM cloud_incomplete_outgoing WHERE native=?3 AND binding=?1 AND name=?2",
            params![binding.0.as_slice(), name, native])?;
        if removed != 1 { return Err(Error::InvalidData); }
        finish_cause(tx, binding, original, true)?;
    }
    Ok(())
}
pub(super) fn clean(tx: &Transaction<'_>, b: BindingId, e: &EntityFacts) -> Result<(), Error> {
    if e.server_erev == 0 || e.mark.is_some() || !check_version(tx)? { return Ok(()); }
    settle_named_work(tx, b, &e.name, |_, _| true)
}
pub(super) fn dropped(tx: &Transaction<'_>, b: BindingId, name: &str, revision: u64) -> Result<(), Error> {
    if !check_version(tx)? { return Ok(()); }
    settle_named_work(tx, b, name, |original, _| revision >= original)
}

// Flight evidence is needed only while a matching retained flight can settle.
// Header writers retire at most the flights for this one entity (K12's cap 250).
pub(super) fn retain_flights(tx: &Transaction<'_>, b: BindingId, name: &str, flights: &[Flight]) -> Result<(), Error> {
    if !check_version(tx)? { return Ok(()); }
    let live = flights.iter().map(flight_identity).collect::<Result<std::collections::BTreeSet<_>, _>>()?;
    let keys = {
        let mut q = tx.prepare("SELECT identity FROM cloud_incomplete_flights WHERE binding=?1 AND name=?2")?;
        q.query_map(params![b.0.as_slice(), name], |r| r.get::<_, Vec<u8>>(0))?.collect::<Result<Vec<_>, _>>()?
    };
    for key in keys {
        let identity: [u8; 32] = key.try_into().map_err(|_| Error::InvalidData)?;
        if !live.contains(&identity) {
            tx.execute("DELETE FROM cloud_incomplete_flights WHERE binding=?1 AND name=?2 AND identity=?3", params![b.0.as_slice(), name, identity.as_slice()])?;
        }
    }
    Ok(())
}

// P12.6.1 follows the accepted server version across classification and asset
// staging. Local retry metadata therefore contributes no version evidence.
fn incoming_version_key(entry: &InboxEntry) -> Result<[u8; 32], Error> {
    #[derive(Serialize)]
    enum VersionEvidence<'a> {
        Deleted,
        Entity { origin: [u8; 16], revision: u64, digest: [u8; 32], tombstone: bool },
        Opaque { digest: [u8; 32], system: &'a OpaqueBytes },
    }
    let evidence = match &entry.value {
        InboxValue::Deletion => VersionEvidence::Deleted,
        InboxValue::Record { envelope, system_fields, .. } => {
            let entity = match entry.record_type {
                RecordType::Entity => match entity_codec::Entity::decode(&envelope.0).map_err(|_| Error::InvalidData)? {
                    entity_codec::DecodeOutcome::Supported(entity) => Some(entity),
                    entity_codec::DecodeOutcome::UnsupportedFormat(_) => None,
                },
                _ => None,
            };
            if let Some(entity) = entity {
                VersionEvidence::Entity { digest: entity.payload_sha256, revision: entity.fields.erev,
                    origin: entity.fields.origin, tombstone: entity.fields.tombstone }
            } else {
                VersionEvidence::Opaque { digest: entity_codec::sha256(&envelope.0), system: system_fields }
            }
        }
    };
    let canonical = encode(&(entry.record_type, &entry.record_name, evidence))?;
    Ok(entity_codec::sha256(canonical.as_bytes()))
}
// The caller names the original entry that its accepted K13 transaction
// settles. A same-name replacement, refusal or raw deletion is not this proof.
pub(super) fn incoming_done(tx: &Transaction<'_>, b: BindingId, inbox: i64, original: &InboxEntry) -> Result<(), Error> {
    if !check_version(tx)? { return Ok(()); }
    let key = incoming_version_key(original)?;
    let affected = {
        let mut q = tx.prepare("SELECT native FROM cloud_incomplete_incoming WHERE binding=?1 AND inbox=?2 AND identity=?3")?;
        q.query_map(params![b.0.as_slice(), inbox, key.as_slice()], |r| r.get::<_, i64>(0))?.collect::<Result<Vec<_>, _>>()?
    };
    for native in affected {
        let receipt = read_end(tx, b, native)?;
        let removed = tx.execute("DELETE FROM cloud_incomplete_incoming WHERE binding=?1 AND inbox=?2 AND identity=?3 AND native=?4",
            params![b.0.as_slice(), inbox, key.as_slice(), native])?;
        if removed != 1 { return Err(Error::InvalidData); }
        finish_cause(tx, b, receipt, false)?;
    }
    Ok(())
}
pub(super) fn nonblocking(tx: &Transaction<'_>, b: BindingId, inbox: i64, e: &InboxEntry) -> Result<(), Error> {
    if matches!(e.reason, InboxReason::Generation | InboxReason::Reference | InboxReason::Structure) {
        incoming_done(tx, b, inbox, e)?;
    }
    Ok(())
}
