use super::{storage as sql, types::*};
use meguri_model::rng::RandomSource;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Synchronous requests on one connection; callers serialize service access.
pub struct Service<R> {
    connection: Connection,
    random: R,
}

// Logical refusals commit their log; infrastructure failures roll back.
enum Failure {
    Logical(ServiceError),
    Storage(StorageError),
}
impl From<ServiceError> for Failure {
    fn from(e: ServiceError) -> Self {
        Self::Logical(e)
    }
}
impl From<StorageError> for Failure {
    fn from(e: StorageError) -> Self {
        Self::Storage(e)
    }
}
impl From<rusqlite::Error> for Failure {
    fn from(e: rusqlite::Error) -> Self {
        Self::Storage(e.into())
    }
}
type Dispatch = Result<Reply, Failure>;
fn error(code: ErrorCode) -> ServiceError {
    ServiceError::code(code)
}

impl<R: RandomSource> Service<R> {
    /// Uses a generated run directory. Existing files must have this schema's
    /// identity; unknown layouts are refused, without migration or replacement.
    pub fn open(run_directory: &Path, initial_config: Config, random: R) -> Result<Self, StorageError> {
        if !initial_config.valid() {
            return Err(StorageError::InvalidConfig);
        }
        if !run_directory.is_dir() {
            return Err(StorageError::Io);
        }
        let path = run_directory.join("fake-cloud.db");
        let existing = path.try_exists().map_err(|_| StorageError::Io)?;
        if existing && (!path.is_file() || path.metadata().map_err(|_| StorageError::Io)?.len() == 0) {
            return Err(StorageError::Database);
        }
        let mut connection = Connection::open(path)?;
        connection.execute_batch("PRAGMA foreign_keys=ON")?;
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if existing {
            let identity: (i64, i64) =
                tx.query_row("SELECT format,revision FROM service_meta WHERE id=1", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?;
            if identity != (117, 1) {
                return Err(StorageError::Database);
            }
        } else {
            sql::initialize(&tx, &initial_config)?;
        }
        tx.commit()?;
        Ok(Self { connection, random })
    }
    /// State, quota, reference outcome and request log share this transaction,
    /// including logical errors. Entropy is drawn once per request.
    pub fn request(&mut self, scope: &Scope, caller: Caller, operation: Operation) -> Result<Response, StorageError> {
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let request_id = sql::counter(&tx, "request")?;
        let entropy = self.random.next_u64();
        let key = sql::scope_key(scope);
        let mut log = request_log(request_id, scope, caller, &operation);
        let result = match dispatch(&tx, &key, scope, request_id, operation, entropy) {
            Ok(reply) => Ok(reply),
            Err(Failure::Logical(e)) => Err(e),
            Err(Failure::Storage(e)) => return Err(e),
        };
        finish_log(&mut log, &result);
        let response = Response { request_id, result };
        tx.execute(
            "INSERT INTO requests VALUES(?1,?2,?3)",
            params![
                sql::write_u64(request_id)?,
                sql::encode(&log)?,
                sql::encode(&sql::outcome(&tx, &response)?)?
            ],
        )?;
        tx.commit()?;
        Ok(response)
    }
    pub fn into_random(self) -> R {
        self.random
    }
    pub fn config(&self, scope: &Scope) -> Result<Config, StorageError> {
        sql::config(&self.connection, &sql::scope_key(scope))
    }

    /// Convenience inspection is capped at 2048 entries. Use the page methods
    /// for retained logs/outcomes; outcomes contain version references only.
    pub fn request_log(&self) -> Result<Vec<RequestLog>, StorageError> {
        inspection_all(&self.connection, "log")
    }
    pub fn outcomes(&self) -> Result<Vec<OutcomeReference>, StorageError> {
        inspection_all(&self.connection, "outcome")
    }
    pub fn request_log_page(&self, after: u64, limit: usize) -> Result<Vec<RequestLog>, StorageError> {
        inspection_page(&self.connection, "log", after, limit)
    }
    pub fn outcomes_page(&self, after: u64, limit: usize) -> Result<Vec<OutcomeReference>, StorageError> {
        inspection_page(&self.connection, "outcome", after, limit)
    }

    /// Explicit single-version body resolution. The scope is checked before IO.
    pub fn resolve_record(&self, scope: &Scope, reference: &RecordReference) -> Result<Record, StorageError> {
        let matches: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM versions WHERE id=?1 AND scope=?2)",
            params![sql::write_u64(reference.version)?, sql::scope_key(scope)],
            |r| r.get(0),
        )?;
        if !matches {
            return Err(StorageError::Database);
        }
        sql::record(
            &self.connection,
            reference.version,
            &RequestedFields::Named(reference.fields.clone()),
        )
    }
    /// Small-fixture compatibility view. At most 2048 rows and 8 MiB of record
    /// wire bytes (including repeated historical bodies) may be materialized.
    /// Large fixtures use reference pages, stats, or individual record reads.
    pub fn inspect(&self, scope: &Scope) -> Result<Option<DatabaseState>, StorageError> {
        inspect(&self.connection, scope)
    }
    pub fn storage_stats(&self) -> Result<StorageStats, StorageError> {
        let c = &self.connection;
        let count = |table: &str| -> Result<u64, StorageError> {
            Ok(c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| sql::read_u64(r, 0))?)
        };
        Ok(StorageStats {
            payloads: count("payloads")?,
            payload_bytes: c.query_row("SELECT coalesce(sum(length(body)),0) FROM payloads", [], |r| {
                sql::read_u64(r, 0)
            })?,
            versions: count("versions")?,
            current_records: count("current_records")?,
            record_changes: count("record_changes")?,
            requests: count("requests")?,
            snapshots: count("snapshots")?,
            memberships: count("members")?,
        })
    }
}
fn inspection_all<T: serde::de::DeserializeOwned>(c: &Connection, column: &str) -> Result<Vec<T>, StorageError> {
    let count: u64 = c.query_row("SELECT count(*) FROM requests", [], |r| sql::read_u64(r, 0))?;
    if count > 2048 {
        return Err(StorageError::InspectionLimit);
    }
    inspection_page(c, column, 0, 2048)
}
fn inspection_page<T: serde::de::DeserializeOwned>(
    c: &Connection,
    column: &str,
    after: u64,
    limit: usize,
) -> Result<Vec<T>, StorageError> {
    if limit == 0 || limit > 2048 {
        return Err(StorageError::InspectionLimit);
    }
    let statement = match column {
        "log" => "SELECT log FROM requests WHERE id>?1 ORDER BY id LIMIT ?2",
        "outcome" => "SELECT outcome FROM requests WHERE id>?1 ORDER BY id LIMIT ?2",
        _ => return Err(StorageError::Database),
    };
    let mut query = c.prepare(statement)?;
    let rows = query.query_map(params![sql::write_u64(after)?, sql::write_usize(limit)?], |r| r.get(0))?;
    rows.map(|row| sql::decode(row?)).collect()
}
fn live_zone(c: &Connection, key: &str, zone: &str) -> Result<u64, Failure> {
    let row: Option<(u64, Option<Vec<u8>>)> = c
        .query_row(
            "SELECT generation,removed FROM zones WHERE scope=?1 AND zone=?2",
            params![key, zone],
            |r| Ok((sql::read_u64(r, 0)?, r.get(1)?)),
        )
        .optional()?;
    match row {
        Some((generation, None)) => Ok(generation),
        other => {
            let cause: Option<RemovalCause> = other.and_then(|(_, r)| r).map(sql::decode).transpose()?;
            Err(Failure::Logical(match cause {
                Some(RemovalCause::Purged) => error(ErrorCode::UserDeletedZone),
                Some(RemovalCause::EncryptedDataReset) => ServiceError {
                    encrypted_data_reset: true,
                    ..error(ErrorCode::ZoneNotFound)
                },
                _ => error(ErrorCode::ZoneNotFound),
            }))
        }
    }
}
fn dispatch(c: &Connection, key: &str, scope: &Scope, request: u64, op: Operation, entropy: u64) -> Dispatch {
    match op {
        Operation::FetchAllZones => {
            // One actual request transaction captures every live zone in this
            // scope, without pagination or incremental-feed inference.
            let mut query = c.prepare("SELECT zone FROM zones WHERE scope=?1 AND removed IS NULL ORDER BY zone")?;
            let zones = query
                .query_map([key], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Reply::AllZones(zones))
        }
        Operation::Configure(config) => {
            if !config.valid() {
                return Err(error(ErrorCode::InvalidArguments).into());
            }
            sql::ensure_scope(c, scope)?;
            c.execute(
                "UPDATE databases SET config=?2 WHERE scope=?1",
                params![key, sql::encode(&config)?],
            )?;
            Ok(Reply::Done)
        }
        Operation::SetQuota { bytes } => {
            sql::ensure_scope(c, scope)?;
            // Preserve the complete u64 quota as small JSON metadata.
            c.execute(
                "UPDATE databases SET quota=?2 WHERE scope=?1",
                params![key, bytes.map(|v| sql::encode(&v)).transpose()?],
            )?;
            Ok(Reply::Done)
        }
        Operation::CreateZone { zone } => {
            if zone.is_empty() {
                return Err(error(ErrorCode::InvalidArguments).into());
            }
            sql::ensure_scope(c, scope)?;
            let live: bool = c.query_row(
                "SELECT EXISTS(SELECT 1 FROM zones WHERE scope=?1 AND zone=?2 AND removed IS NULL)",
                params![key, zone],
                |r| r.get(0),
            )?;
            if live {
                return Ok(Reply::Done);
            }
            let sequence = sql::counter(c, "sequence")?;
            c.execute("INSERT INTO zones VALUES(?1,?2,?3,NULL,0) ON CONFLICT(scope,zone) DO UPDATE SET generation=excluded.generation,removed=NULL,used=0", params![key,zone,sql::write_u64(request)?])?;
            c.execute(
                "INSERT INTO zone_changes VALUES(?1,?2,?3,NULL)",
                params![sql::write_u64(sequence)?, key, zone],
            )?;
            Ok(Reply::Done)
        }
        Operation::DeleteZone { zone, cause } => {
            sql::ensure_scope(c, scope)?;
            live_zone(c, key, &zone)?;
            let sequence = sql::counter(c, "sequence")?;
            c.execute(
                "UPDATE databases SET used=used-(SELECT used FROM zones WHERE scope=?1 AND zone=?2) WHERE scope=?1",
                params![key, zone],
            )?;
            c.execute(
                "UPDATE zones SET removed=?3,used=0 WHERE scope=?1 AND zone=?2",
                params![key, zone, sql::encode(&cause)?],
            )?;
            c.execute(
                "DELETE FROM current_records WHERE scope=?1 AND zone=?2",
                params![key, zone],
            )?;
            c.execute(
                "INSERT INTO zone_changes VALUES(?1,?2,?3,?4)",
                params![sql::write_u64(sequence)?, key, zone, sql::encode(&cause)?],
            )?;
            Ok(Reply::Done)
        }
        Operation::Read { zone, name, fields } => {
            sql::ensure_scope(c, scope)?;
            live_zone(c, key, &zone)?;
            let id = current(c, key, &zone, &name)?.ok_or_else(|| error(ErrorCode::UnknownItem))?;
            Ok(Reply::Record(sql::record(c, id, &fields)?))
        }
        Operation::Modify { zone, atomic, changes } => {
            sql::ensure_scope(c, scope)?;
            modify(c, key, &zone, atomic, changes, entropy)
        }
        Operation::FetchZone { zone, token } => {
            sql::ensure_scope(c, scope)?;
            fetch(c, key, scope, request, Some(&zone), token)
        }
        Operation::FetchDatabase { token } => fetch(c, key, scope, request, None, token),
    }
}
fn current(c: &Connection, key: &str, zone: &str, name: &str) -> Result<Option<u64>, StorageError> {
    Ok(c.query_row(
        "SELECT version FROM current_records WHERE scope=?1 AND zone=?2 AND name=?3",
        params![key, zone, name],
        |r| sql::read_u64(r, 0),
    )
    .optional()?)
}
fn modify(c: &Connection, key: &str, zone: &str, atomic: bool, changes: Vec<Mutation>, entropy: u64) -> Dispatch {
    let generation = live_zone(c, key, zone)?;
    let config = sql::config(c, key)?;
    let request_bytes = changes
        .iter()
        .map(|change| match change {
            Mutation::Save { record, .. } => record_non_asset_bytes(zone, record),
            Mutation::Delete { name } => 16 + zone.len() as u64 + name.len() as u64,
        })
        .sum::<u64>();
    if changes.len() > config.limits.records_per_request || request_bytes > config.limits.non_asset_per_request {
        return Err(error(ErrorCode::LimitExceeded).into());
    }
    let mut seen = BTreeSet::new();
    if changes.iter().any(|change| !seen.insert(change.name())) {
        return Err(error(ErrorCode::InvalidArguments).into());
    }
    drop(seen);
    c.execute_batch("SAVEPOINT modify")?;
    let mut results = Vec::with_capacity(changes.len());
    for change in changes {
        let name = change.name().to_owned();
        let result = match mutate(c, key, zone, generation, change, &config, entropy) {
            Ok(result) => Ok(result),
            Err(Failure::Logical(e)) => Err(e),
            Err(e) => return Err(e),
        };
        results.push(MutationResult { name, result });
    }
    if atomic && results.iter().any(|r| r.result.is_err()) {
        c.execute_batch("ROLLBACK TO modify")?;
        for item in &mut results {
            if item.result.is_ok() {
                item.result = Err(error(ErrorCode::BatchRequestFailed));
            }
        }
    }
    c.execute_batch("RELEASE modify")?;
    Ok(Reply::Modified(results))
}
fn mutate(
    c: &Connection,
    key: &str,
    zone: &str,
    generation: u64,
    change: Mutation,
    config: &Config,
    entropy: u64,
) -> Result<MutationSuccess, Failure> {
    match change {
        Mutation::Delete { name } => {
            let id = current(c, key, zone, &name)?.ok_or_else(|| error(ErrorCode::UnknownItem))?;
            let (size, ty): (u64, String) = c.query_row(
                "SELECT size,record_type FROM versions WHERE id=?1",
                [sql::write_u64(id)?],
                |r| Ok((sql::read_u64(r, 0)?, r.get(1)?)),
            )?;
            let sequence = sql::counter(c, "sequence")?;
            c.execute(
                "DELETE FROM current_records WHERE scope=?1 AND zone=?2 AND name=?3",
                params![key, zone, name],
            )?;
            adjust_usage(c, key, zone, size, 0)?;
            c.execute(
                "INSERT INTO record_changes VALUES(?1,?2,?3,?4,?5,?6,NULL)",
                params![
                    sql::write_u64(sequence)?,
                    key,
                    zone,
                    sql::write_u64(generation)?,
                    name,
                    ty
                ],
            )?;
            Ok(MutationSuccess::Deleted)
        }
        Mutation::Save { record, if_unchanged } => {
            if !schema_valid(&record) {
                return Err(error(ErrorCode::InvalidArguments).into());
            }
            let non_asset = record_non_asset_bytes(zone, &record);
            if non_asset > config.limits.non_asset_per_record
                || record
                    .fields
                    .values()
                    .any(|f| matches!(f,Field::Asset(a) if a.size()>config.limits.asset_bytes))
            {
                return Err(error(ErrorCode::LimitExceeded).into());
            }
            if if_unchanged
                .as_ref()
                .is_some_and(|s| !valid_system_fields(zone, &record.name, s))
            {
                return Err(error(ErrorCode::InvalidArguments).into());
            }
            let old = current(c, key, zone, &record.name)?;
            let mut old_size = 0;
            if let Some(id) = old {
                let (size, system): (u64, Vec<u8>) = c.query_row(
                    "SELECT size,system FROM versions WHERE id=?1",
                    [sql::write_u64(id)?],
                    |r| Ok((sql::read_u64(r, 0)?, r.get(1)?)),
                )?;
                old_size = size;
                if if_unchanged.as_ref().map(SystemFields::bytes) != Some(system.as_slice()) {
                    return Err(ServiceError {
                        server_record: Some(Box::new(sql::record(c, id, &RequestedFields::All)?)),
                        ..error(ErrorCode::ServerRecordChanged)
                    }
                    .into());
                }
            } else if if_unchanged.is_some() && config.tagged_missing == TaggedMissing::UnknownItem {
                return Err(error(ErrorCode::UnknownItem).into());
            }
            let size = non_asset + asset_bytes(&record);
            let (used, quota): (u64, Option<Vec<u8>>) =
                c.query_row("SELECT used,quota FROM databases WHERE scope=?1", [key], |r| {
                    Ok((sql::read_u64(r, 0)?, r.get(1)?))
                })?;
            let quota: Option<u64> = quota.map(sql::decode).transpose()?;
            let new_used = used
                .checked_sub(old_size)
                .and_then(|v| v.checked_add(size))
                .ok_or(StorageError::Exhausted)?;
            if quota.is_some_and(|limit| new_used > limit) {
                return Err(error(ErrorCode::QuotaExceeded).into());
            }
            // SQLite would promote an overflowing INTEGER sum to REAL.
            sql::write_u64(new_used)?;
            let id = sql::counter(c, "version")?;
            let saved = Record {
                system_fields: system_fields(zone, &record.name, &tag(id, entropy)),
                name: record.name,
                record_type: record.record_type,
                fields: record.fields,
            };
            sql::save_version(c, key, zone, generation, id, &saved, size)?;
            let sequence = sql::counter(c, "sequence")?;
            c.execute("INSERT INTO current_records VALUES(?1,?2,?3,?4) ON CONFLICT(scope,zone,name) DO UPDATE SET version=excluded.version", params![key,zone,saved.name,sql::write_u64(id)?])?;
            adjust_usage(c, key, zone, old_size, size)?;
            c.execute(
                "INSERT INTO record_changes VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![
                    sql::write_u64(sequence)?,
                    key,
                    zone,
                    sql::write_u64(generation)?,
                    saved.name,
                    saved.record_type,
                    sql::write_u64(id)?
                ],
            )?;
            Ok(MutationSuccess::Saved(saved))
        }
    }
}
fn adjust_usage(c: &Connection, key: &str, zone: &str, old: u64, new: u64) -> Result<(), StorageError> {
    let old = sql::write_u64(old)?;
    let new = sql::write_u64(new)?;
    c.execute(
        "UPDATE databases SET used=used-?2+?3 WHERE scope=?1",
        params![key, old, new],
    )?;
    c.execute(
        "UPDATE zones SET used=used-?3+?4 WHERE scope=?1 AND zone=?2",
        params![key, zone, old, new],
    )?;
    Ok(())
}
fn fetch(c: &Connection, key: &str, scope: &Scope, request: u64, zone: Option<&str>, token: Option<Token>) -> Dispatch {
    let generation = match zone {
        Some(z) => live_zone(c, key, z)?,
        None => 0,
    };
    let sequence: u64 = c.query_row("SELECT sequence FROM service_meta WHERE id=1", [], |r| {
        sql::read_u64(r, 0)
    })?;
    if let Some(t) = &token {
        if t.scope != *scope || t.zone.as_deref() != zone {
            return Err(error(ErrorCode::InvalidArguments).into());
        }
        if t.generation != generation || t.watermark > sequence {
            return Err(error(ErrorCode::ChangeTokenExpired).into());
        }
    }
    let (id, watermark, offset, count) = if let Some(t) = token.as_ref().filter(|t| t.snapshot.is_some()) {
        let id = t.snapshot.ok_or_else(|| error(ErrorCode::InvalidArguments))?;
        let saved_id = sql::write_u64(id).map_err(|_| error(ErrorCode::ChangeTokenExpired))?;
        let saved: Option<(String, Option<String>, u64, u64, usize)> = c
            .query_row(
                "SELECT scope,zone,generation,watermark,count FROM snapshots WHERE id=?1",
                [saved_id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        sql::read_u64(r, 2)?,
                        sql::read_u64(r, 3)?,
                        sql::read_usize(r, 4)?,
                    ))
                },
            )
            .optional()?;
        let (saved_scope, saved_zone, saved_generation, watermark, count) =
            saved.ok_or_else(|| error(ErrorCode::ChangeTokenExpired))?;
        if saved_scope != key
            || saved_zone.as_deref() != zone
            || saved_generation != generation
            || watermark != t.watermark
            || t.offset > count
        {
            return Err(error(ErrorCode::InvalidArguments).into());
        }
        (id, watermark, t.offset, count)
    } else {
        c.execute(
            "INSERT INTO snapshots VALUES(?1,?2,?3,?4,?5,0)",
            params![
                sql::write_u64(request)?,
                key,
                zone,
                sql::write_u64(generation)?,
                sql::write_u64(sequence)?
            ],
        )?;
        match (zone, token.as_ref()) {
            (Some(z), None) => {
                c.execute("INSERT INTO members SELECT ?1,r.name,r.version,v.record_type,NULL FROM current_records r JOIN versions v ON v.id=r.version WHERE r.scope=?2 AND r.zone=?3", params![sql::write_u64(request)?,key,z])?;
            }
            (Some(z), Some(t)) => {
                c.execute("INSERT INTO members SELECT ?1,c.name,c.version,c.record_type,NULL FROM record_changes c JOIN
                    (SELECT max(sequence) AS sequence FROM record_changes WHERE scope=?2 AND zone=?3 AND sequence>?4 AND sequence<=?5 GROUP BY name) latest
                    ON c.sequence=latest.sequence", params![sql::write_u64(request)?,key,z,sql::write_u64(t.watermark)?,sql::write_u64(sequence)?])?;
            }
            (None, None) => {
                c.execute(
                    "INSERT INTO members SELECT ?1,zone,NULL,NULL,NULL FROM zones WHERE scope=?2 AND removed IS NULL",
                    params![sql::write_u64(request)?, key],
                )?;
            }
            (None, Some(t)) => {
                c.execute("INSERT INTO members SELECT ?1,c.zone,NULL,NULL,c.removed FROM zone_changes c JOIN
                    (SELECT max(sequence) AS sequence FROM zone_changes WHERE scope=?2 AND sequence>?3 AND sequence<=?4 GROUP BY zone) latest
                    ON c.sequence=latest.sequence", params![sql::write_u64(request)?,key,sql::write_u64(t.watermark)?,sql::write_u64(sequence)?])?;
            }
        }
        let count: usize = c.query_row(
            "SELECT count(*) FROM members WHERE snapshot=?1",
            [sql::write_u64(request)?],
            |r| sql::read_usize(r, 0),
        )?;
        c.execute(
            "UPDATE snapshots SET count=?2 WHERE id=?1",
            params![sql::write_u64(request)?, sql::write_usize(count)?],
        )?;
        (request, sequence, 0, count)
    };
    sql::ensure_scope(c, scope)?;
    let size = sql::config(c, key)?.page_size;
    let end = offset.checked_add(size).ok_or(StorageError::Exhausted)?.min(count);
    let more = end < count;
    let next = Token {
        scope: scope.clone(),
        zone: zone.map(str::to_owned),
        generation,
        watermark,
        snapshot: if more { Some(id) } else { None },
        offset: if more { end } else { 0 },
    };
    let mut statement = c.prepare(
        "SELECT name,version,record_type,removed FROM members WHERE snapshot=?1 ORDER BY name LIMIT ?2 OFFSET ?3",
    )?;
    let rows = statement.query_map(
        params![sql::write_u64(id)?, sql::write_usize(size)?, sql::write_usize(offset)?],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                sql::read_optional_u64(r, 1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<Vec<u8>>>(3)?,
            ))
        },
    )?;
    if zone.is_some() {
        let mut changes = Vec::new();
        for row in rows {
            let (name, version, ty, _) = row?;
            changes.push(match version {
                Some(v) => RecordChange::Saved(sql::record(c, v, &RequestedFields::All)?),
                None => RecordChange::Deleted {
                    name,
                    record_type: ty.ok_or(StorageError::Encoding)?,
                },
            });
        }
        Ok(Reply::ZoneChanges(Page {
            changes,
            token: next,
            more,
        }))
    } else {
        let mut changes = Vec::new();
        for row in rows {
            let (name, _, _, removed) = row?;
            changes.push(ZoneChange {
                zone: name,
                removed: removed.map(sql::decode).transpose()?,
            });
        }
        Ok(Reply::DatabaseChanges(Page {
            changes,
            token: next,
            more,
        }))
    }
}
fn inspect(c: &Connection, scope: &Scope) -> Result<Option<DatabaseState>, StorageError> {
    let key = sql::scope_key(scope);
    let header: Option<(Vec<u8>, Option<Vec<u8>>, u64)> = c
        .query_row("SELECT config,quota,used FROM databases WHERE scope=?1", [&key], |r| {
            Ok((r.get(0)?, r.get(1)?, sql::read_u64(r, 2)?))
        })
        .optional()?;
    let Some((config, quota, used)) = header else {
        return Ok(None);
    };
    let rows: u64 = c.query_row("SELECT (SELECT count(*) FROM zones WHERE scope=?1)+(SELECT count(*) FROM current_records WHERE scope=?1)+(SELECT count(*) FROM record_changes WHERE scope=?1)+(SELECT count(*) FROM zone_changes WHERE scope=?1)", [&key], |r| sql::read_u64(r, 0))?;
    if rows > 2048 {
        return Err(StorageError::InspectionLimit);
    }
    let bytes: u64 = c.query_row("SELECT coalesce(sum(v.size),0) FROM versions v JOIN (SELECT version FROM current_records WHERE scope=?1 UNION ALL SELECT version FROM record_changes WHERE scope=?1 AND version IS NOT NULL) refs ON refs.version=v.id", [&key], |r| sql::read_u64(r, 0))?;
    if bytes > 8 * 1024 * 1024 {
        return Err(StorageError::InspectionLimit);
    }
    let mut db = DatabaseState {
        scope: scope.clone(),
        config: sql::decode(config)?,
        quota: quota.map(sql::decode).transpose()?,
        used_bytes: used,
        zones: BTreeMap::new(),
        changes: Vec::new(),
    };
    let mut zones = c.prepare("SELECT zone,generation,removed FROM zones WHERE scope=?1 ORDER BY zone")?;
    for row in zones.query_map([&key], |r| {
        Ok((
            r.get::<_, String>(0)?,
            sql::read_u64(r, 1)?,
            r.get::<_, Option<Vec<u8>>>(2)?,
        ))
    })? {
        let (zone, generation, removed) = row?;
        let mut z = ZoneState {
            generation,
            removed: removed.map(sql::decode).transpose()?,
            records: BTreeMap::new(),
            changes: Vec::new(),
        };
        let mut records = c.prepare("SELECT version FROM current_records WHERE scope=?1 AND zone=?2 ORDER BY name")?;
        for row in records.query_map(params![key, zone], |r| sql::read_u64(r, 0))? {
            let record = sql::record(c, row?, &RequestedFields::All)?;
            z.records.insert(record.name.clone(), record);
        }
        let mut changes = c.prepare("SELECT sequence,generation,name,record_type,version FROM record_changes WHERE scope=?1 AND zone=?2 ORDER BY sequence")?;
        for row in changes.query_map(params![key, zone], |r| {
            Ok((
                sql::read_u64(r, 0)?,
                sql::read_u64(r, 1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                sql::read_optional_u64(r, 4)?,
            ))
        })? {
            let (sequence, generation, name, record_type, version) = row?;
            z.changes.push(LoggedRecordChange {
                sequence,
                generation,
                change: match version {
                    Some(v) => RecordChange::Saved(sql::record(c, v, &RequestedFields::All)?),
                    None => RecordChange::Deleted { name, record_type },
                },
            });
        }
        db.zones.insert(zone, z);
    }
    let mut changes = c.prepare("SELECT sequence,zone,removed FROM zone_changes WHERE scope=?1 ORDER BY sequence")?;
    for row in changes.query_map([&key], |r| {
        Ok((
            sql::read_u64(r, 0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<Vec<u8>>>(2)?,
        ))
    })? {
        let (sequence, zone, removed) = row?;
        db.changes.push(LoggedZoneChange {
            sequence,
            change: ZoneChange {
                zone,
                removed: removed.map(sql::decode).transpose()?,
            },
        });
    }
    Ok(Some(db))
}
pub fn record_non_asset_bytes(zone: &str, record: &RecordInput) -> u64 {
    non_asset_bytes(zone, &record.name, &record.record_type, &record.fields)
}
fn non_asset_bytes(zone: &str, name: &str, ty: &str, fields: &BTreeMap<String, Field>) -> u64 {
    let strings = |s: &str| 8 + s.len() as u64;
    strings(name)
        + strings(ty)
        + 8
        + fields
            .iter()
            .map(|(key, value)| {
                strings(key)
                    + 1
                    + match value {
                        Field::Asset(_) => 40,
                        Field::Integer(_) => 8,
                        _ => 8 + value.value_size(),
                    }
            })
            .sum::<u64>()
        + 8
        + 4
        + strings(zone)
        + strings(name)
        + 32
}
fn fields_asset_bytes(fields: &BTreeMap<String, Field>) -> u64 {
    fields
        .values()
        .filter_map(|f| match f {
            Field::Asset(a) => Some(a.size()),
            _ => None,
        })
        .sum()
}
fn asset_bytes(record: &RecordInput) -> u64 {
    fields_asset_bytes(&record.fields)
}
fn schema_valid(record: &RecordInput) -> bool {
    if record.name.is_empty() {
        return false;
    }
    let allowed_asset = match record.record_type.as_str() {
        "MGActive" | "MGEpoch" | "MGMediaRoot" => None,
        "MGEntity" => Some("pa"),
        "MGGeneration" => Some("ma"),
        "MGBlob" => Some("a"),
        _ => return false,
    };
    matches!(record.fields.get("e"), Some(Field::EncryptedBytes(_)))
        && record.fields.iter().all(|(key, field)| {
            if key == "e" {
                matches!(field, Field::EncryptedBytes(_))
            } else {
                Some(key.as_str()) == allowed_asset && matches!(field, Field::Asset(a) if a.valid())
            }
        })
}
fn system_fields(zone: &str, name: &str, tag: &[u8; 32]) -> SystemFields {
    let mut bytes = b"MGC1".to_vec();
    for s in [zone, name] {
        bytes.extend_from_slice(&(s.len() as u64).to_be_bytes());
        bytes.extend_from_slice(s.as_bytes());
    }
    bytes.extend_from_slice(tag);
    SystemFields(bytes)
}
fn valid_system_fields(zone: &str, name: &str, fields: &SystemFields) -> bool {
    // Compare the identity prefix; the trailing tag is always 32 opaque bytes.
    let expected = system_fields(zone, name, &[0; 32]);
    fields.0.len() == expected.0.len() && fields.0[..fields.0.len() - 32] == expected.0[..expected.0.len() - 32]
}
fn tag(version: u64, entropy: u64) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"meguri synthetic cloud version 1");
    hash.update(version.to_be_bytes());
    hash.update(entropy.to_be_bytes());
    hash.finalize().into()
}

fn request_log(request_id: u64, scope: &Scope, caller: Caller, op: &Operation) -> RequestLog {
    let (kind, zone) = match op {
        Operation::FetchAllZones => (RequestKind::FetchAllZones, None),
        Operation::CreateZone { zone } => (RequestKind::CreateZone, Some(zone)),
        Operation::DeleteZone { zone, .. } => (RequestKind::DeleteZone, Some(zone)),
        Operation::Modify { zone, .. } => (RequestKind::Modify, Some(zone)),
        Operation::Read { zone, .. } => (RequestKind::Read, Some(zone)),
        Operation::FetchZone { zone, .. } => (RequestKind::FetchZone, Some(zone)),
        Operation::FetchDatabase { .. } => (RequestKind::FetchDatabase, None),
        Operation::Configure(_) => (RequestKind::Configure, None),
        Operation::SetQuota { .. } => (RequestKind::SetQuota, None),
    };
    let records = match op {
        Operation::Modify { zone, changes, .. } => changes
            .iter()
            .map(|c| {
                let (non_asset_bytes, asset_bytes) = match c {
                    Mutation::Save { record, .. } => (record_non_asset_bytes(zone, record), asset_bytes(record)),
                    Mutation::Delete { name } => (16 + zone.len() as u64 + name.len() as u64, 0),
                };
                RecordSummary {
                    name: c.name().into(),
                    non_asset_bytes,
                    asset_bytes,
                    outcome: LogOutcome::Success,
                }
            })
            .collect(),
        Operation::Read { name, .. } => vec![RecordSummary {
            name: name.clone(),
            non_asset_bytes: 0,
            asset_bytes: 0,
            outcome: LogOutcome::Success,
        }],
        _ => vec![],
    };
    RequestLog {
        request_id,
        scope: scope.clone(),
        caller,
        zone: zone.cloned(),
        kind,
        records,
        outcome: LogOutcome::Success,
    }
}
fn summary(zone: &str, record: &Record) -> RecordSummary {
    RecordSummary {
        name: record.name.clone(),
        non_asset_bytes: non_asset_bytes(zone, &record.name, &record.record_type, &record.fields),
        asset_bytes: fields_asset_bytes(&record.fields),
        outcome: LogOutcome::Success,
    }
}
fn finish_log(log: &mut RequestLog, result: &Result<Reply, ServiceError>) {
    match result {
        Err(e) => {
            log.outcome = LogOutcome::Failure(e.code);
            for r in &mut log.records {
                r.outcome = LogOutcome::Failure(e.code);
            }
        }
        Ok(Reply::Modified(results)) => {
            for (item, result) in log.records.iter_mut().zip(results) {
                if let Err(e) = &result.result {
                    item.outcome = LogOutcome::Failure(e.code);
                    if log.outcome == LogOutcome::Success {
                        log.outcome = LogOutcome::Failure(e.code);
                    }
                }
            }
        }
        Ok(Reply::Record(record)) => log.records = vec![summary(log.zone.as_deref().unwrap_or(""), record)],
        Ok(Reply::ZoneChanges(page)) => {
            log.records = page
                .changes
                .iter()
                .map(|c| match c {
                    RecordChange::Saved(record) => summary(log.zone.as_deref().unwrap_or(""), record),
                    RecordChange::Deleted { name, .. } => RecordSummary {
                        name: name.clone(),
                        non_asset_bytes: 0,
                        asset_bytes: 0,
                        outcome: LogOutcome::Success,
                    },
                })
                .collect();
        }
        _ => {}
    }
}
