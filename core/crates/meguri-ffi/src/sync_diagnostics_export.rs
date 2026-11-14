//! Safe diagnostics use the current host only. They do not construct a workflow,
//! retain inspection, open a collection or invoke any account/provider port.
use crate::{CoreError, callback, cloud_zone_deletion::CloudZoneDeletionFailure, runtime};
use meguri_sync::cloud::diagnostics_export as d;

#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum CloudDiagnosticRootOutcome { Found, UnknownItem, ZoneNotFound, Failed, Other }
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudDiagnosticRootRead {
    pub active: bool,
    pub lost_collection: bool,
    pub outcome: CloudDiagnosticRootOutcome,
    pub a7_failure: bool,
    pub accepted: bool,
    pub failure: Option<CloudZoneDeletionFailure>,
    pub terminal: Option<CloudZoneDeletionFailure>,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudDiagnosticResult { pub failure: Option<CloudZoneDeletionFailure> }
#[derive(Clone, Debug, uniffi::Enum)]
pub enum CloudDiagnosticDeletion {
    Read { found: bool, allowed: u64, failure: Option<CloudZoneDeletionFailure>, terminal: Option<CloudZoneDeletionFailure> },
    Deleted { results: Vec<CloudDiagnosticResult>, results_omitted_count: u64, terminal: Option<CloudZoneDeletionFailure> },
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudDiagnosticStatusEvidence {
    pub scan_required: bool,
    pub scan_running: bool,
    pub whole_scan_complete: bool,
    pub capture_present: bool,
    pub note_scan_present: bool,
    pub creation_time_known: bool,
    pub creation_time_matches: bool,
    pub tracking_stopped: bool,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudDiagnosticSnapshot {
    pub missing_did_fetch_events: u64,
    pub root_reads: Vec<CloudDiagnosticRootRead>,
    pub root_reads_omitted_count: u64,
    pub zone_deletions: Vec<CloudDiagnosticDeletion>,
    pub zone_deletions_omitted_count: u64,
    pub status_evidence: Option<CloudDiagnosticStatusEvidence>,
}
impl From<d::Snapshot> for CloudDiagnosticSnapshot {
    fn from(value: d::Snapshot) -> Self {
        Self {
            missing_did_fetch_events: value.missing_did_fetch_events,
            root_reads_omitted_count: value.root_reads_omitted_count,
            root_reads: value.root_reads.into_iter().map(|v| CloudDiagnosticRootRead {
                active: v.active, lost_collection: v.lost_collection,
                outcome: match v.outcome {
                    d::RootOutcome::Found => CloudDiagnosticRootOutcome::Found,
                    d::RootOutcome::UnknownItem => CloudDiagnosticRootOutcome::UnknownItem,
                    d::RootOutcome::ZoneNotFound => CloudDiagnosticRootOutcome::ZoneNotFound,
                    d::RootOutcome::Failed => CloudDiagnosticRootOutcome::Failed,
                    d::RootOutcome::Other => CloudDiagnosticRootOutcome::Other,
                },
                a7_failure: v.a7_failure, accepted: v.accepted,
                failure: v.failure.map(Into::into), terminal: v.terminal.map(Into::into),
            }).collect(),
            zone_deletions_omitted_count: value.zone_deletions_omitted_count,
            zone_deletions: value.zone_deletions.into_iter().map(|v| match v {
                d::Deletion::Read { found, allowed, failure, terminal } => CloudDiagnosticDeletion::Read {
                    found, allowed, failure: failure.map(Into::into), terminal: terminal.map(Into::into),
                },
                d::Deletion::Deleted { results, results_omitted_count, terminal } => CloudDiagnosticDeletion::Deleted {
                    results: results.into_iter().map(|failure| CloudDiagnosticResult { failure: failure.map(Into::into) }).collect(),
                    results_omitted_count, terminal: terminal.map(Into::into),
                },
            }).collect(),
            status_evidence: value.status_evidence.map(|v| CloudDiagnosticStatusEvidence {
                scan_required: v.scan_required, scan_running: v.scan_running, whole_scan_complete: v.whole_scan_complete,
                capture_present: v.capture_present, note_scan_present: v.note_scan_present,
                creation_time_known: v.creation_time_known, creation_time_matches: v.creation_time_matches,
                tracking_stopped: v.tracking_stopped,
            }),
        }
    }
}
/// None is unavailable, never an empty successful history. All caps have already
/// been applied inside Rust before this record is lowered into native vectors.
#[uniffi::export]
pub fn cloud_diagnostics_export() -> Result<Option<CloudDiagnosticSnapshot>, CoreError> {
    callback::enter("cloud_diagnostics_export");
    let Some(host) = runtime::retained_host() else { return Ok(None) };
    let Some(core) = host.cloud_runtime() else { return Ok(None) };
    Ok(Some(project(core)))
}
fn project(core: &meguri_sync::cloud::runtime::Runtime) -> CloudDiagnosticSnapshot {
    core.diagnostics_export().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use meguri_sync::cloud::{runtime::*, zone_deletion::*, *};
    use std::{path::{Path, PathBuf}, sync::Arc};
    struct NoProviders;
    impl IdentityProvider for NoProviders {
        fn read_identity(&self, _: &BuildScope) -> IdentityStatus { panic!("diagnostics queried identity") }
    }
    impl FileIdentityProvider for NoProviders {
        fn identity(&self, _: &Path) -> Result<NativeFileIdentity, RuntimeError> { panic!("diagnostics queried files") }
    }
    #[test]
    fn no_host_export_does_not_initialize_process_core() {
        const CHILD: &str = "MEGURI_DIAGNOSTICS_FRESH_PROCESS";
        if std::env::var_os(CHILD).is_some() {
            assert!(!crate::runtime::process_initialized_for_testing());
            assert!(cloud_diagnostics_export().unwrap().is_none());
            // Runtime::new creates ProcessCore (workers) and Dispatcher together.
            // No runtime means none of those or their callbacks/providers ran.
            assert!(!crate::runtime::process_initialized_for_testing());
            return;
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "sync::diagnostics_export::tests::no_host_export_does_not_initialize_process_core", "--nocapture"])
            .env(CHILD, "1")
            .output().unwrap();
        assert!(output.status.success(), "{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"), "fresh-process test must actually run");
    }
    #[test]
    fn production_projection_bounds_real_original_result_history_before_ffi_lowering() {
        let core = Runtime::new(PathBuf::new(), CloudInputs {
            scope: BuildScope { container: ContainerId::new("synthetic-export-bridge").unwrap(), environment: Environment::Development },
            identity: Arc::new(NoProviders), files: Arc::new(NoProviders),
        });
        let original = (0..101).map(|index| ZoneResult {
            zone: format!("private-zone-{index}"),
            failure: Some(FailureFacts { code: Some(index), retry_after_ms: Some(2 * index), encrypted_data_reset: index == 26 }),
        }).collect::<Vec<_>>();
        core.record_zone_deletion_diagnostic(Diagnostic::Deleted { results: original.clone(), terminal: None });
        let bounded = core.diagnostics_export();
        let d::Deletion::Deleted { results, results_omitted_count, .. } = &bounded.zone_deletions[0] else { panic!("deleted required") };
        assert_eq!(results.len(), 64, "bounded before FFI conversion");
        assert_eq!(*results_omitted_count, 37);
        let ffi = project(&core);
        let CloudDiagnosticDeletion::Deleted { results, results_omitted_count, terminal } = &ffi.zone_deletions[0] else { panic!("deleted required") };
        assert_eq!(results.len(), 64);
        assert_eq!(*results_omitted_count, 37);
        assert!(terminal.is_none());
        assert_eq!(results[63].failure.as_ref().unwrap().code, Some(63));
        let Diagnostic::Deleted { results, .. } = &core.zone_deletion_diagnostics()[0] else { panic!("original history required") };
        assert_eq!(results, &original);
    }
}
