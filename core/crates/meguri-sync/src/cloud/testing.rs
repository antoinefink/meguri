//! Fixed, feature-only observation and transaction interruption boundaries.
use super::Error;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, PoisonError};

/// Fixed refusal facts from one synchronous preparation call. Handoff is an
/// observation at denial, not proof of which provider predicate denied it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimePreparationRefusal {
    MissingBinding,
    ProviderAdmissionDenied { handoff: HandoffObservation },
    PublishingBoundUsabilityRefused,
    ReconcileUsabilityRefused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoffObservation {
    Present,
    Absent,
    Unknown,
}

thread_local! {
    // Outer None means there is no collector. No process history or identity.
    static PREPARATION_REFUSAL: std::cell::Cell<Option<Option<RuntimePreparationRefusal>>> = const { std::cell::Cell::new(None) };
}

/// Installs a fresh calling-thread collector; restores its predecessor even
/// when the original operation unwinds. The closure stays entirely in Rust.
pub fn observe_runtime_preparation<R>(call: impl FnOnce() -> R) -> (R, Option<RuntimePreparationRefusal>) {
    struct Restore(Option<Option<RuntimePreparationRefusal>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            PREPARATION_REFUSAL.with(|slot| slot.set(self.0));
        }
    }
    let _restore = Restore(PREPARATION_REFUSAL.with(|slot| slot.replace(Some(None))));
    let result = call();
    let observation = PREPARATION_REFUSAL.with(|slot| slot.get().flatten());
    (result, observation)
}

pub(super) fn record_preparation_refusal(refusal: RuntimePreparationRefusal) {
    PREPARATION_REFUSAL.with(|slot| {
        if slot.get() == Some(None) {
            slot.set(Some(Some(refusal)));
        }
    });
}

pub(super) fn preparation_refusal_pending() -> bool {
    PREPARATION_REFUSAL.with(|slot| slot.get() == Some(None))
}

#[cfg(test)]
mod preparation_refusal_tests {
    use super::{RuntimePreparationRefusal::*, *};

    #[test]
    fn no_active_scope_and_first_refusal_wins() {
        record_preparation_refusal(MissingBinding);
        assert_eq!(PREPARATION_REFUSAL.with(|slot| slot.get()), None);
        let (_, observed) = observe_runtime_preparation(|| {
            record_preparation_refusal(ReconcileUsabilityRefused);
            record_preparation_refusal(MissingBinding);
        });
        assert_eq!(observed, Some(ReconcileUsabilityRefused));
        assert_eq!(observe_runtime_preparation(|| ()).1, None);
    }

    #[test]
    fn nested_scope_restores_its_predecessor() {
        let (_, outer) = observe_runtime_preparation(|| {
            record_preparation_refusal(MissingBinding);
            let (_, inner) = observe_runtime_preparation(|| {
                record_preparation_refusal(PublishingBoundUsabilityRefused);
            });
            assert_eq!(inner, Some(PublishingBoundUsabilityRefused));
        });
        assert_eq!(outer, Some(MissingBinding));
        assert_eq!(PREPARATION_REFUSAL.with(|slot| slot.get()), None);
    }

    #[test]
    fn scope_is_thread_local() {
        let (_, outer) = observe_runtime_preparation(|| {
            record_preparation_refusal(MissingBinding);
            std::thread::spawn(|| {
                assert_eq!(PREPARATION_REFUSAL.with(|slot| slot.get()), None);
                let (_, inner) = observe_runtime_preparation(|| {
                    record_preparation_refusal(ReconcileUsabilityRefused);
                });
                assert_eq!(inner, Some(ReconcileUsabilityRefused));
            }).join().unwrap();
        });
        assert_eq!(outer, Some(MissingBinding));
    }

    #[test]
    fn unwind_restores_scope_and_does_not_leak_a_refusal() {
        let (_, outer) = observe_runtime_preparation(|| {
            record_preparation_refusal(MissingBinding);
            let caught = std::panic::catch_unwind(|| observe_runtime_preparation(|| {
                record_preparation_refusal(ReconcileUsabilityRefused);
                panic!("collector unwind");
            }));
            assert!(caught.is_err());
        });
        assert_eq!(outer, Some(MissingBinding));
        assert_eq!(PREPARATION_REFUSAL.with(|slot| slot.get()), None);
        assert_eq!(observe_runtime_preparation(|| ()).1, None);
    }
}

/// Control state for child orchestration, without entity image bodies.
#[derive(Clone, Serialize, Deserialize)]
pub struct PublicationControl {
    pub current: Option<super::Binding>,
    pub recovery_ops: Vec<super::adoption::RecoveryOpFacts>,
    pub zones: std::collections::BTreeMap<String, super::publication::RetainedZone>,
}
impl std::fmt::Debug for PublicationControl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PublicationControl")
    }
}

/// Scalar-only observation of the real collection-drain cleanup boundary.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CleanupObservation {
    pub sdk_current: bool,
    pub drain_present: bool,
    pub drain_sdk_matches: bool,
    pub drain_guard_matches: bool,
    pub drain_at_ms: Option<i64>,
    pub cached_now_ms: i64,
    pub media_observed_now_ms: i64,
    pub last_cleanup_ms: Option<i64>,
    pub root_confirmed: bool,
    pub gate_open: bool,
    pub publication_sending: bool,
    pub media_enabled: Option<bool>,
    pub media_paused: bool,
    pub media_root_checked: bool,
    pub ledger_cleanup_drain_ms: Option<i64>,
    pub zone_missing: bool,
    pub blobs: Vec<CleanupBlobObservation>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CleanupBlobObservation {
    pub server: super::media::ServerState,
    pub upload: super::media::Transfer,
    pub download: super::media::Transfer,
    pub staged: bool,
    pub unreferenced_since_ms: Option<i64>,
    pub deletion_due_ms: Option<i64>,
}

/// Cumulative workflow payload body materializations on the calling thread.
pub fn entity_body_reads() -> usize {
    super::workflow_store::body_reads()
}

/// Calling-thread row queries, SQLite steps, owner decodes and classification
/// semantic validations. Reading resets the existing work counters.
pub fn take_row_read_work() -> (u64, u64, u64, u64) {
    super::row_classification::take_read_work()
}

thread_local! {
    static MEDIA_POLICY_BLOB_CANDIDATE_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Calling-thread BlobState row visits by policy media metadata matching only:
/// each inspected linear-search candidate, or each index-build row and selected
/// indexed candidate. Excludes BTreeMap key comparisons, entity decoding and
/// all other Runtime work; this is not a total-runtime cost measurement.
pub fn media_policy_blob_candidate_visits() -> usize {
    MEDIA_POLICY_BLOB_CANDIDATE_VISITS.with(std::cell::Cell::get)
}

pub fn reset_media_policy_blob_candidate_visits() {
    MEDIA_POLICY_BLOB_CANDIDATE_VISITS.with(|v| v.set(0));
}

pub(super) fn media_policy_blob_candidate_visit() {
    MEDIA_POLICY_BLOB_CANDIDATE_VISITS.with(|v| v.set(v.get().saturating_add(1)));
}

/// Body materializations in the last receipt's fully validated Saved ACK loop.
/// Reset at receipt entry; None unless its entire Saved loop succeeds on this thread.
/// Excludes receipt prevalidation, conflict handling and pending emission.
pub fn saved_ack_body_reads() -> Option<usize> {
    super::workflow_store::saved_ack_body_reads()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum IncomingStage {
    AccountIsolationPublished,
    SDKIsolationPublished,
    SDKRetireBeforeReacquisition,
    PolicyMediaPrepared,
    PolicyPendingUpdated,
    PolicyApplyWriterAdmission,
    // These effect witnesses run under State; token/atomic observation only.
    PolicyMediaSelected,
    PolicyFetchBegun,
    PolicySendRetryCleared,
    AssetReadPassEffect,
    // Outside State/tracking ownership, before FFI activity-guard acquisition.
    AssetReadPassCreated,
    AssetReadPassTrackingContended,
    // After the original pass guard is retained, outside State/tracking.
    AssetReadNextGuarded,
    AssetReadNextTrackingContended,
    // Selection boundaries under State/tracking: atomics/barriers only.
    AssetReadNextScan,
    AssetReadNextScanned,
    AssetReadNextEffect,
    AssetReadNextSelected,
    // After State/tracking release, before FFI stores the actual result.
    AssetReadNextReturned,
    AdoptionIsolationPublished,
    AssetReadSubmitEdge,
    AssetReadSubmitAccounting,
    AssetReadCompleteBody,
    AssetReadCancelBody,
    AssetReadFinishBody,
    AssetReadPrepared,
    AssetReadFinishing,
    FetchBeforeInbox,
    FetchAfterInbox,
    FetchAfterCursor,
    KnownLossBeforeAcceptance,
    KnownLossAfterAcceptance,
    KnownLossBeforeCacheSweep,
    KnownLossAfterCacheSweep,
    KnownLossBeforeCursor,
    KnownLossAfterCursor,
    KnownLossBeforeActiveResult,
    KnownLossAfterActiveResult,
    // After ordinary/rekey selection, off the writer and all core locks.
    ApplyBeforeWriter,
    ApplyAfterIntent,
    ApplyAfterCommit,
    ApplyAfterFlush,
    IncomingRollbackComplete,
    BeforeCollectionSave,
    IntentSettlementComplete,
    ResolutionBeforeMove,
    ResolutionMoveBeforeRewrite,
    ResolutionRewriteCommitted,
    MediaTrashBeforeInstall,
    MediaReplyBeforeApply,
    MediaReplyBeforeTracking,
    MediaSettlementBeforeCommit,
    MediaLedgerCommitted,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApplyInterruption {
    Cancel,
    Error,
    Constraint,
    Panic,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackingFault {
    Io,
    NoSpace,
    Constraint,
    Panic,
}
pub(super) type StageHook = Arc<dyn Fn(IncomingStage) + Send + Sync>;
#[derive(Default)]
pub(super) struct Hooks {
    pub(super) capture_asset_submit_reader: std::sync::atomic::AtomicBool,
    pub(super) retire_readback_failure: std::sync::atomic::AtomicBool,
    pub(super) media_readback_failure: std::sync::atomic::AtomicBool,
    pub(super) stage: Mutex<Option<StageHook>>,
    pub(super) interruption: Mutex<Option<ApplyInterruption>>,
}
impl Hooks {
    pub(super) fn hit(&self, stage: IncomingStage) {
        let hook = self.stage.lock().unwrap_or_else(PoisonError::into_inner).clone();
        if let Some(hook) = hook {
            hook(stage);
        }
    }
    pub(super) fn interrupt(
        &self,
        ctx: &mut meguri_engine::OpContext<'_, '_>,
    ) -> Result<(), meguri_engine::EngineError> {
        use meguri_storage::{Cell, ReadRows, RowKey, Table};
        let interruption = self.interruption.lock().unwrap_or_else(PoisonError::into_inner).take();
        match interruption {
            None => Ok(()),
            Some(ApplyInterruption::Cancel) => Err(meguri_engine::EngineError::Cancelled),
            Some(ApplyInterruption::Error) => Err(meguri_engine::EngineError::Internal {
                message: "incoming interruption".into(),
            }),
            Some(ApplyInterruption::Panic) => panic!("incoming writer interruption"),
            Some(ApplyInterruption::Constraint) => {
                let mut row = ctx
                    .scope()
                    .row_page(Table::Notes, None)?
                    .into_iter()
                    .next()
                    .ok_or_else(|| meguri_engine::EngineError::Internal {
                        message: "constraint target absent".into(),
                    })?;
                let key = match row.first() {
                    Some(Cell::Integer(id)) => RowKey::Id(*id),
                    _ => {
                        return Err(meguri_engine::EngineError::Internal {
                            message: "invalid constraint target".into(),
                        });
                    }
                };
                row[1] = Cell::Null;
                ctx.scope().restore(Table::Notes, &key, Some(&row))?;
                Err(meguri_engine::EngineError::Internal {
                    message: "constraint was not enforced".into(),
                })
            }
        }
    }
}
// The subprocess owns one Runtime. Faults are consumed at the real transaction
// boundary, never at a kill-only point or in the parent's receive model.
static TRACKING_FAULT: Mutex<Option<(String, TrackingFault)>> = Mutex::new(None);
pub fn arm_tracking_fault(name: &str, fault: TrackingFault) -> Result<(), Error> {
    if !matches!(
        name,
        "cloud.tracking.commit.inbox"
            | "cloud.tracking.commit.asset_read"
            | "cloud.tracking.commit.engine_state"
            | "cloud.tracking.commit.intent"
            | "cloud.tracking.commit.rollback"
            | "cloud.publication.ack_control"
            | "cloud.media.commit.ledger"
    ) {
        return Err(Error::InvalidData);
    }
    *TRACKING_FAULT.lock().unwrap_or_else(PoisonError::into_inner) = Some((name.into(), fault));
    Ok(())
}
pub(super) fn tracking_commit(tx: &rusqlite::Transaction<'_>, name: &str) -> Result<(), Error> {
    let fault = {
        let mut armed = TRACKING_FAULT.lock().unwrap_or_else(PoisonError::into_inner);
        if armed.as_ref().is_some_and(|(point, _)| point == name) {
            armed.take().map(|(_, fault)| fault)
        } else {
            None
        }
    };
    match fault {
        None => Ok(()),
        Some(TrackingFault::Io) => Err(Error::Io(std::io::ErrorKind::Other)),
        Some(TrackingFault::NoSpace) => Err(Error::Io(std::io::ErrorKind::StorageFull)),
        Some(TrackingFault::Panic) => panic!("tracking transaction interruption"),
        Some(TrackingFault::Constraint) => {
            tx.execute_batch(
                "CREATE TEMP TABLE IF NOT EXISTS meguri_tracking_constraint(value INTEGER CHECK(value=0))",
            )?;
            tx.execute("INSERT INTO meguri_tracking_constraint VALUES (1)", [])?;
            Err(Error::InvalidData)
        }
    }
}
