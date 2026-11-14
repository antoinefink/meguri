//! Accepted collection images and payload-independent pending revisions.
use super::{entity_codec as c, row_payload::RowFailure, runtime::{NativeFileIdentity, StoredRuntimeState, DirtySource}, store::{decode, encode}, *};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(any(test, feature = "test-hooks"))]
thread_local! { static READ_WORK: std::cell::Cell<(u64,u64,u64,u64)> = const { std::cell::Cell::new((0,0,0,0)) }; }
#[cfg(any(test, feature = "test-hooks"))]
pub(super) fn take_read_work() -> (u64,u64,u64,u64) { READ_WORK.with(|m| m.replace((0,0,0,0))) }
#[cfg(any(test, feature = "test-hooks"))]
fn measured(q: &rusqlite::Statement<'_>) {
    READ_WORK.with(|m| { let (queries,steps,owners,semantic) = m.get(); m.set((queries+1,steps+q.get_status(rusqlite::StatementStatus::VmStep) as u64,owners,semantic)); });
}

#[cfg(any(test, feature = "test-hooks"))]
pub(super) fn owner_read() {
    READ_WORK.with(|m| { let (q,s,n,r) = m.get(); m.set((q,s,n+1,r)); });
}

// A selected read checks its accepted coverage without decoding other rows.
pub(super) fn validate_selected(cn: &Connection, b: BindingId, state: Option<&StoredRuntimeState>) -> Result<(), Error> {
    let mut q = cn.prepare("SELECT content FROM cloud_row_classification_coverage WHERE binding=?1")?;
    let text: Option<String> = q.query_row([b.0.as_slice()], |r| r.get(0)).optional()?;
    #[cfg(any(test, feature = "test-hooks"))]
    measured(&q);
    if let Some(text) = text {
        coverage_matches(state.ok_or(Error::InvalidData)?, &decode(&text)?)?;
    }
    Ok(())
}

// Unpublished V8 layout. The runtime marker is its downgrade fence;
// actual historical-reader execution remains integration evidence.
pub(super) const SCHEMA: &str = "
CREATE TABLE cloud_row_classifications(
 binding BLOB NOT NULL REFERENCES bindings(id), kind INTEGER NOT NULL, key BLOB NOT NULL,
 name TEXT NOT NULL, failure INTEGER NOT NULL CHECK(failure BETWEEN 0 AND 2),
 local_rev INTEGER NOT NULL CHECK(local_rev >= 0), change_seq INTEGER NOT NULL CHECK(change_seq >= 0),
 content TEXT NOT NULL CHECK(json_valid(content)), PRIMARY KEY(binding,kind,key)
);
CREATE UNIQUE INDEX cloud_row_names ON cloud_row_classifications(binding,name);
CREATE INDEX cloud_row_failures ON cloud_row_classifications(binding,failure,name) WHERE failure!=0;
CREATE TABLE cloud_row_classification_coverage(
 binding BLOB PRIMARY KEY REFERENCES bindings(id), content TEXT NOT NULL CHECK(json_valid(content))
);";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RowClassification {
    pub kind: c::Kind,
    pub key: Vec<u8>,
    pub open: u64,
    pub generation: u64,
    pub seq: u64,
    pub file: NativeFileIdentity,
    pub raw_digest: Option<[u8; 32]>,
    pub logical_digest: Option<[u8; 32]>,
    pub failure: Option<RowFailure>,
}
impl RowClassification {
    fn validate(&self) -> Result<(), Error> {
        #[cfg(any(test, feature = "test-hooks"))]
        READ_WORK.with(|m| { let (q,s,o,n) = m.get(); m.set((q,s,o,n+1)); });
        for n in [self.open, self.generation, self.seq] { super::runtime_store::checked(n)?; }
        if self.open == 0 || self.generation == 0 || self.file.created_nanos >= 1_000_000_000
            || self.kind == c::Kind::MediaName || self.raw_digest.is_some() != self.logical_digest.is_some()
            || (self.failure.is_some() && self.raw_digest.is_none())
        { return Err(Error::InvalidData); }
        let k = super::workflow_store::row_key(self.kind, &self.key)?;
        if super::row_payload::tracking_key(self.kind, &k)? != self.key { return Err(Error::InvalidData); }
        if let Some(RowFailure::Unreadable { table, column }) = self.failure {
            let head = super::row_payload::table(self.kind).ok_or(Error::InvalidData)?;
            let allowed = [head, meguri_storage::Table::Fields, meguri_storage::Table::Templates];
            if !allowed.into_iter().take(if self.kind == c::Kind::Notetype { 3 } else { 1 })
                .any(|t| t.name() == table && t.columns().contains(&column))
            { return Err(Error::InvalidData); }
        }
        Ok(())
    }
    fn code(&self) -> i64 {
        match self.failure { None => 0, Some(RowFailure::Oversize) => 1, Some(RowFailure::Unreadable { .. }) => 2 }
    }
}
fn unsigned(n: i64) -> Result<u64, Error> { u64::try_from(n).map_err(|_| Error::InvalidData) }
fn coverage_matches(s: &StoredRuntimeState, ledger: &workflow::CoverageLedger) -> Result<(), Error> {
    ledger.validate()?;
    if ledger.open == 0 || ledger.open > s.open_counter || s.coverage.as_ref() != Some(ledger)
        || s.file != Some(ledger.cursor.native_file)
    { return Err(Error::InvalidData); }
    Ok(())
}

pub(super) fn validate_shape(cn: &Connection) -> Result<(), Error> {
    cn.prepare("SELECT binding,kind,key,name,failure,local_rev,change_seq,content FROM cloud_row_classifications LIMIT 0")?;
    cn.prepare("SELECT binding,content FROM cloud_row_classification_coverage LIMIT 0")?;
    let indices: i64 = cn.query_row("SELECT count(*) FROM sqlite_master WHERE type='index' AND name IN ('cloud_row_names','cloud_row_failures')", [], |r| r.get(0))?;
    if indices != 2 { return Err(Error::InvalidData); }
    Ok(())
}

pub(super) fn validate(cn: &Connection, scope: &BuildScope) -> Result<(), Error> {
    validate_shape(cn)?;
    // Decode each owner's accepted state once, rather than once per row.
    let mut owners = BTreeMap::new();
    let mut q = cn.prepare("SELECT b.id,b.facts,r.content FROM bindings b JOIN cloud_runtime_bindings r ON r.binding=b.id WHERE b.salvage=0 AND b.container=?1 AND b.environment=?2")?;
    let mut rows = q.query(params![scope.container.as_str(), scope.environment.code()])?;
    while let Some(row) = rows.next()? {
        #[cfg(any(test, feature = "test-hooks"))]
        owner_read();
        let state: StoredRuntimeState = decode(&row.get::<_, String>(2)?)?;
        state.validate()?;
        let facts: BindingFacts = decode(&row.get::<_, String>(1)?)?;
        owners.insert(row.get::<_, Vec<u8>>(0)?, (facts.record_key.0, state));
    }
    let mut q = cn.prepare("SELECT r.binding,r.kind,r.key,r.name,r.failure,r.local_rev,r.change_seq,r.content FROM cloud_row_classifications r JOIN bindings b ON b.id=r.binding WHERE b.salvage=0 AND b.container=?1 AND b.environment=?2")?;
    let mut rows = q.query(params![scope.container.as_str(), scope.environment.code()])?;
    while let Some(row) = rows.next()? {
        let (record_key, state) = owners.get(&row.get::<_, Vec<u8>>(0)?).ok_or(Error::InvalidData)?;
        let f: RowClassification = decode(&row.get::<_, String>(7)?)?;
        f.validate()?;
        let key: [u8; 32] = record_key.as_slice().try_into().map_err(|_| Error::InvalidData)?;
        let rev = unsigned(row.get(5)?)?;
        let seq = unsigned(row.get(6)?)?;
        if f.open > state.open_counter || (f.open == state.open_counter && state.file != Some(f.file))
            || f.kind as i64 != row.get::<_, i64>(1)? || f.key != row.get::<_, Vec<u8>>(2)?
            || c::entity_name(&key, f.kind, &f.key).map_err(|_| Error::InvalidData)? != row.get::<_, String>(3)?
            || f.code() != row.get::<_, i64>(4)? || seq > state.change_counter
            || (f.failure.is_some() && (rev == 0 || seq == 0))
        { return Err(Error::InvalidData); }
    }
    let mut q = cn.prepare("SELECT r.binding,r.content FROM cloud_row_classification_coverage r JOIN bindings b ON b.id=r.binding WHERE b.salvage=0 AND b.container=?1 AND b.environment=?2")?;
    let mut rows = q.query(params![scope.container.as_str(), scope.environment.code()])?;
    while let Some(row) = rows.next()? {
        let (_, state) = owners.get(&row.get::<_, Vec<u8>>(0)?).ok_or(Error::InvalidData)?;
        coverage_matches(state, &decode(&row.get::<_, String>(1)?)?)?;
    }
    Ok(())
}

pub(super) fn get(cn: &Connection, b: BindingId, kind: c::Kind, key: &[u8]) -> Result<Option<RowClassification>, Error> {
    let text: Option<String> = cn.query_row("SELECT content FROM cloud_row_classifications WHERE binding=?1 AND kind=?2 AND key=?3", params![b.0.as_slice(), kind as i64, key], |r| r.get(0)).optional()?;
    text.map(|text| { let f: RowClassification = decode(&text)?; f.validate()?;
        if f.kind != kind || f.key != key { return Err(Error::InvalidData); } Ok(f)
    }).transpose()
}
pub(super) fn failed(cn: &Connection, b: BindingId, name: &str) -> Result<bool, Error> {
    let mut q = cn.prepare("SELECT EXISTS(SELECT 1 FROM cloud_row_classifications WHERE binding=?1 AND name=?2 AND failure!=0)")?;
    let value = q.query_row(params![b.0.as_slice(), name], |r| r.get(0))?;
    #[cfg(any(test, feature = "test-hooks"))] measured(&q);
    Ok(value)
}
pub(super) fn revision(cn: &Connection, b: BindingId, kind: c::Kind, key: &[u8]) -> Result<u64, Error> {
    let n: Option<i64> = cn.query_row("SELECT local_rev FROM cloud_row_classifications WHERE binding=?1 AND kind=?2 AND key=?3", params![b.0.as_slice(), kind as i64, key], |r| r.get(0)).optional()?;
    n.map(unsigned).transpose().map(|n| n.unwrap_or(0))
}
fn header(cn: &Connection, b: BindingId, kind: c::Kind, key: &[u8]) -> Result<Option<super::tracking_payload::Header>, Error> {
    let text: Option<String> = if kind == c::Kind::Note {
        let id = i64::from_be_bytes(key.try_into().map_err(|_| Error::InvalidData)?);
        cn.query_row("SELECT content FROM cloud_runtime_notes WHERE binding=?1 AND id=?2", params![b.0.as_slice(), id], |r| r.get(0)).optional()?
    } else {
        cn.query_row("SELECT content FROM cloud_runtime_entities WHERE binding=?1 AND kind=?2 AND key=?3", params![b.0.as_slice(), kind as i64, key], |r| r.get(0)).optional()?
    };
    text.map(|t| super::tracking_payload::header(kind, &t)).transpose()
}

// Caller owns the State/file guard and this transaction, and persists state
// with every accepted classification. No canonical image is created here.
pub(super) fn accept(tx: &Transaction<'_>, b: BindingId, f: &RowClassification,
    state: &mut StoredRuntimeState, record_key: &RecordKey, source: DirtySource) -> Result<bool, Error>
{
    f.validate()?;
    if state.open_counter != f.open || state.file != Some(f.file) { return Err(Error::InvalidData); }
    let old = get(tx, b, f.kind, &f.key)?;
    if let Some(old) = &old {
        if old.open > f.open { return Err(Error::InvalidData); }
        if old.open == f.open {
            if old.file != f.file || old.generation != f.generation { return Err(Error::InvalidData); }
            if old.seq > f.seq { return Ok(false); }
            if old.seq == f.seq && old != f { return Err(Error::InvalidData); }
        }
    }
    let name = c::entity_name(&record_key.0, f.kind, &f.key).map_err(|_| Error::InvalidData)?;
    let mut rev = revision(tx, b, f.kind, &f.key)?;
    let mut change: i64 = tx.query_row("SELECT change_seq FROM cloud_row_classifications WHERE binding=?1 AND kind=?2 AND key=?3", params![b.0.as_slice(), f.kind as i64, &f.key], |r| r.get(0)).optional()?.unwrap_or(0);
    let mut h = header(tx, b, f.kind, &f.key)?;
    if h.as_ref().is_some_and(|h| h.facts.observer_open == f.open
        && h.facts.observed_generation == f.generation && h.facts.observed_seq > f.seq)
    { return Ok(false); }
    if f.failure.is_some() {
        let changed = old.as_ref().is_none_or(|old| old.failure != f.failure || old.logical_digest != f.logical_digest);
        if changed {
            rev = super::runtime_store::next(rev.max(h.as_ref().map_or(0, |h| h.facts.local_rev)))?;
            state.change_counter = super::runtime_store::next(state.change_counter)?;
            change = super::runtime_store::checked(state.change_counter)?;
        }
        if let Some(h) = &mut h {
            h.facts.local_rev = rev.max(h.facts.local_rev);
            h.facts.observer_open = f.open;
            h.facts.observed_seq = f.seq;
            h.facts.observed_generation = f.generation;
            if changed || h.facts.mark.is_none() {
                h.facts.mark = Some(runtime::DirtyMark { source, change_seq: unsigned(change)?, payload_digest: h.local_digest() });
            }
            super::tracking_payload::put_header(tx, b, h)?;
        }
    }
    if f.raw_digest.is_none() && h.is_none() && rev != 0 {
        super::incomplete_store::dropped(tx, b, &name, rev)?;
    }
    tx.execute("INSERT INTO cloud_row_classifications(binding,kind,key,name,failure,local_rev,change_seq,content) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(binding,kind,key) DO UPDATE SET name=excluded.name,failure=excluded.failure,local_rev=excluded.local_rev,change_seq=excluded.change_seq,content=excluded.content", params![b.0.as_slice(), f.kind as i64, &f.key, name, f.code(), super::runtime_store::checked(rev)?, change, encode(f)?])?;
    Ok(true)
}
impl TrackingStore {
    pub(super) fn classify_row(&mut self, b: BindingId, f: &RowClassification) -> Result<bool, Error> {
        if f.failure.is_none() && f.raw_digest.is_some() { return Err(Error::InvalidData); }
        let binding = self.binding(b)?.ok_or(Error::BindingNotFound)?;
        let mut state = self.runtime_binding(b)?;
        let tx = self.connection.transaction()?;
        let accepted = accept(&tx, b, f, &mut state, &binding.facts.record_key, DirtySource::Snapshot)?;
        super::runtime_store::put_binding(&tx, b, &state)?;
        tx.commit()?;
        Ok(accepted)
    }
    pub(super) fn row_classification_page(&self, b: BindingId, after: Option<(c::Kind, &[u8])>) -> Result<Vec<RowClassification>, Error> {
        let sql = if after.is_some() {
            "SELECT content FROM cloud_row_classifications WHERE binding=?1 AND (kind,key)>(?2,?3) ORDER BY kind,key LIMIT 250"
        } else { "SELECT content FROM cloud_row_classifications WHERE binding=?1 ORDER BY kind,key LIMIT 250" };
        let mut q = self.connection.prepare(sql)?;
        let mut rows = if let Some((kind,key)) = after { q.query(params![b.0.as_slice(),kind as i64,key])? } else { q.query([b.0.as_slice()])? };
        let mut result = Vec::new();
        while let Some(row) = rows.next()? { let f: RowClassification = decode(&row.get::<_, String>(0)?)?; f.validate()?; result.push(f); }
        Ok(result)
    }
    pub(super) fn row_classifications(&self, b: BindingId, failures_only: bool) -> Result<Vec<RowClassification>, Error> {
        self.binding(b)?.ok_or(Error::BindingNotFound)?;
        if failures_only {
            let mut q = self.connection.prepare("SELECT content FROM cloud_row_classifications WHERE binding=?1 AND failure!=0 ORDER BY failure,name")?;
            let mut result = Vec::new();
            for text in q.query_map([b.0.as_slice()], |r| r.get::<_, String>(0))? { let f: RowClassification = decode(&text?)?; f.validate()?; result.push(f); }
            return Ok(result);
        }
        let mut result = Vec::new();
        loop {
            let page = self.row_classification_page(b, result.last().map(|f: &RowClassification| (f.kind, f.key.as_slice())))?;
            if page.is_empty() { break; }
            result.extend(page);
        }
        Ok(result)
    }
    pub(super) fn row_failed(&self, b: BindingId, name: &str) -> Result<bool, Error> { failed(&self.connection, b, name) }
    pub(super) fn row_failure_counts(&self, b: BindingId) -> Result<Vec<(i64, u64)>, Error> {
        let mut q = self.connection.prepare("SELECT failure,count(*) FROM cloud_row_classifications WHERE binding=?1 AND failure!=0 GROUP BY failure")?;
        let result = q.query_map([b.0.as_slice()], |r| Ok((r.get(0)?, r.get::<_, i64>(1)?)))?.map(|r| { let (kind,n) = r?; Ok((kind,unsigned(n)?)) }).collect();
        #[cfg(any(test, feature = "test-hooks"))] measured(&q);
        result
    }
    pub(super) fn row_failure_names(&self, b: BindingId) -> Result<BTreeSet<String>, Error> {
        let mut q = self.connection.prepare("SELECT name FROM cloud_row_classifications WHERE binding=?1 AND failure!=0")?;
        let result = q.query_map([b.0.as_slice()], |r| r.get(0))?.collect::<Result<_,_>>()?;
        #[cfg(any(test, feature = "test-hooks"))] measured(&q);
        Ok(result)
    }
    pub(super) fn row_failure_revision(&self, b: BindingId, name: &str) -> Result<Option<(u64,u64,u64,u64)>, Error> {
        let row: Option<(i64,String)> = self.connection.query_row("SELECT local_rev,content FROM cloud_row_classifications WHERE binding=?1 AND name=?2 AND failure!=0", params![b.0.as_slice(),name], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        row.map(|(rev,text)| { let f: RowClassification = decode(&text)?; f.validate()?; Ok((unsigned(rev)?,f.open,f.generation,f.seq)) }).transpose()
    }
    pub(super) fn row_classification_coverage(&self, b: BindingId) -> Result<bool, Error> {
        let s = self.runtime_binding(b)?;
        let text: Option<String> = self.connection.query_row("SELECT content FROM cloud_row_classification_coverage WHERE binding=?1", [b.0.as_slice()], |r| r.get(0)).optional()?;
        let Some(text) = text else { return Ok(false); };
        let ledger: workflow::CoverageLedger = decode(&text)?;
        coverage_matches(&s, &ledger)?;
        Ok(ledger.open == s.open_counter)
    }
}
