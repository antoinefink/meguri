// SPDX-License-Identifier: MIT
//! Startup checks are read-only and precede any provisioning resumption.
use super::{DataRoot, root::unavailable};
use crate::{Result, crypto};
use rusqlite::{Connection, OptionalExtension, Row};

struct AccountIdentity {
    id: String,
    username: String,
    role: String,
}
impl AccountIdentity {
    fn decode(row: &Row<'_>) -> Result<Self> {
        let identity = Self {
            id: row.get("id")?,
            username: row.get("username")?,
            role: row.get("role")?,
        };
        require(
            crypto::bearer(&identity.id).is_ok()
                && crypto::username(&identity.username).is_ok_and(|n| n == identity.username)
                && matches!(identity.role.as_str(), "admin" | "member"),
        )?;
        Ok(identity)
    }
}

enum AccountLifecycle {
    Provisioning,
    Ready,
}
impl AccountLifecycle {
    fn decode(row: &Row<'_>) -> Result<Self> {
        let state: String = row.get("provisioning_state")?;
        // Read the persisted integer: bool conversion would hide invalid values.
        let enabled: i64 = row.get("enabled")?;
        require(matches!(enabled, 0 | 1))?;
        match state.as_str() {
            "provisioning" => Ok(Self::Provisioning),
            "ready" => Ok(Self::Ready),
            _ => Err(unavailable()),
        }
    }
}

struct CredentialMetadata {
    phc: String,
    revision: i64,
    generation: i64,
}
impl CredentialMetadata {
    fn decode(row: &Row<'_>) -> Result<Self> {
        let credentials = Self {
            phc: row.get("password_phc")?,
            revision: row.get("password_revision")?,
            generation: row.get("generation")?,
        };
        require(
            credentials.revision > 0
                && credentials.generation > 0
                && crypto::valid_phc_shape(&credentials.phc),
        )?;
        Ok(credentials)
    }
}

fn require(value: bool) -> Result<()> {
    if value { Ok(()) } else { Err(unavailable()) }
}
fn count(c: &Connection, sql: &str) -> Result<i64> {
    Ok(c.query_row(sql, [], |r| r.get(0))?)
}
pub(super) fn validate(root: &DataRoot, c: &Connection, allow_incomplete: bool) -> Result<()> {
    require(count(c, "SELECT count(*) FROM setup")? == 1)?;
    require(
        count(
            c,
            "SELECT count(*) FROM service_state WHERE singleton=1 AND admission_open IN (0,1)",
        )? == 1,
    )?;
    require(count(c, "SELECT count(*) FROM service_state")? == 1)?;
    let (phase, hash): (String, Vec<u8>) = c.query_row(
        "SELECT phase,capability FROM setup WHERE singleton=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let (kind, state, reserved): (String, String, Option<String>) = c.query_row(
        "SELECT kind,state,reserved_id FROM capabilities WHERE hash=?1",
        [&hash],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    require(kind == "setup")?;
    match phase.as_str() {
        "incomplete" if allow_incomplete => {
            require(
                state == "available"
                    && reserved.is_none()
                    && count(c, "SELECT count(*) FROM accounts")? == 0,
            )?;
        }
        "open" => {
            require(matches!(state.as_str(), "available" | "provisioning"))?;
            require(
                count(
                    c,
                    "SELECT count(*) FROM accounts WHERE role='admin' AND provisioning_state='ready'",
                )? == 0,
            )?;
            require(
                count(c, "SELECT count(*) FROM accounts")?
                    == if reserved.is_some() { 1 } else { 0 },
            )?;
        }
        "closed" => {
            require(state == "consumed" && reserved.is_some())?;
            let first: bool = c.query_row(
                "SELECT role='admin' AND provisioning_state='ready' FROM accounts WHERE id=?1",
                [reserved.as_deref().ok_or_else(unavailable)?],
                |r| r.get(0),
            )?;
            // Disabling/recovering this administrator must not reopen setup.
            require(first)?;
        }
        _ => return Err(unavailable()),
    }
    // Other setup capabilities may only be the retained, unclaimed rotated ones.
    require(!c.prepare("SELECT 1 FROM capabilities WHERE kind='setup' AND hash<>(SELECT capability FROM setup) AND (state<>'revoked' OR reserved_id IS NOT NULL)")?.exists([])?)?;

    let mut accounts = c.prepare("SELECT id,username,role,provisioning_state,enabled,password_phc,password_revision,generation FROM accounts")?;
    let mut rows = accounts.query([])?;
    while let Some(r) = rows.next()? {
        let identity = AccountIdentity::decode(r)?;
        let lifecycle = AccountLifecycle::decode(r)?;
        let credentials = CredentialMetadata::decode(r)?;
        let claim: Option<(String,String,Vec<u8>,String,String)> = c.query_row(
            "SELECT p.username,p.role,p.credential_proof,k.kind,k.state FROM provisioning_reservations p JOIN capabilities k ON k.hash=p.capability WHERE p.account_id=?1 AND k.reserved_id=p.account_id",
            [&identity.id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
        let (name, claim_role, proof, kind, cap_state) = claim.ok_or_else(unavailable)?;
        // The reservation records original provisioning authority, not a
        // lifetime role restriction on ready/restored administrator metadata.
        // There is no runtime role mutation or remote promotion endpoint.
        require(
            name == identity.username
                && proof.len() == 32
                && ((claim_role == "admin" && kind == "setup")
                    || (claim_role == "member" && kind == "invite")),
        )?;
        match lifecycle {
            AccountLifecycle::Provisioning => {
                require(
                    claim_role == identity.role
                        && matches!(cap_state.as_str(), "provisioning" | "revoked")
                        && crypto::same_digest(&proof, &crypto::digest(credentials.phc.as_bytes()))
                        && credentials.revision == 1,
                )?;
            }
            AccountLifecycle::Ready => {
                require(cap_state == "consumed")?;
                if credentials.revision == 1 {
                    require(crypto::same_digest(
                        &proof,
                        &crypto::digest(credentials.phc.as_bytes()),
                    ))?;
                }
                root.validate_stores(&identity.id)?;
            }
        }
    }
    // No orphan or repointed proof, including proofs retained after publication.
    require(!c.prepare("SELECT 1 FROM provisioning_reservations p JOIN capabilities k ON k.hash=p.capability WHERE k.reserved_id IS NULL OR k.reserved_id<>p.account_id OR k.kind='reset'")?.exists([])?)?;
    require(!c.prepare("SELECT 1 FROM capabilities k WHERE k.reserved_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM provisioning_reservations p WHERE p.capability=k.hash AND p.account_id=k.reserved_id)")?.exists([])?)?;

    // Generations are historical references, never future or nonpositive ones.
    // Expired/revoked/stale-authority reservations are valid blocked state.
    require(!c.prepare("SELECT 1 FROM capabilities k LEFT JOIN accounts i ON i.id=k.issuer LEFT JOIN accounts t ON t.id=k.target WHERE length(k.hash)<>32 OR (k.kind='setup' AND (k.issuer IS NOT NULL OR k.issuer_generation IS NOT NULL OR k.target IS NOT NULL OR k.target_generation IS NOT NULL OR k.expires_at IS NOT NULL)) OR (k.kind<>'setup' AND (i.role<>'admin' OR i.provisioning_state<>'ready' OR k.issuer_generation IS NULL OR k.issuer_generation<=0 OR k.issuer_generation>i.generation OR k.expires_at IS NULL)) OR (k.kind='invite' AND (k.target IS NOT NULL OR k.target_generation IS NOT NULL)) OR (k.kind='reset' AND (k.target IS NULL OR k.target_generation IS NULL OR k.target_generation<=0 OR k.target_generation>t.generation OR t.provisioning_state<>'ready' OR k.reserved_id IS NOT NULL OR k.state='provisioning')) OR (k.state='available' AND k.reserved_id IS NOT NULL) OR (k.state='provisioning' AND k.reserved_id IS NULL) OR (k.kind IN ('setup','invite') AND k.state='consumed' AND k.reserved_id IS NULL)")?.exists([])?)?;
    require(!c.prepare("SELECT 1 FROM credentials k JOIN accounts a ON a.id=k.account_id WHERE k.generation<=0 OR k.generation>a.generation OR a.provisioning_state<>'ready' OR (k.revoked=0 AND (k.generation<>a.generation OR a.enabled<>1))")?.exists([])?)?;
    let version:i64=c.pragma_query_value(None,"user_version",|r|r.get(0))?;
    if version==2 {crate::media::recovery::validate(root,c)?;}
    Ok(())
}
