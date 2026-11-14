//! Read-only, bounded export of process evidence. No provider, tracking I/O,
//! policy evaluation, payload, identifier or SDK text enters this projection.
use super::Runtime;
use crate::cloud::zone_deletion::{Diagnostic, FailureFacts};

pub const DELETION_LIMIT: usize = 128;
pub const ROOT_READ_LIMIT: usize = 64;
pub const RESULT_LIMIT: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootOutcome { Found, UnknownItem, ZoneNotFound, Failed, Other }
#[derive(Clone, Debug)]
pub struct RootRead {
    pub active: bool,
    pub lost_collection: bool,
    pub outcome: RootOutcome,
    pub a7_failure: bool,
    pub accepted: bool,
    pub failure: Option<FailureFacts>,
    pub terminal: Option<FailureFacts>,
}
#[derive(Clone, Debug)]
pub enum Deletion {
    Read { found: bool, allowed: u64, failure: Option<FailureFacts>, terminal: Option<FailureFacts> },
    Deleted { results: Vec<Option<FailureFacts>>, results_omitted_count: u64, terminal: Option<FailureFacts> },
}
#[derive(Clone, Debug)]
pub struct StatusEvidence {
    pub scan_required: bool,
    pub scan_running: bool,
    pub whole_scan_complete: bool,
    pub capture_present: bool,
    pub note_scan_present: bool,
    pub creation_time_known: bool,
    pub creation_time_matches: bool,
    pub tracking_stopped: bool,
}
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub missing_did_fetch_events: u64,
    pub root_reads: Vec<RootRead>,
    pub root_reads_omitted_count: u64,
    pub zone_deletions: Vec<Deletion>,
    pub zone_deletions_omitted_count: u64,
    pub status_evidence: Option<StatusEvidence>,
}

impl Runtime {
    /// One lock, no policy or provider calls. Caps precede every vector
    /// allocation/copy. Legacy inspection retains its original complete values.
    pub fn diagnostics_export(&self) -> Snapshot {
        // Immutable evidence only: mutation guards must keep publishing native
        // authority, but this read must neither acquire nor replace authority.
        let guard = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = &*guard;
        let reads = &state.adapter.root_reads;
        let deletions = &state.adapter.zone_deletions;
        Snapshot {
            missing_did_fetch_events: state.adapter.missing_did,
            root_reads_omitted_count: reads.len().saturating_sub(ROOT_READ_LIMIT) as u64,
            root_reads: reads.iter().skip(reads.len().saturating_sub(ROOT_READ_LIMIT)).map(|read| RootRead {
                active: read.active,
                lost_collection: read.lost_collection,
                outcome: match read.outcome.as_str() {
                    "found" => RootOutcome::Found,
                    "unknownItem" => RootOutcome::UnknownItem,
                    "zoneNotFound" => RootOutcome::ZoneNotFound,
                    "failed" => RootOutcome::Failed,
                    _ => RootOutcome::Other,
                },
                // Original qualification is evidence; never infer A7 here.
                a7_failure: read.a7_failure,
                accepted: read.accepted,
                failure: read.failure.clone(),
                terminal: read.terminal.clone(),
            }).collect(),
            zone_deletions_omitted_count: deletions.len().saturating_sub(DELETION_LIMIT) as u64,
            zone_deletions: deletions.iter().skip(deletions.len().saturating_sub(DELETION_LIMIT)).map(|value| match value {
                Diagnostic::Read { found, allowed, failure, terminal } => Deletion::Read {
                    found: *found, allowed: *allowed as u64, failure: failure.clone(), terminal: terminal.clone(),
                },
                Diagnostic::Deleted { results, terminal } => Deletion::Deleted {
                    results: results.iter().take(RESULT_LIMIT).map(|result| result.failure.clone()).collect(),
                    results_omitted_count: results.len().saturating_sub(RESULT_LIMIT) as u64,
                    terminal: terminal.clone(),
                },
            }).collect(),
            status_evidence: state.bound.as_ref().map(|bound| StatusEvidence {
                scan_required: state.scan_required,
                scan_running: state.scan_running,
                whole_scan_complete: state.whole_complete,
                capture_present: state.last_capture.is_some(),
                note_scan_present: state.note_scan.is_some(),
                creation_time_known: bound.epoch_crt.is_some(),
                creation_time_matches: bound.epoch_crt == Some(bound.current_crt.1),
                tracking_stopped: state.tracking_stopped,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::{adapter::RootReadDiagnostic, runtime::*, *};
    use std::sync::Arc;

    struct NoProviders;
    impl IdentityProvider for NoProviders {
        fn read_identity(&self, _: &BuildScope) -> IdentityStatus { panic!("export must not query identity") }
    }
    impl FileIdentityProvider for NoProviders {
        fn identity(&self, _: &std::path::Path) -> Result<NativeFileIdentity, RuntimeError> { panic!("export must not query files") }
    }
    #[test]
    fn export_never_acquires_or_publishes_native_authority() {
        let runtime = Runtime::new(std::path::PathBuf::new(), CloudInputs {
            scope: BuildScope { container: ContainerId::new("synthetic-read-only-export").unwrap(), environment: Environment::Development },
            identity: Arc::new(NoProviders), files: Arc::new(NoProviders),
        });
        let mut authority = runtime.native_authority.lock().unwrap();
        // Deliberately different from State: even equivalent publication is
        // forbidden. Holding this mutex also detects authority-lock contention.
        authority.instance = Some("retained-authority-canary".into());
        let instance = authority.instance.clone();
        std::thread::scope(|scope| {
            let (tx, rx) = std::sync::mpsc::channel();
            let runtime = &runtime;
            let export = scope.spawn(move || { tx.send(runtime.diagnostics_export()).unwrap(); });
            let result = rx.recv_timeout(std::time::Duration::from_secs(5));
            assert_eq!(authority.instance, instance);
            // Release before joining even on regression, so a failed assertion
            // reports an error instead of leaving a blocked test worker.
            drop(authority);
            export.join().unwrap();
            assert!(result.is_ok(), "export must complete with authority locked");
        });
        assert_eq!(runtime.native_authority.lock().unwrap().instance, instance);
    }
    #[test]
    fn original_result_history_is_bounded_before_bridge_and_legacy_history_is_unchanged() {
        let runtime = Runtime::new(std::path::PathBuf::new(), CloudInputs {
            scope: BuildScope { container: ContainerId::new("synthetic-diagnostics").unwrap(), environment: Environment::Development },
            identity: Arc::new(NoProviders), files: Arc::new(NoProviders),
        });
        let original = (0..101).map(|index| crate::cloud::zone_deletion::ZoneResult {
            zone: format!("private-zone-{index}"),
            failure: Some(FailureFacts { code: Some(index), retry_after_ms: Some(index + 1), encrypted_data_reset: index == 26 }),
        }).collect::<Vec<_>>();
        runtime.record_zone_deletion_diagnostic(Diagnostic::Deleted { results: original.clone(), terminal: None });
        {
            let mut state = runtime.state();
            for index in 0..100 {
                state.adapter.root_reads.push(RootReadDiagnostic {
                    active: index % 3 == 0, lost_collection: false,
                    outcome: if index == 99 { "private-outcome-canary" } else { "found" }.into(),
                    a7_failure: index % 3 == 1, accepted: true, failure: None, terminal: None,
                });
            }
        }
        let before = runtime.zone_deletion_diagnostics();
        let reads_before = runtime.adapter_root_read_diagnostics();
        let snapshot = runtime.diagnostics_export();
        assert_eq!(snapshot.root_reads.len(), ROOT_READ_LIMIT);
        assert_eq!(snapshot.root_reads_omitted_count, 36);
        assert_eq!(snapshot.root_reads.last().unwrap().outcome, RootOutcome::Other);
        for (projected, original) in snapshot.root_reads.iter().zip(&reads_before[36..]) {
            assert_eq!(projected.a7_failure, original.a7_failure);
            assert!(!projected.a7_failure || (!projected.active && projected.accepted && projected.outcome == RootOutcome::Found));
        }
        let Deletion::Deleted { results, results_omitted_count, .. } = &snapshot.zone_deletions[0] else { panic!("deleted evidence required") };
        assert_eq!(results.len(), RESULT_LIMIT);
        assert_eq!(*results_omitted_count, 37);
        assert_eq!(results[63], original[63].failure);
        assert_eq!(runtime.zone_deletion_diagnostics(), before);
        assert_eq!(runtime.adapter_root_read_diagnostics(), reads_before);
        let Diagnostic::Deleted { results, .. } = &before[0] else { panic!("original history required") };
        assert_eq!(results, &original);
        assert_eq!(snapshot.zone_deletions_omitted_count, 0);
        assert!(snapshot.status_evidence.is_none());
    }
}
