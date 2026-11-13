//! P3/K10.3 root attention, separate from historical qualified crt knowledge.
//! An absent row is unknown. Only owned control intake evaluates this ledger;
//! drains, cursor writes, local crt and exchange success never do.
use super::{control_codec::Active, store::decode, *};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

pub(super) const SCHEMA: &str = "
CREATE TABLE cloud_root_readability(
 binding BLOB PRIMARY KEY REFERENCES bindings(id),
 epoch_knowledge TEXT NOT NULL CHECK(epoch_knowledge IN ('unknown','complete','incomplete')),
 status TEXT NOT NULL CHECK(status IN ('unknown','readable','unreadable')),
 active_system_fields BLOB,
 CHECK((status='unreadable' AND typeof(active_system_fields)='blob' AND length(active_system_fields)>0)
    OR (status!='unreadable' AND active_system_fields IS NULL)),
 CHECK(status!='readable' OR epoch_knowledge='complete')
);";

pub(super) fn validate(c: &Connection) -> Result<(), Error> {
    c.prepare("SELECT binding,epoch_knowledge,status,active_system_fields FROM cloud_root_readability LIMIT 0")?;
    // Validate metadata/owners without opening another build's control payloads.
    let invalid: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM cloud_root_readability r LEFT JOIN bindings b ON b.id=r.binding
         WHERE b.id IS NULL OR b.salvage!=0 OR typeof(r.epoch_knowledge)!='text'
         OR r.epoch_knowledge NOT IN ('unknown','complete','incomplete') OR typeof(r.status)!='text' OR r.status NOT IN ('unknown','readable','unreadable')
         OR (r.status='readable' AND r.epoch_knowledge!='complete')
         OR (r.status='unreadable' AND (typeof(r.active_system_fields)!='blob' OR length(r.active_system_fields)=0))
         OR (r.status!='unreadable' AND r.active_system_fields IS NOT NULL))",
        [], |r| r.get(0),
    )?;
    if invalid { return Err(Error::InvalidData); }
    Ok(())
}

/// A current complete body qualifies independently of a retained crt. A bad,
/// publishing, missing or unsupported body revokes only this readability proof.
pub(super) fn epoch_observation(binding: &Binding, records: &[workflow::Saved], deleted: &[String]) -> Result<Option<bool>, Error> {
    let mut epochs = records.iter().filter(|r| r.name == "epoch");
    let epoch = epochs.next();
    if epochs.next().is_some() { return Err(Error::InvalidData); }
    if deleted.iter().any(|n| n == "epoch") { return Ok(Some(false)); }
    Ok(epoch.map(|_| super::epoch_crt_store::fetched(binding, records).is_some()))
}

/// Called only inside the transaction recording actual accepted Active/Epoch
/// intake. `active` is the last applied Service version, never a local proof.
/// A missing Active leaves unknown knowledge (or an existing mark) intact.
pub(super) fn observe(
    tx: &Transaction<'_>, binding: BindingId, active: Option<&workflow::Saved>, epoch: Option<bool>,
) -> Result<(), Error> {
    let facts: String = tx.query_row("SELECT facts FROM bindings WHERE id=?1 AND salvage=0", [binding.0.as_slice()], |r| r.get(0))?;
    let facts: BindingFacts = decode(&facts)?;
    let previous: Option<(String, String, Option<Vec<u8>>)> = tx.query_row(
        "SELECT epoch_knowledge,status,active_system_fields FROM cloud_root_readability WHERE binding=?1",
        [binding.0.as_slice()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ).optional()?;
    let (knowledge, mut status, mut mark) = previous.unwrap_or(("unknown".into(), "unknown".into(), None));
    let knowledge = match epoch {
        Some(true) => "complete",
        Some(false) => "incomplete",
        None => knowledge.as_str(),
    };
    if !matches!(knowledge, "unknown" | "complete" | "incomplete") { return Err(Error::InvalidData); }
    if let Some(active) = active {
        if active.name != "active" || active.system_fields.is_empty() { return Err(Error::InvalidData); }
        match Active::decode(&active.envelope) {
            Ok(a) if a.epoch != facts.epoch.0 => {
                // EpochSwitched has its own isolation producer. Bound-Epoch
                // knowledge says nothing about the competing epoch's body.
                // It cannot clear an earlier root-unreadable obligation.
                if mark.is_none() { status = "unknown".into(); }
            }
            Ok(_) if knowledge == "complete" => {
                status = "readable".into();
                mark = None;
            }
            Ok(_) if knowledge == "unknown" => {
                // Migration supplies no Epoch observation. A matching Active
                // alone neither establishes readability nor changes attention.
            }
            _ => {
                status = "unreadable".into();
                mark = Some(active.system_fields.clone());
            }
        }
    } else if status == "readable" && knowledge != "complete" {
        // A readable ledger always has a retained Active. Refuse corruption
        // rather than forget its conditional replacement version.
        return Err(Error::InvalidData);
    }
    tx.execute(
        "INSERT INTO cloud_root_readability(binding,epoch_knowledge,status,active_system_fields) VALUES(?1,?2,?3,?4)
         ON CONFLICT(binding) DO UPDATE SET epoch_knowledge=excluded.epoch_knowledge,status=excluded.status,active_system_fields=excluded.active_system_fields",
        params![binding.0.as_slice(), knowledge, status, mark],
    )?;
    Ok(())
}

impl TrackingStore {
    pub(super) fn root_unreadable(&self, binding: BindingId) -> Result<Option<OpaqueBytes>, Error> {
        self.binding(binding)?.ok_or(Error::BindingNotFound)?;
        let row: Option<(String, Option<Vec<u8>>)> = self.connection.query_row(
            "SELECT status,active_system_fields FROM cloud_root_readability WHERE binding=?1",
            [binding.0.as_slice()], |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional()?;
        match row {
            None => Ok(None), // unknown, never proof of clean status
            Some((status, Some(fields))) if status == "unreadable" && !fields.is_empty() => Ok(Some(OpaqueBytes(fields))),
            Some((status, None)) if status == "unknown" || status == "readable" => Ok(None),
            _ => Err(Error::InvalidData),
        }
    }
    #[cfg(feature = "test-hooks")]
    pub fn root_unreadable_for_testing(&self, binding: BindingId) -> Result<Option<OpaqueBytes>, Error> {
        self.root_unreadable(binding)
    }
}
