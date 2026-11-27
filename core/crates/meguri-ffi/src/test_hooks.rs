//! Test-only exports (KTD18, R1967), compiled only with the `test-hooks`
//! feature: Debug and Profile builds, never Release or AppStore. CoreBridge
//! names them only in DEBUG inspection helpers; the MeguriCore Swift tests
//! also call them through generated bindings. Like every export they exist
//! on every slice of a test-hooks configuration.

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use meguri_engine::{EventKind, Generation};

use crate::cancel::CancelToken;
use crate::error::CoreError;
use crate::events::{self, CoreEventKind};
use crate::jobs::{self, JobHandle, JobOutcome};
use crate::{callback, changes, runtime};

/// Returns `error`, so Swift can check how each error crosses the bridge.
#[uniffi::export]
pub fn test_return_error(error: CoreError) -> Result<(), CoreError> {
    callback::enter("test_return_error");
    Err(error)
}

/// Panics with `message`; UniFFI catches it and Swift gets an internal error.
#[uniffi::export]
pub fn test_panic(message: String) -> Result<(), CoreError> {
    callback::enter("test_panic");
    panic!("{message}");
}

/// Sleeps in 1 ms steps until `token` is cancelled (`Err(Cancelled)`) or
/// `max_iterations` steps have passed (`Ok(steps)`).
#[uniffi::export]
pub fn test_cancellable_loop(token: Arc<CancelToken>, max_iterations: u64) -> Result<u64, CoreError> {
    callback::enter("test_cancellable_loop");
    for _ in 0..max_iterations {
        token.check()?;
        thread::sleep(Duration::from_millis(1));
    }
    Ok(max_iterations)
}

/// Publishes `per_producer` `JobFinished` events (ids `(producer + 1) *
/// 1_000_000 + index`, above any real job id a test reaches; outcome
/// `Succeeded`) from each of `producers` threads named
/// `meguri-test-producer-<n>`, and returns once all are queued.
#[uniffi::export]
pub fn test_emit_events(producers: u32, per_producer: u32) -> Result<(), CoreError> {
    callback::enter("test_emit_events");
    let threads: Vec<_> = (0..producers)
        .map(|producer| {
            thread::Builder::new()
                .name(format!("meguri-test-producer-{producer}"))
                .spawn(move || -> Result<(), CoreError> {
                    for index in 0..per_producer {
                        let id = (u64::from(producer) + 1) * 1_000_000 + u64::from(index);
                        events::publish(
                            0,
                            None,
                            CoreEventKind::JobFinished {
                                id,
                                outcome: JobOutcome::Succeeded,
                            },
                        )?;
                    }
                    Ok(())
                })
                .map_err(|e| CoreError::Internal {
                    message: format!("cannot start a producer: {e}"),
                })
        })
        .collect::<Result<_, _>>()?;
    for producer in threads {
        producer.join().map_err(|p| CoreError::from_panic(p.as_ref()))??;
    }
    Ok(())
}

/// Publishes one `Changes` event with `changes::sample()`'s change set
/// (every list filled; its values are listed there), as the engine's
/// executor would, at generation 0, and returns its `seq`.
#[uniffi::export]
pub fn test_emit_changes() -> Result<u64, CoreError> {
    callback::enter("test_emit_changes");
    let stream = runtime::engine()?.stream();
    Ok(stream
        .publish(Generation(0), None, EventKind::Changes(Arc::new(changes::sample())))
        .0)
}

/// Starts a job that reports progress `reports_per_second` times a second for
/// `duration_ms` (or until cancelled), then ends with `fail_with` or success.
#[uniffi::export]
pub fn test_start_job(
    duration_ms: u64,
    reports_per_second: u32,
    fail_with: Option<CoreError>,
) -> Result<JobHandle, CoreError> {
    callback::enter("test_start_job");
    let step = Duration::from_secs(1) / reports_per_second.max(1);
    let steps =
        u64::try_from(Duration::from_millis(duration_ms).as_nanos() / step.as_nanos().max(1)).map_err(|_| {
            CoreError::InvalidArgument {
                message: "duration too long".into(),
            }
        })?;
    // Batch high-rate test reports to avoid per-report sub-ms sleep rounding in the VM.
    // Keep nominal pacing (including the final sleep); production throttling is unchanged.
    let batch_size = (Duration::from_millis(5).as_nanos() / step.as_nanos().max(1)).clamp(1, 64) as u32;
    jobs::start("test", 0, move |ctx| {
        let mut done = 0;
        while done < steps {
            let batch_len = (steps - done).min(u64::from(batch_size)) as u32;
            for _ in 0..batch_len {
                ctx.cancel.check()?;
                ctx.progress.report(done, Some(steps));
                done += 1;
            }
            thread::sleep(step * batch_len);
        }
        fail_with.map_or(Ok(()), Err)
    })
}

/// Synthetic empty collection for exported-service tests; no production opener.
#[uniffi::export]
pub fn test_bootstrap_collection(location: crate::collection::CollectionLocation) -> Result<(), CoreError> {
    callback::enter("test_bootstrap_collection");
    let clock = meguri_model::clock::FixedClock::new(1_894_701_600_000, meguri_model::clock::UtcOffset::UTC);
    meguri_engine::bootstrap::create(
        &std::path::Path::new(&location.collection_directory).join("collection.anki2"),
        &clock,
        &meguri_model::rng::SeededRandom::new(37),
    )
    .map_err(CoreError::from)
}

/// Wrap an integration fixture's original Host at its current generation.
/// Not a UniFFI export; private owner fields and production admission stay intact.
pub fn provider_owner_for_testing(host: Arc<meguri_host::CoreHost>) -> Result<Arc<crate::provider::ProviderOwner>, CoreError> {
    crate::provider::owner_for_testing(host)
}

/// Rust writer-gate fixtures use the runtime owned by the real exports.
/// This helper is not a UniFFI export and adds no generated measurement API.
pub fn collection_for_testing() -> Result<meguri_engine::Collection, CoreError> {
    runtime::host()?.collection().map_err(Into::into)
}

/// Rust integration fixtures share the exports' engine, slot and maintenance
/// drain. Not a UniFFI export; never opens a second process engine.
pub fn engine_for_testing() -> Result<meguri_engine::Engine, CoreError> {
    runtime::engine().cloned()
}

// Swift cannot arm Rust function-pointer faults. This narrow KTD18 adapter
// exposes the existing restore boundaries, never a replacement result or event.
#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum MaintenanceTestBoundary {
    BeforeRename,
    AfterActivation,
}
#[derive(Default)]
struct MaintenanceGateState {
    reached: bool,
    released: bool,
}
#[derive(uniffi::Object)]
pub struct MaintenanceTestGate {
    state: std::sync::Mutex<MaintenanceGateState>,
    changed: std::sync::Condvar,
    point: &'static str,
    cleared: std::sync::atomic::AtomicBool,
}
static MAINTENANCE_GATE: std::sync::Mutex<Option<std::sync::Weak<MaintenanceTestGate>>> = std::sync::Mutex::new(None);
fn maintenance_boundary() {
    let gate = MAINTENANCE_GATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .and_then(std::sync::Weak::upgrade);
    if let Some(gate) = gate {
        let mut state = gate.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        state.reached = true;
        gate.changed.notify_all();
        drop(
            gate.changed
                .wait_timeout_while(state, Duration::from_secs(20), |s| !s.released)
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
    }
}
#[uniffi::export]
pub fn test_arm_maintenance_boundary(
    boundary: MaintenanceTestBoundary,
    fail_reevaluation: bool,
) -> Result<Arc<MaintenanceTestGate>, CoreError> {
    callback::enter("test_arm_maintenance_boundary");
    use meguri_model::faultpoint::{self, Fault};
    let mut active = MAINTENANCE_GATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if active.as_ref().and_then(std::sync::Weak::upgrade).is_some() {
        return Err(CoreError::Busy {
            running_op: "maintenance fault fixture".into(),
        });
    }
    let point = match boundary {
        MaintenanceTestBoundary::BeforeRename => "replace.after_check",
        MaintenanceTestBoundary::AfterActivation => "replace.after_dir_sync",
    };
    let gate = Arc::new(MaintenanceTestGate {
        state: std::sync::Mutex::new(MaintenanceGateState::default()),
        changed: std::sync::Condvar::new(),
        point,
        cleared: std::sync::atomic::AtomicBool::new(false),
    });
    *active = Some(Arc::downgrade(&gate));
    drop(active);
    faultpoint::arm(point, Fault::Callback(maintenance_boundary));
    if fail_reevaluation {
        faultpoint::arm(
            "restore.before_maintenance_reevaluation",
            Fault::IoError(std::io::ErrorKind::Other),
        );
    }
    Ok(gate)
}
#[uniffi::export]
impl MaintenanceTestGate {
    /// The test waits on the query lane; release/cancel remain available on control.
    pub fn wait_until_reached(&self) -> Result<(), CoreError> {
        callback::enter("MaintenanceTestGate.wait_until_reached");
        let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (state, _) = self
            .changed
            .wait_timeout_while(state, Duration::from_secs(20), |s| !s.reached)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.reached {
            Ok(())
        } else {
            Err(CoreError::Internal {
                message: "maintenance boundary was not reached".into(),
            })
        }
    }
    pub fn release(&self) -> Result<(), CoreError> {
        callback::enter("MaintenanceTestGate.release");
        self.unblock();
        Ok(())
    }
    /// Call after drain so the injected reevaluation error is not cleared early.
    pub fn reset(&self) -> Result<(), CoreError> {
        callback::enter("MaintenanceTestGate.reset");
        self.clear();
        Ok(())
    }
}
impl MaintenanceTestGate {
    fn unblock(&self) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .released = true;
        self.changed.notify_all();
    }
    fn clear(&self) {
        self.unblock();
        if self.cleared.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        meguri_model::faultpoint::disarm(self.point);
        meguri_model::faultpoint::disarm("restore.before_maintenance_reevaluation");
        MAINTENANCE_GATE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }
}
impl Drop for MaintenanceTestGate {
    fn drop(&mut self) {
        self.clear();
    }
}

/// Read the real notes b-tree location and flush WAL before the caller closes
/// publicly and damages the on-disk page. This supplies layout, never a verdict.
#[uniffi::export]
pub fn test_maintenance_notes_page_offset() -> Result<u64, CoreError> {
    callback::enter("test_maintenance_notes_page_offset");
    use meguri_storage::{ReaderRole, raw::SqlValue};
    let collection = runtime::host()?.collection()?;
    let reader = collection.reader(ReaderRole::Query)?;
    let integer = |sql: &str| -> Result<i64, CoreError> {
        let rows = reader.execute_raw(sql, &[])?;
        match rows.rows.first().and_then(|row| row.first()) {
            Some(SqlValue::Integer(value)) => Ok(*value),
            _ => Err(CoreError::Internal {
                message: "expected SQLite page metadata".into(),
            }),
        }
    };
    let root = integer("SELECT rootpage FROM sqlite_master WHERE name='notes'")?;
    let size = integer("PRAGMA page_size")?;
    drop(reader);
    let offset = root
        .checked_sub(1)
        .filter(|page| *page > 0)
        .and_then(|page| page.checked_mul(size))
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(|| CoreError::Internal {
            message: "invalid notes page location".into(),
        })?;
    collection
        .submit_maintenance(
            meguri_engine::executor::catalog::ANALYZE,
            Duration::from_secs(1),
            |writer| Ok(writer.execute_raw("PRAGMA wal_checkpoint(TRUNCATE)", &[])?),
        )
        .wait()?;
    Ok(offset)
}

/// Fixed integrity facts from one owned Query snapshot; no caller-selected query.
#[derive(Clone, Debug, uniffi::Record)]
pub struct SmokeIntegrityObservation {
    pub generation: u64,
    pub seq: u64,
    pub integrity_ok: bool,
    pub schema_ok: bool,
    pub reference_failures: u64,
}

#[uniffi::export]
pub fn test_smoke_integrity() -> Result<SmokeIntegrityObservation, CoreError> {
    callback::enter("test_smoke_integrity");
    let collection = collection_for_testing()?;
    let lease = collection.reader(meguri_storage::ReaderRole::Query)?;
    let snapshot = lease.snapshot()?;
    let r = snapshot.smoke_integrity()?;
    Ok(SmokeIntegrityObservation {
        generation: snapshot.generation().0,
        seq: snapshot.seq().0,
        integrity_ok: r.integrity_ok,
        schema_ok: r.schema_ok,
        reference_failures: r.missing_card_notes
            + r.missing_card_decks
            + r.missing_note_types
            + r.missing_field_types
            + r.wrong_field_counts,
    })
}

/// Observe the actual answered card and history entry through one Query snapshot.
#[uniffi::export]
pub fn test_physical_review_rows(
    card_id: i64,
    revlog_id: i64,
) -> Result<crate::physical_review_rows::PhysicalReviewRows, CoreError> {
    callback::enter("test_physical_review_rows");
    crate::physical_review_rows::observe(&runtime::host()?.collection()?, card_id, revlog_id)
}

/// Writes the open collection, without media, as a latest-format package at
/// `path`. Under a deck package's name it is a deck package of every note:
/// the Swift tests import it into another collection.
#[uniffi::export]
pub fn test_write_deck_package(path: String) -> Result<(), CoreError> {
    callback::enter("test_write_deck_package");
    use meguri_package::{CollectionPackageWriteContext, Finalisation, write_collection_package_from_live};
    let host = runtime::host()?;
    let context = host.deck_package_context();
    let collection = host.collection()?;
    let lease = collection.reader(meguri_storage::ReaderRole::Maintenance)?;
    write_collection_package_from_live(
        &lease,
        std::path::Path::new(&path),
        &CollectionPackageWriteContext {
            collection_directory: &context.collection_directory,
            purpose: meguri_storage::atomic_file::Purpose::Export,
            finalisation: Finalisation::Replace,
            clock: context.clock.as_ref(),
            random: context.random.as_ref(),
            space: context.space.as_ref(),
            is_cancelled: &|| false,
        },
    )
    .map_err(|error| CoreError::Internal { message: format!("test package: {error}") })?;
    Ok(())
}
