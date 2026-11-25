// SPDX-License-Identifier: MIT
use super::*;
use crate::{collection::schema, storage::{backups as store, collection::ReadPin}, media::{blobs, index}};
use std::{collections::BTreeMap, path::PathBuf};

pub use store::Backup;

#[derive(serde::Serialize)]
pub struct BackupHistory {
    pub account_id: String,
    pub backups: Vec<Backup>,
}

fn check_expected_account(id: &str, expected: Option<&str>) -> Result<()> {
    if expected.is_some_and(|expected| expected != id) {
        return Err(Error::new("stale_authority"));
    }
    Ok(())
}

// Collection admission and temporary reservations bound these jobs. Keep root
// ownership and validation without occupying a shared metadata worker slot.
async fn backup_files<T: Send + 'static>(
    root: Arc<crate::DataRoot>,
    job: impl FnOnce(&crate::DataRoot) -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(move || {
        root.validate()?;
        job(&root)
    })
    .await
    .map_err(|_| Error::new("storage_unavailable"))?
}
/// The mutex guard alone does not retain the Weak-map's publication barrier.
/// Every blocking stage owns this lease, including when its waiter is dropped.
struct BackupWork {
    gate: Arc<PublicationBarrier>,
    _backup: tokio::sync::OwnedMutexGuard<()>,
}
impl BackupWork {
    fn acquire(gate: Arc<PublicationBarrier>) -> Result<Arc<Self>> {
        let guard=gate.try_backup().ok_or(Error::new("busy"))?;
        Ok(Arc::new(Self {gate,_backup:guard}))
    }
}
/// Drop the candidate before releasing the last backup-work lease. This also
/// covers a blocking job whose completed result has no surviving receiver.
struct Candidate<T> {
    value: T,
    _work: Arc<BackupWork>,
}
impl<T> std::ops::Deref for Candidate<T> {
    type Target=T;
    fn deref(&self) -> &T {&self.value}
}
impl<T> std::ops::DerefMut for Candidate<T> {
    fn deref_mut(&mut self) -> &mut T {&mut self.value}
}
struct Capture {
    point: store::Point,
    media: BTreeMap<String,blobs::Content>,
    source: Connection,
    path: PathBuf,
    version: i64,
    cursor: i32,
    epoch: i64,
    _pin: ReadPin,
    _reservations: Vec<crate::limits::TempReservation>,
    _capacity: tokio::sync::OwnedSemaphorePermit,
    _work: Arc<BackupWork>,
}
fn eligible(c: &Connection,id: &str,expected: i64) -> Result<()> {
    admission(c)?;
    read_account(c,id)?.filter(|a|a.state=="ready").ok_or(Error::new("route_not_found"))?;
    if crate::storage::reset::admitted_epoch(c,id)?!=expected {return Err(Error::new("busy"));}
    Ok(())
}
impl Service {
    /// Serial, keyset-paged account traversal. One failed account does not stop
    /// later accounts, and a busy account is reconsidered on the next sweep.
    pub(crate) async fn sweep_backups(self: &Arc<Self>,clock: impl Fn() -> i64 + Clone + Send + 'static) -> Result<()> {
        let owner=self.clone();
        tokio::spawn(async move {owner.sweep_backups_inner(clock).await}).await.map_err(|_|Error::new("storage_unavailable"))?
    }
    async fn sweep_backups_inner(&self,clock: impl Fn() -> i64 + Clone + Send + 'static) -> Result<()> {
        let mut after=String::new();
        loop {
            let cursor=after.clone();
            let ids=self.metadata.run(move |c| {
                admission(c)?;
                let mut q=c.prepare("SELECT id FROM accounts WHERE provisioning_state='ready' AND id>?1 ORDER BY id LIMIT 32")?;
                Ok(q.query_map([cursor],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?)
            }).await?;
            if ids.is_empty() {return Ok(());}
            for id in ids {
                after=id.clone();
                if self.daily_backup(id,clock.clone()).await.is_err() {
                    // No paths, credentials or collection content in diagnostics.
                    eprintln!("backup phase=capture outcome=unavailable");
                }
            }
        }
    }
    #[cfg(feature="test-hooks")]
    pub async fn capture_backups_at(self: &Arc<Self>,at: i64) -> Result<()> {self.sweep_backups(move ||at).await}
    #[cfg(feature="test-hooks")]
    pub async fn capture_backups_with_clock(self: &Arc<Self>,clock: impl Fn() -> i64 + Clone + Send + 'static) -> Result<()> {self.sweep_backups(clock).await}
    /// Direct fixture entry exercises cancellation of the daily future itself.
    #[cfg(feature="test-hooks")]
    pub async fn capture_account_backup_at(&self,id: String,at: i64) -> Result<()> {self.daily_backup(id,move ||at).await}

    async fn daily_backup(
        &self,
        id: String,
        clock: impl Fn() -> i64 + Clone + Send + 'static,
    ) -> Result<()> {
        let epoch = self.metadata.run({
            let id = id.clone();
            move |c| {
                admission(c)?;
                crate::storage::reset::admitted_epoch(c, &id)
            }
        }).await?;
        let gate = self.barriers.get(&id)?;
        let work = BackupWork::acquire(gate.clone())?;
        self.reclaim_backup_payloads(id.clone(), epoch, work.clone()).await?;
        let at = clock();
        let exists = self.metadata.run({
            let id = id.clone();
            move |c| {
                eligible(c, &id, epoch)?;
                Ok(c.prepare("SELECT 1 FROM backup_points WHERE account_id=?1 AND kind<>'before_restore' AND created_at>=?2 AND created_at<?3")?
                    .exists(params![id, at.div_euclid(86400) * 86400, (at.div_euclid(86400) + 1) * 86400])?)
            }
        }).await?;
        if !exists {
            let captured = self.capture_backup(id.clone(), epoch, clock.clone(), "daily", work.clone()).await?;
            self.publish_backup(id.clone(), captured, epoch, work.clone()).await?;
        }
        let guard = gate.enter().await;
        #[cfg(feature = "test-hooks")]
        let hooks = self.hooks.clone();
        self.metadata.files(move |root| {
            let _work = work;
            let _publication = (gate, guard);
            {
                let mut c = crate::storage::open(root.database(), false)?;
                let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                eligible(&tx, &id, epoch)?;
                store::prune(&tx, &id, clock(), &[])?;
                tx.commit()?;
            }
            #[cfg(feature = "test-hooks")]
            tokio::runtime::Handle::current().block_on(hooks.hit("backup_cleanup"));
            let c = crate::storage::open(root.database(), false)?;
            eligible(&c, &id, epoch)?;
            root.cleanup_backups(&c, &id)
        }).await
    }
    async fn reclaim_backup_payloads(
        &self,
        id: String,
        epoch: i64,
        work: Arc<BackupWork>,
    ) -> Result<()> {
        let gate = work.gate.clone();
        let guard = gate.enter().await;
        self.metadata.files(move |root| {
            let _work = work;
            let _publication = (gate, guard);
            let c = crate::storage::open(root.database(), false)?;
            eligible(&c, &id, epoch)?;
            // A new account has no backup directory until its first capture.
            root.backup_directory(&id)?;
            root.cleanup_backups(&c, &id)
        }).await
    }
    async fn capture_backup(
        &self,
        id: String,
        epoch: i64,
        clock: impl Fn() -> i64 + Send + 'static,
        kind: &'static str,
        work: Arc<BackupWork>,
    ) -> Result<Capture> {
        let gate = work.gate.clone();
        let capacity = self.collections.acquire()?;
        let metadata = self.temp.reserve(4096)?;
        let budget = self.temp.clone();
        let guard = gate.enter().await;
        let (source, pin, path, version, cursor, media, reservation, stage, at) = self.metadata.files({
            let id = id.clone();
            move |root| {
                let _publication = (gate, guard);
                let mut metadata_connection = crate::storage::open(root.database(), false)?;
                let c = metadata_connection.transaction()?;
                eligible(&c, &id, epoch)?;
                // Bound both manifest memory and snapshot scratch before copying.
                let names: i64 = c.query_row(
                    "SELECT coalesce(sum(4*length(CAST(name AS BLOB))+2048),0) FROM media_names WHERE account_id=?1 AND blob IS NOT NULL",
                    [&id], |r| r.get(0),
                )?;
                let path = root.collection_path(&id)?;
                let source = schema::open(&path, false)?;
                source.execute_batch("BEGIN DEFERRED")?;
                source.query_row("SELECT mod FROM col WHERE id=1", [], |r| r.get::<_, i64>(0))?;
                let version = source.pragma_query_value(None, "data_version", |r| r.get::<_, i64>(0))?;
                let pages: i64 = source.pragma_query_value(None, "page_count", |r| r.get(0))?;
                let size: i64 = source.pragma_query_value(None, "page_size", |r| r.get(0))?;
                store::collection_bytes(pages, size)?;
                let reservation = budget.reserve(
                    schema::snapshot_reservation_bytes(pages, size)
                        .and_then(|n| n.checked_add(u64::try_from(names).ok()?))
                        .ok_or(Error::new("storage_unavailable"))?,
                )?;
                let media = store::media(&c, &id, None)?;
                let cursor = index::cursor(&c, &id)?;
                let pin = root.pin_collection(&path)?;
                // Date the successful snapshot pin, not the start of the sweep.
                let at = clock();
                let stage = Candidate { value: root.stage_backup(&id)?, _work: work };
                Ok((source, pin, path, version, cursor, media, reservation, stage, at))
            }
        }).await?;
        self.pause("backup_snapshot_pinned").await;
        backup_files(self.metadata.owner(), move |root| {
            // No publication guard or metadata writer spans page copying,
            // validation or hashing. Never invoke the native full download.
            schema::snapshot(&source, &stage.path)?;
            schema::validate(&schema::open_upload(&stage.path)?)?;
            source.execute_batch("ROLLBACK")?;
            let mut verified = std::collections::BTreeSet::new();
            for content in media.values() {
                if verified.insert(content.blob.clone()) {
                    blobs::verify(&mut root.media_blob(&id, &content.blob)?, content)?;
                }
            }
            phase("backup_before_payload")?;
            let point = stage.publish(at, kind)?;
            phase("backup_after_payload")?;
            Ok(Capture {
                point, media, source, path, version, cursor, epoch,
                _pin: pin,
                _reservations: vec![metadata, reservation],
                _capacity: capacity,
                _work: stage._work.clone(),
            })
        }).await
    }
    async fn publish_backup(&self,id: String,captured: Capture,epoch: i64,work: Arc<BackupWork>) -> Result<()> {
        let gate=work.gate.clone();
        let guard=gate.enter().await;
        self.metadata.run(move |c| {
            let _work=work;
            let _publication=(gate,guard);
            let tx=c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            eligible(&tx,&id,epoch)?;
            // Retain the whole capture, including its pins and reservations,
            // until publication completes; do not clone the media manifest.
            let captured=captured;
            // Pinning may cross UTC midnight after the early duplicate check.
            let at=captured.point.summary.created_at;
            if tx.prepare("SELECT 1 FROM backup_points WHERE account_id=?1 AND kind<>'before_restore' AND created_at>=?2 AND created_at<?3")?.exists(params![id,at.div_euclid(86400)*86400,(at.div_euclid(86400)+1)*86400])? {return Ok(());}
            store::insert(&tx,&id,&captured.point,&captured.media,epoch)?;
            phase("backup_before_publish")?;
            tx.commit()?;
            Ok(())
        }).await
    }
    pub async fn backups(
        &self,
        authority: BrowserAuthority,
        expected_account: Option<String>,
    ) -> Result<BackupHistory> {
        #[cfg(feature = "test-hooks")]
        let hooks = self.hooks.clone();
        self.metadata.run(move |c| {
            let tx = c.transaction()?;
            admission(&tx)?;
            let account = check_browser(&tx, &authority, "authentication_failed")?;
            let id = account.identity.id;
            check_expected_account(&id, expected_account.as_deref())?;
            crate::storage::reset::admitted_epoch(&tx, &id)?;
            #[cfg(feature = "test-hooks")]
            tokio::runtime::Handle::current().block_on(hooks.hit("backup_history_admitted"));
            let backups = store::list(&tx, &id)?;
            Ok(BackupHistory { account_id: id, backups })
        }).await
    }
    pub async fn restore_backup(
        self: &Arc<Self>,
        authority: BrowserAuthority,
        backup: String,
    ) -> Result<()> {
        self.restore_backup_for_account(authority, backup, None).await.map(|_| ())
    }
    pub async fn restore_backup_for_account(
        self: &Arc<Self>,
        authority: BrowserAuthority,
        backup: String,
        expected_account: Option<String>,
    ) -> Result<String> {
        let owner = self.clone();
        tokio::spawn(async move {
            owner.restore_backup_inner(authority, backup, expected_account).await.map_err(|e| {
                if matches!(e.code, "invalid_collection" | "media_integrity") {
                    Error::new("storage_unavailable")
                } else {
                    e
                }
            })
        }).await.map_err(|_| Error::new("storage_unavailable"))?
    }
    async fn restore_backup_inner(
        &self,
        authority: BrowserAuthority,
        backup: String,
        expected_account: Option<String>,
    ) -> Result<String> {
        let actor = authority.clone();
        let (id, epoch) = self.metadata.run(move |c| {
            let tx = c.transaction()?;
            admission(&tx)?;
            let account = check_browser(&tx, &actor, "authentication_failed")?;
            let id = account.identity.id;
            check_expected_account(&id, expected_account.as_deref())?;
            let epoch = crate::storage::reset::admitted_epoch(&tx, &id)?;
            Ok((id, epoch))
        }).await?;
        let gate = self.barriers.get(&id)?;
        let work = BackupWork::acquire(gate.clone())?;
        // Verify and prepare the selected immutable point before the safety
        // capture. No invalid/foreign ID can create a new recovery point.
        let capacity = self.collections.acquire()?;
        let budget = self.temp.clone();
        #[cfg(feature = "test-hooks")]
        let hooks = self.hooks.clone();
        let (generation, selected_media, reservations, capacity) = backup_files(
            self.metadata.owner(),
            {
                let id = id.clone();
                let backup = backup.clone();
                let work = work.clone();
                move |root| {
                    // The worker owns admission even if its caller is cancelled.
                    let c = crate::storage::open(root.database(), false)?;
                    eligible(&c, &id, epoch)?;
                    let point = store::point(&c, &id, &backup)?;
                    let manifest_bytes: i64 = c.query_row(
                        "SELECT coalesce(sum(4*length(CAST(name AS BLOB))+2048),0) FROM backup_media WHERE account_id=?1 AND backup_id=?2",
                        params![id, backup], |r| r.get(0),
                    )?;
                    let source = schema::open_upload(
                        &root.stores(&id)?.directory.join("backups").join(&point.payload),
                    )?;
                    let pages: i64 = source.pragma_query_value(None, "page_count", |r| r.get(0))?;
                    let size: i64 = source.pragma_query_value(None, "page_size", |r| r.get(0))?;
                    if store::collection_bytes(pages, size)? != point.bytes {
                        return Err(Error::new("storage_unavailable"));
                    }
                    drop(source);
                    let hold = budget.reserve(
                        schema::snapshot_reservation_bytes(pages, size)
                            .and_then(|n| n.checked_add(u64::try_from(manifest_bytes).ok()?))
                            .and_then(|n| n.checked_add(4096))
                            .ok_or(Error::new("storage_unavailable"))?,
                    )?;
                    let selected_media = store::media(&c, &id, Some(&backup))?;
                    drop(c);
                    #[cfg(feature = "test-hooks")]
                    tokio::runtime::Handle::current().block_on(hooks.hit("backup_restore_preparing"));
                    let mut file = root.backup_file(&id, &point.payload)?;
                    if store::digest(&mut file)? != (point.bytes, point.digest) {
                        return Err(Error::new("storage_unavailable"));
                    }
                    let mut verified = std::collections::BTreeSet::new();
                    for content in selected_media.values() {
                        if verified.insert(content.blob.clone()) {
                            blobs::verify(&mut root.media_blob(&id, &content.blob)?, content)?;
                        }
                    }
                    let generation = Candidate {
                        value: root.stage_collection(&id)?,
                        _work: work,
                    };
                    std::io::copy(&mut file, &mut generation.writer()?)?;
                    schema::validate(&schema::open_upload(&generation.path)?)?;
                    Ok((generation, selected_media, hold, capacity))
                }
            },
        ).await?;
        // Reclaim any unpublished payload before allocating safety-copy scratch.
        self.reclaim_backup_payloads(id.clone(), epoch, work.clone()).await?;
        let safety = self.capture_backup(id.clone(), epoch, now, "before_restore", work.clone()).await?;
        // Both stamps strictly exceed both live and restored stamps. A repeated
        // restore in one millisecond still forces collection reconciliation.
        let old: (i64, i64) = safety.source.query_row(
            "SELECT scm,mod FROM col WHERE id=1", [], |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let (mut generation, reservations, capacity) = backup_files(self.metadata.owner(), move |_| {
            let resources = (reservations, capacity);
            let c = schema::open(&generation.path, true)?;
            let restored: (i64, i64) = c.query_row(
                "SELECT scm,mod FROM col WHERE id=1", [], |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let sampled = i64::try_from(time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000)
                .map_err(|_| Error::new("storage_unavailable"))?;
            let stamp = [old.0, old.1, restored.0, restored.1, sampled]
                .into_iter().max().unwrap().checked_add(1)
                .ok_or(Error::new("storage_unavailable"))?;
            c.execute("UPDATE col SET scm=?1,mod=?1 WHERE id=1", [stamp])?;
            schema::checkpoint(&c)?;
            c.pragma_update(None, "journal_mode", "DELETE")?;
            drop(c);
            generation.durable()?;
            Ok((generation, resources.0, resources.1))
        }).await?;
        self.pause("backup_restore_before_commit").await;
        let guard = gate.enter().await;
        #[cfg(feature = "test-hooks")]
        let hooks = self.hooks.clone();
        self.metadata.files(move |root| {
            let _publication = (gate.clone(), guard);
            let _work = (work, reservations, capacity);
            let mut c = crate::storage::open(root.database(), false)?;
            let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            admission(&tx)?;
            check_browser(&tx, &authority, "authentication_failed")?;
            eligible(&tx, &id, safety.epoch)?;
            if root.collection_path(&id)? != safety.path
                || index::cursor(&tx, &id)? != safety.cursor
                || safety.source.pragma_query_value(None, "data_version", |r| r.get::<_, i64>(0))? != safety.version
            {
                return Err(Error::new("busy"));
            }
            // Refuse cursor exhaustion before intent or pointer changes.
            let changed = safety.media.keys().chain(selected_media.keys())
                .collect::<std::collections::BTreeSet<_>>().into_iter()
                .filter(|name| safety.media.get(*name) != selected_media.get(*name)).count();
            safety.cursor.checked_add(
                i32::try_from(changed).map_err(|_| Error::new("storage_unavailable"))?,
            ).ok_or(Error::new("storage_unavailable"))?;
            store::point(&tx, &id, &backup)?;
            store::insert(&tx, &id, &safety.point, &safety.media, safety.epoch)?;
            // Protect the selected point even when it is an older safety point.
            store::prune(&tx, &id, now(), &[&backup, &safety.point.summary.id])?;
            let epoch = safety.epoch.checked_add(1).ok_or(Error::new("storage_unavailable"))?;
            let job = store::Intent {
                account: id.clone(),
                backup,
                generation: generation.name().into(),
                epoch,
            };
            gate.retire()?;
            tx.execute(
                "INSERT INTO account_data_epochs VALUES(?1,?2) ON CONFLICT(account_id) DO UPDATE SET epoch=excluded.epoch",
                params![id, epoch],
            )?;
            tx.execute(
                "INSERT INTO backup_restores VALUES(?1,?2,?3,?4)",
                params![id, job.backup, job.generation, epoch],
            )?;
            // Keep before commit: an uncertain commit must retain the candidate.
            // Exclusive startup cleanup reclaims it if no intent was committed.
            phase("backup_restore_before_intent")?;
            generation.retain();
            tx.commit()?;
            phase("backup_restore_after_intent")?;
            drop(c);
            drop(safety);
            root.finish_backup_restore(&job)?;
            #[cfg(feature = "test-hooks")]
            tokio::runtime::Handle::current().block_on(hooks.hit("backup_restore_before_cleanup"));
            if root.cleanup_collection_generations(&id).is_err() {
                eprintln!("backup phase=restore_cleanup outcome=deferred");
            }
            #[cfg(feature = "test-hooks")]
            tokio::runtime::Handle::current().block_on(hooks.hit("backup_restore_cleanup"));
            // Keep publication and the backup lease through cleanup: reset
            // and deletion cannot invalidate the account check before unlink.
            let cleanup = (|| {
                let c = crate::storage::open(root.database(), false)?;
                eligible(&c, &id, job.epoch)?;
                root.cleanup_backups(&c, &id)
            })();
            if cleanup.is_err() {
                eprintln!("backup phase=restore_cleanup outcome=deferred");
            }
            Ok(id)
        }).await
    }
    pub(crate) async fn recover_backup_restores(&self) -> Result<()> {
        let jobs=self.metadata.run(|c| {let jobs=store::intents(c)?;if admission(c).is_ok() {Ok(jobs)} else {Ok(Vec::new())}}).await?;
        for job in jobs {
            let gate=self.barriers.get(&job.account)?;
            let guard=gate.enter().await;
            self.metadata.files(move |root| {let _publication=(gate,guard);root.finish_backup_restore(&job)}).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::barrier::Barriers;
    #[cfg(feature = "test-hooks")]
    #[tokio::test]
    async fn restore_file_preparation_keeps_all_eight_metadata_permits_available() {
        let temp = tempfile::tempdir().unwrap();
        let mut root = crate::DataRoot::acquire(&temp.path().join("owner"), true).unwrap();
        root.init().unwrap();
        let token = std::fs::read_to_string(root.setup_file()).unwrap();
        let service = Service::open(
            root, crate::Config::new("https://example.test", "127.0.0.1:0".parse().unwrap()).unwrap(),
        ).await.unwrap();
        let ip = "127.0.0.1".parse().unwrap();
        let password = "synthetic password 123";
        let account = service.claim(ip, "setup", token.trim().into(), "restore.member", password.into()).await.unwrap();
        service.capture_account_backup_at(account.id, now()).await.unwrap();
        let (_, token) = service.browser_login(ip, "restore.member", password.into()).await.unwrap();
        let actor = service.browser_authority(&token).await.unwrap();
        let history = service.backups(actor.clone(), None).await.unwrap();
        let backup = history.backups[0].id.clone();
        let pause = service.hooks.arm("backup_restore_preparing");
        struct ResumeOnDrop(Arc<crate::test_hooks::Pause>);
        impl Drop for ResumeOnDrop {
            fn drop(&mut self) { self.0.resume(); }
        }
        let _release = ResumeOnDrop(pause.clone());
        let owner = service.clone();
        let pending = tokio::spawn(async move { owner.restore_backup(actor, backup).await });
        tokio::time::timeout(std::time::Duration::from_secs(30), pause.arrived()).await.unwrap();
        // try_acquire of all eight fails immediately if heavy preparation owns
        // even one metadata permit. No elapsed-time or overload assumption.
        let permits = service.metadata.test_hold_capacity();
        pause.resume();
        drop(permits);
        tokio::time::timeout(std::time::Duration::from_secs(30), pending).await.unwrap().unwrap().unwrap();
    }
    struct CandidateDrop {
        barriers: Arc<Barriers>,
        gate: std::sync::Weak<PublicationBarrier>,
        dropped: Option<tokio::sync::oneshot::Sender<()>>,
    }
    impl Drop for CandidateDrop {
        fn drop(&mut self) {
            let original=self.gate.upgrade().expect("candidate retains strong gate through Drop");
            let current=self.barriers.get("account").unwrap();
            assert!(Arc::ptr_eq(&original,&current));
            assert!(BackupWork::acquire(current).is_err(),"candidate cleanup still owns backup lock");
            self.dropped.take().unwrap().send(()).unwrap();
        }
    }
    #[tokio::test]
    async fn cancelled_blocking_candidate_keeps_the_same_gate_until_its_cleanup_finishes() {
        let barriers=Arc::new(Barriers::default());
        let gate=barriers.get("account").unwrap();
        let weak=Arc::downgrade(&gate);
        let work=BackupWork::acquire(gate).unwrap();
        let (dropped_tx,dropped_rx)=tokio::sync::oneshot::channel();
        let candidate=Candidate {
            value:CandidateDrop {barriers:barriers.clone(),gate:weak.clone(),dropped:Some(dropped_tx)},
            _work:work,
        };
        let (started_tx,started_rx)=tokio::sync::oneshot::channel();
        let (release_tx,release_rx)=std::sync::mpsc::channel();
        let pending=tokio::spawn(async move {
            tokio::task::spawn_blocking(move || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                candidate
            }).await
        });
        tokio::time::timeout(std::time::Duration::from_secs(30),started_rx).await.unwrap().unwrap();
        pending.abort();assert!(pending.await.err().expect("cancelled waiter").is_cancelled());
        {
            let original=weak.upgrade().unwrap();
            let current=barriers.get("account").unwrap();
            assert!(Arc::ptr_eq(&original,&current));
            assert!(BackupWork::acquire(current).is_err());
        }
        release_tx.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(30),dropped_rx).await.unwrap().unwrap();
    }
}
