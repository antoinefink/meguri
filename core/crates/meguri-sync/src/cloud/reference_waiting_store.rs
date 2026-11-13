//! P14.4/V11 causal arrival stamps. A live drain advances one small watermark;
//! classification may follow it, but a later version cannot inherit its proof.
use super::{store::{decode, encode}, *};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::collections::BTreeSet;

const SCHEMA: &str = "CREATE TABLE cloud_reference_schema(
 version INTEGER PRIMARY KEY CHECK(version=1),
 serial INTEGER NOT NULL CHECK(typeof(serial)='integer' AND serial>=0)
)";
const WAITING: &str = "CREATE TABLE cloud_reference_waiting(
 binding BLOB NOT NULL REFERENCES bindings(id),
 inbox INTEGER PRIMARY KEY REFERENCES inbox(id) ON DELETE CASCADE CHECK(inbox>0),
 arrival INTEGER NOT NULL CHECK(typeof(arrival)='integer' AND arrival>=0),
 identity BLOB CHECK(identity IS NULL OR (typeof(identity)='blob' AND length(identity)=32))
)";
const DRAINS: &str = "CREATE TABLE cloud_reference_drains(
 binding BLOB PRIMARY KEY NOT NULL REFERENCES bindings(id),
 through INTEGER NOT NULL CHECK(typeof(through)='integer' AND through>=0)
)";
const INDEX: &str = "CREATE INDEX cloud_reference_binding ON cloud_reference_waiting(binding)";
const DELETE: &str = "CREATE TRIGGER cloud_reference_deleted AFTER DELETE ON inbox BEGIN
 DELETE FROM cloud_reference_waiting WHERE inbox=OLD.id;
END";
const INSERT: &str = "CREATE TRIGGER cloud_reference_arrived AFTER INSERT ON inbox
 WHEN EXISTS(SELECT 1 FROM bindings WHERE id=NEW.binding AND salvage=0) BEGIN
 UPDATE cloud_reference_schema SET serial=serial+1 WHERE version=1;
 INSERT INTO cloud_reference_waiting(binding,inbox,arrival)
 SELECT NEW.binding,NEW.id,serial FROM cloud_reference_schema WHERE version=1;
END";
const UPDATE: &str = "CREATE TRIGGER cloud_reference_changed AFTER UPDATE ON inbox
 WHEN NEW.id!=OLD.id OR NEW.binding!=OLD.binding
 OR json_type(NEW.entry,'$.value') IS NOT json_type(OLD.entry,'$.value')
 OR json_extract(NEW.entry,'$.record_type','$.record_name','$.value.Record.envelope',
     '$.value.Record.erev','$.value.Record.system_fields') IS NOT
    json_extract(OLD.entry,'$.record_type','$.record_name','$.value.Record.envelope',
     '$.value.Record.erev','$.value.Record.system_fields') BEGIN
 DELETE FROM cloud_reference_waiting WHERE inbox=OLD.id;
 UPDATE cloud_reference_schema SET serial=serial+1 WHERE version=1;
 INSERT INTO cloud_reference_waiting(binding,inbox,arrival)
 SELECT NEW.binding,NEW.id,serial FROM cloud_reference_schema
 WHERE version=1 AND EXISTS(SELECT 1 FROM bindings WHERE id=NEW.binding AND salvage=0);
END";
const OBJECTS: &[(&str, &str, &str, &str)] = &[
    ("cloud_reference_schema", "table", "cloud_reference_schema", SCHEMA),
    ("cloud_reference_waiting", "table", "cloud_reference_waiting", WAITING),
    ("cloud_reference_drains", "table", "cloud_reference_drains", DRAINS),
    ("cloud_reference_binding", "index", "cloud_reference_waiting", INDEX),
    ("cloud_reference_deleted", "trigger", "inbox", DELETE),
    ("cloud_reference_arrived", "trigger", "inbox", INSERT),
    ("cloud_reference_changed", "trigger", "inbox", UPDATE),
];

/// Bounded layout check used by private admission and runtime readback. The
/// unshipped extension has one exact physical layout, including keys, checks,
/// references and expiry triggers. Never repair damaged evidence in place.
pub(super) fn check_version(c: &Connection) -> Result<bool, Error> {
    let kind: Option<String> = c.query_row(
        "SELECT type FROM sqlite_master WHERE name='cloud_reference_schema'", [], |r| r.get(0),
    ).optional()?;
    if kind.is_none() {
        for &(name, _, _, _) in &OBJECTS[1..] {
            if c.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name=?1)", [name], |r| r.get::<_, bool>(0))? {
                return Err(Error::InvalidData);
            }
        }
        return Ok(false);
    }
    if kind.as_deref() != Some("table") { return Err(Error::InvalidData); }
    let versions = c.prepare("SELECT version FROM cloud_reference_schema LIMIT 2")?
        .query_map([], |r| r.get::<_, i64>(0))?.collect::<Result<Vec<_>, _>>()?;
    if versions != [1] { return Err(Error::RuntimeSchemaUnsupported); }
    for &(name, kind, table, sql) in OBJECTS {
        let actual: Option<(String, String, String)> = c.query_row(
            "SELECT type,tbl_name,sql FROM sqlite_master WHERE name=?1", [name],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).optional()?;
        if actual.as_ref().map(|(k, t, s)| (k.as_str(), t.as_str(), s.as_str())) != Some((kind, table, sql)) {
            return Err(Error::InvalidData);
        }
    }
    Ok(true)
}

/// Full content admission belongs to the private open probe, not the marker
/// check used by routine runtime readback (including synchronous drains).
pub(super) fn validate(c: &Connection) -> Result<(), Error> {
    if !check_version(c)? { return Ok(()); }
    // All evidence contents and ownership are checked here, before source I/O.
    // NULL identity is an explicitly unenriched historical-writer arrival; its
    // trigger stamp still requires later live work, never saved drain history.
    let invalid: bool = c.query_row("SELECT
        EXISTS(SELECT 1 FROM cloud_reference_schema WHERE typeof(serial)!='integer' OR serial<0)
        OR EXISTS(SELECT 1 FROM cloud_reference_waiting r
            LEFT JOIN inbox i ON i.id=r.inbox LEFT JOIN bindings b ON b.id=r.binding
            WHERE b.id IS NULL OR b.salvage!=0 OR i.id IS NULL OR i.binding!=r.binding
            OR typeof(r.inbox)!='integer' OR r.inbox<=0 OR typeof(r.arrival)!='integer'
            OR r.arrival<0 OR r.arrival>(SELECT serial FROM cloud_reference_schema)
            OR (r.identity IS NOT NULL AND (typeof(r.identity)!='blob' OR length(r.identity)!=32)))
        OR EXISTS(SELECT 1 FROM inbox i JOIN bindings b ON b.id=i.binding
            LEFT JOIN cloud_reference_waiting r ON r.inbox=i.id
            WHERE b.salvage=0 AND r.inbox IS NULL)
        OR EXISTS(SELECT 1 FROM cloud_reference_drains d LEFT JOIN bindings b ON b.id=d.binding
            WHERE b.id IS NULL OR b.salvage!=0 OR typeof(d.through)!='integer'
            OR d.through<0 OR d.through>(SELECT serial FROM cloud_reference_schema))",
        [], |r| r.get(0))?;
    if invalid { return Err(Error::InvalidData); }
    // Stream one entry at a time on the PRIVATE probe, never at a drain. Do not
    // decode entity payloads or another build's runtime representation.
    let mut q = c.prepare("SELECT r.binding,r.inbox,r.arrival,r.identity,i.entry
        FROM cloud_reference_waiting r JOIN inbox i ON i.id=r.inbox WHERE r.identity IS NOT NULL")?;
    let mut rows = q.query([])?;
    while let Some(row) = rows.next()? {
        let b = store::binding_id(row.get(0)?)?;
        let e: InboxEntry = decode(&row.get::<_, String>(4)?)?;
        let digest: Vec<u8> = row.get(3)?;
        if digest.as_slice() != identity(b, row.get(1)?, row.get(2)?, &e)?.as_slice() {
            return Err(Error::InvalidData);
        }
    }
    Ok(())
}

// Match the trigger's Service fields; Some/None distinguishes record/deletion.
// Ownership and ordinal also bind the digest to this particular arrival.
fn identity(b: BindingId, id: i64, arrival: i64, e: &InboxEntry) -> Result<[u8; 32], Error> {
    let service = match &e.value {
        InboxValue::Record { envelope, erev, system_fields, .. } => Some((envelope, erev, system_fields)),
        InboxValue::Deletion => None,
    };
    Ok(entity_codec::sha256(encode(&(b, id, arrival, &e.record_type, &e.record_name,
        service))?.as_bytes()))
}

/// Enrich an actually accepted write in its original transaction. Old writers
/// retain their trigger stamps without inventing body identities for old data.
pub(super) fn identify(tx: &Transaction<'_>, b: BindingId, id: i64, e: &InboxEntry) -> Result<(), Error> {
    let present: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='cloud_reference_schema')", [], |r| r.get(0))?;
    if !present { return Ok(()); }
    let arrival: i64 = tx.query_row("SELECT arrival FROM cloud_reference_waiting WHERE binding=?1 AND inbox=?2",
        params![b.0.as_slice(), id], |r| r.get(0))?;
    tx.execute("UPDATE cloud_reference_waiting SET identity=?3 WHERE binding=?1 AND inbox=?2",
        params![b.0.as_slice(), id, identity(b, id, arrival, e)?.as_slice()])?;
    Ok(())
}

/// Captured under State + tracking ownership before calling any provider. No
/// inbox scan/body read: arrivals during that gap have strictly larger stamps.
pub(super) fn arrival_point(c: &Connection) -> Result<i64, Error> {
    Ok(c.query_row("SELECT serial FROM cloud_reference_schema WHERE version=1", [], |r| r.get(0))?)
}

pub(super) fn drained(tx: &Transaction<'_>, b: BindingId, through: i64) -> Result<(), Error> {
    if through < 0 || through > arrival_point(tx)? { return Err(Error::InvalidData); }
    tx.execute("INSERT INTO cloud_reference_drains(binding,through) VALUES(?1,?2)
        ON CONFLICT(binding) DO UPDATE SET through=max(through,excluded.through)",
        params![b.0.as_slice(), through])?;
    Ok(())
}

impl TrackingStore {
    pub(super) fn install_reference_waiting(&mut self) -> Result<(), Error> {
        if !check_version(&self.connection)? {
            let tx = self.connection.transaction()?;
            for &(_, _, _, sql) in OBJECTS { tx.execute_batch(sql)?; }
            tx.execute("INSERT INTO cloud_reference_schema VALUES(1,0)", [])?;
            // Existing rows start at zero with no accepted collection cutoff.
            // Only a subsequent live drain can cover this enrollment.
            tx.execute("INSERT INTO cloud_reference_waiting(binding,inbox,arrival)
                SELECT i.binding,i.id,0 FROM inbox i JOIN bindings b ON b.id=i.binding WHERE b.salvage=0", [])?;
            tx.commit()?;
        }
        Ok(())
    }
    pub(super) fn reference_arrival_point(&self) -> Result<i64, Error> {
        arrival_point(&self.connection)
    }
    pub(super) fn reference_waiting(&self, b: BindingId) -> Result<BTreeSet<i64>, Error> {
        self.binding(b)?.ok_or(Error::BindingNotFound)?;
        let mut q = self.connection.prepare("SELECT r.inbox FROM cloud_reference_waiting r
            JOIN cloud_reference_drains d ON d.binding=r.binding
            WHERE r.binding=?1 AND r.arrival<=d.through")?;
        Ok(q.query_map([b.0.as_slice()], |r| r.get(0))?.collect::<Result<_, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn installed() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE bindings(id BLOB PRIMARY KEY,salvage INTEGER);
            CREATE TABLE inbox(id INTEGER PRIMARY KEY,binding BLOB,entry TEXT);").unwrap();
        for &(_, _, _, sql) in OBJECTS { c.execute_batch(sql).unwrap(); }
        c.execute("INSERT INTO cloud_reference_schema VALUES(1,0)", []).unwrap();
        c
    }
    #[test]
    fn reference_waiting_schema_admission_refuses_partial_or_future_extension() {
        for &(name, kind, _, _) in OBJECTS {
            let c = installed();
            assert!(check_version(&c).unwrap());
            c.execute_batch(&format!("DROP {kind} {name}")).unwrap();
            assert!(check_version(&c).is_err(), "{name}");
        }
        let c = installed();
        c.execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE cloud_reference_schema SET version=2").unwrap();
        assert!(matches!(check_version(&c), Err(Error::RuntimeSchemaUnsupported)));
    }
    #[test]
    fn reference_waiting_schema_admission_refuses_missing_key_and_changed_expiry() {
        let c = installed();
        c.execute_batch("DROP TABLE cloud_reference_waiting").unwrap();
        c.execute_batch(&WAITING.replacen("inbox INTEGER PRIMARY KEY", "inbox INTEGER", 1)).unwrap();
        c.execute_batch(INDEX).unwrap();
        assert!(matches!(check_version(&c), Err(Error::InvalidData)));
        let c = installed();
        c.execute_batch("DROP TRIGGER cloud_reference_changed;
            CREATE TRIGGER cloud_reference_changed AFTER UPDATE ON inbox BEGIN SELECT 1; END").unwrap();
        assert!(matches!(check_version(&c), Err(Error::InvalidData)));
    }
}
