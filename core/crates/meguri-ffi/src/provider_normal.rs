//! Retained ProviderConnection facade; no supplied result or readiness proof.
use super::{ProviderConnection, ProviderCredentials, ProviderOwner};
use crate::{CoreError, callback, jobs};
use meguri_host::{MeguriAccess, MeguriCollectionState, MeguriRoundStart, MeguriRoundTask};
use meguri_sync::{ErrorKind, HostKey, JobError, provider::MeguriRoundRefusal as R};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ProviderRoundRefusal {
    NotSelected, Paused, NotReady, BindingChanged, ConnectionChanged,
    AccountChanged, PublicationChanged, Busy, Interrupted, Storage,
}
impl From<R> for ProviderRoundRefusal {
    fn from(value: R) -> Self { match value {
        R::NotSelected => Self::NotSelected, R::Paused => Self::Paused, R::NotReady => Self::NotReady,
        R::BindingChanged => Self::BindingChanged, R::ConnectionChanged => Self::ConnectionChanged,
        R::AccountChanged => Self::AccountChanged, R::PublicationChanged => Self::PublicationChanged,
        R::Busy => Self::Busy, R::Interrupted => Self::Interrupted, R::Storage => Self::Storage,
    } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ProviderRoundRecovery { BindingChanged, PublicationChanged, Storage, Unavailable }
impl From<meguri_host::MeguriRoundRecovery> for ProviderRoundRecovery {
    fn from(value: meguri_host::MeguriRoundRecovery) -> Self { use meguri_host::MeguriRoundRecovery as V; match value {
        V::BindingChanged => Self::BindingChanged, V::PublicationChanged => Self::PublicationChanged,
        V::Storage => Self::Storage, V::Unavailable => Self::Unavailable,
    } }
}
/// Static category/method and scalar protocol parameters only. Server text,
/// payloads, filenames, credentials and unrestricted error prose stay in Core.
#[derive(Clone, uniffi::Record)]
pub struct ProviderRoundFailure {
    pub kind: String, pub method: Option<String>, pub status: Option<u16>,
    pub clock_offset_seconds: Option<i64>, pub database_check_is_local: Option<bool>,
}
fn failure(error: &JobError) -> ProviderRoundFailure {
    let status = match error.kind {
        ErrorKind::ServerError { code } | ErrorKind::UnexpectedStatus { code } | ErrorKind::RedirectRefused { code } => Some(code),
        _ => None,
    };
    ProviderRoundFailure { kind: error.kind.id().into(), method: error.method_name().map(str::to_owned), status,
        clock_offset_seconds: match error.kind { ErrorKind::ClockIncorrect { offset_secs } => Some(offset_secs), _ => None },
        database_check_is_local: match error.kind { ErrorKind::DbCheckRequired { origin } => Some(origin == meguri_sync::DbCheckOrigin::Local), _ => None } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ProviderCollectionRoundState { NoChanges, Merged, FullSyncRequired, Failed }
#[derive(Clone, uniffi::Record)]
pub struct ProviderCollectionRound {
    pub state: ProviderCollectionRoundState, pub failure: Option<ProviderRoundFailure>,
    pub refusal: Option<ProviderRoundRefusal>, pub acceptance_unknown: bool, pub skipped: u64,
    pub recovery: Option<ProviderRoundRecovery>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ProviderAffectedMediaKind { Empty, OverLimit, InvalidName, NotRegularFile, Pending, Awaiting }
#[derive(Clone, uniffi::Record)]
pub struct ProviderAffectedMedia { pub name: String, pub kind: ProviderAffectedMediaKind }
#[derive(Clone, uniffi::Record)]
pub struct ProviderMediaRound {
    pub complete: bool, pub failure: Option<ProviderRoundFailure>, pub refusal: Option<ProviderRoundRefusal>,
    pub recovery: Option<ProviderRoundRecovery>, pub uploaded_files: u64, pub uploaded_deletions: u64,
    pub downloaded_files: u64, pub downloaded_deletions: u64, pub bytes_uploaded: u64, pub bytes_downloaded: u64,
    pub checked: u64, pub resynced: bool, pub excluded: Option<u64>, pub pending: Option<u64>,
    pub awaiting: Option<u64>, pub affected: Vec<ProviderAffectedMedia>, pub accounting_failure: Option<ProviderRoundFailure>,
}
#[derive(Clone, uniffi::Enum)]
pub enum ProviderRoundOutcome {
    Pending, Collection { result: ProviderCollectionRound }, Media { result: ProviderMediaRound },
}
fn outcome(result: &meguri_host::MeguriRoundResult) -> ProviderRoundOutcome {
    if let Some(collection) = &result.collection {
        return ProviderRoundOutcome::Collection { result: ProviderCollectionRound {
            state: match collection.state { MeguriCollectionState::NoChanges => ProviderCollectionRoundState::NoChanges,
                MeguriCollectionState::Merged => ProviderCollectionRoundState::Merged,
                MeguriCollectionState::FullSyncRequired => ProviderCollectionRoundState::FullSyncRequired,
                MeguriCollectionState::Failed => ProviderCollectionRoundState::Failed },
            failure: collection.error.as_ref().map(failure), refusal: collection.refusal.map(Into::into),
            acceptance_unknown: collection.acceptance_unknown, skipped: collection.skipped, recovery: result.recovery.map(Into::into) } };
    }
    if let Some(media) = &result.media {
        let m = &media.effects;
        let affected = media.accounting.as_ref().map(|a| a.affected.iter().map(|file| {
            use meguri_sync::media::AffectedMediaKind as K;
            ProviderAffectedMedia { name: file.name.clone(), kind: match file.kind {
                K::Empty => ProviderAffectedMediaKind::Empty, K::OverLimit => ProviderAffectedMediaKind::OverLimit,
                K::InvalidName => ProviderAffectedMediaKind::InvalidName, K::NotRegularFile => ProviderAffectedMediaKind::NotRegularFile,
                K::Pending => ProviderAffectedMediaKind::Pending, K::Awaiting => ProviderAffectedMediaKind::Awaiting,
            } }
        }).collect()).unwrap_or_default();
        return ProviderRoundOutcome::Media { result: ProviderMediaRound { complete: media.complete,
            failure: media.failure.as_ref().map(|value| failure(&value.error)), refusal: media.refusal.map(Into::into),
            recovery: result.recovery.map(Into::into), uploaded_files: m.uploaded_files, uploaded_deletions: m.uploaded_deletions,
            downloaded_files: m.downloaded_files, downloaded_deletions: m.downloaded_deletions,
            bytes_uploaded: m.bytes_uploaded, bytes_downloaded: m.bytes_downloaded, checked: m.checked, resynced: m.resynced,
            excluded: media.accounting.as_ref().map(|a| a.excluded), pending: media.accounting.as_ref().map(|a| a.pending),
            awaiting: media.accounting.as_ref().map(|a| a.awaiting), affected,
            accounting_failure: media.accounting_error.as_ref().map(failure) } };
    }
    ProviderRoundOutcome::Pending
}
#[derive(uniffi::Object)]
pub struct ProviderRoundResult { owner: Arc<ProviderOwner>, task: Arc<MeguriRoundTask> }
#[derive(Clone, uniffi::Enum)]
pub enum ProviderRoundStart {
    Started { handle: jobs::JobHandle, result: Arc<ProviderRoundResult> },
    Refused { refusal: ProviderRoundRefusal },
}
#[uniffi::export]
impl ProviderRoundResult {
    pub fn outcome(&self) -> Result<ProviderRoundOutcome, CoreError> {
        callback::enter("ProviderRoundResult.outcome");
        // Results belong to the original task even after close/handoff. A new
        // current Host is never resolved in order to read an old result.
        self.owner.host.meguri_round_result(&self.task).map(|value|
            value.as_ref().map_or(ProviderRoundOutcome::Pending, |value| outcome(value)))
            .map_err(|_| CoreError::Unavailable { reason: "Meguri original result owner changed".into() })
    }
    pub fn retry_finalization(&self) -> Result<ProviderRoundOutcome, CoreError> {
        callback::enter("ProviderRoundResult.retry_finalization");
        self.owner.host.retry_meguri_round_finalization(&self.task).map(|value|
            value.as_ref().map_or(ProviderRoundOutcome::Pending, |value| outcome(value)))
            .map_err(|_| CoreError::Unavailable { reason: "Meguri original result owner changed".into() })
    }

}
impl ProviderConnection {
    fn round_access(&self, credentials: ProviderCredentials) -> Result<MeguriAccess, ProviderRoundRefusal> {
        if self.owner.validate().is_err() { return Err(ProviderRoundRefusal::Interrupted); }
        let c = &credentials.connection;
        if c.reference != self.inner.reference || c.revision != self.inner.revision || c.endpoint != self.inner.endpoint {
            return Err(ProviderRoundRefusal::ConnectionChanged);
        }
        #[cfg(feature = "test-hooks")]
        let access = self.owner.host.meguri_access_for_testing(self.inner.clone(), &credentials.username, HostKey::new(credentials.hostkey));
        #[cfg(not(feature = "test-hooks"))]
        let access = MeguriAccess::new(self.inner.clone(), &credentials.username, HostKey::new(credentials.hostkey));
        access.map_err(|error| match error {
                meguri_host::MeguriBootstrapError::Username => ProviderRoundRefusal::AccountChanged,
                _ => ProviderRoundRefusal::ConnectionChanged,
            })
    }
    fn round_start(&self, start: MeguriRoundStart) -> ProviderRoundStart {
        match start {
            MeguriRoundStart::Started(task) => ProviderRoundStart::Started { handle: jobs::wrap(task.handle()),
                result: Arc::new(ProviderRoundResult { owner: self.owner.clone(), task }) },
            MeguriRoundStart::Refused(refusal) => ProviderRoundStart::Refused { refusal: refusal.into() },
        }
    }
}
#[uniffi::export]
impl ProviderConnection {
    pub fn start_collection_round(&self, credentials: ProviderCredentials) -> Result<ProviderRoundStart, CoreError> {
        callback::enter("ProviderConnection.start_collection_round");
        Ok(match self.round_access(credentials) {
            Ok(access) => self.round_start(self.owner.host.start_meguri_collection(access)),
            Err(refusal) => ProviderRoundStart::Refused { refusal },
        })
    }
    pub fn start_media_round(&self, credentials: ProviderCredentials) -> Result<ProviderRoundStart, CoreError> {
        callback::enter("ProviderConnection.start_media_round");
        Ok(match self.round_access(credentials) {
            Ok(access) => self.round_start(self.owner.host.start_meguri_media(access)),
            Err(refusal) => ProviderRoundStart::Refused { refusal },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use meguri_sync::Method;
    fn connection(root: &std::path::Path) -> ProviderConnection {
        use meguri_model::{clock::{FixedClock, UtcOffset}, rng::SeededRandom};
        let directory = root.join("collection"); std::fs::create_dir_all(&directory).unwrap();
        let config = meguri_host::HostConfig::new(directory, root.join("backups"), root.join("widget"),
            Arc::new(FixedClock::new(1_894_701_600_000, UtcOffset::UTC)), Arc::new(SeededRandom::new(88)),
            Arc::new(meguri_storage::space::StatvfsSpace));
        meguri_engine::bootstrap::create(&config.collection_directory.join("collection.anki2"), config.clock.as_ref(), config.random.as_ref()).unwrap();
        let host = Arc::new(meguri_host::CoreHost::open(config).unwrap());
        let generation = host.cloud_connection_generation().unwrap();
        let inner = host.configure_meguri_connection("https://routine.synthetic.example/").unwrap();
        ProviderConnection { owner: Arc::new(ProviderOwner { host, generation }), inner }
    }
    #[test]
    fn original_owner_and_all_credential_connection_fields_are_checked_before_start() {
        let root = tempfile::tempdir().unwrap(); let connection = connection(root.path());
        for field in 0..3 {
            let mut credentials = ProviderCredentials { connection: super::super::information(&connection.inner),
                username: "synthetic".into(), hostkey: "synthetic-key".into() };
            match field { 0 => credentials.connection.reference.push('x'), 1 => credentials.connection.revision += 1,
                _ => credentials.connection.endpoint = "https://foreign.synthetic.example/".into() }
            assert!(matches!(connection.start_collection_round(credentials.clone()).unwrap(),
                ProviderRoundStart::Refused { refusal: ProviderRoundRefusal::ConnectionChanged }));
            assert!(matches!(connection.start_media_round(credentials).unwrap(),
                ProviderRoundStart::Refused { refusal: ProviderRoundRefusal::ConnectionChanged }));
        }
        connection.owner.host.close().unwrap();
        let credentials = ProviderCredentials { connection: super::super::information(&connection.inner), username: "synthetic".into(), hostkey: "synthetic".into() };
        assert!(matches!(connection.start_collection_round(credentials).unwrap(),
            ProviderRoundStart::Refused { refusal: ProviderRoundRefusal::Interrupted }));
    }
    #[test]
    fn real_error_categories_stay_distinct_and_never_export_payload_prose() {
        for kind in [ErrorKind::AuthFailed, ErrorKind::Offline, ErrorKind::Network { head_received: true },
            ErrorKind::Conflict, ErrorKind::BadResponse, ErrorKind::LocalStorage, ErrorKind::MediaResyncFailed,
            ErrorKind::Cancelled, ErrorKind::Interrupted, ErrorKind::ServerMessage { text: "private-payload".into() }] {
            let id = kind.id(); let converted = failure(&JobError::new(kind, Some(Method::MediaBegin), "private-payload"));
            assert_eq!(converted.kind, id); assert_eq!(converted.method.as_deref(), Some("msync/begin"));
            assert!(!converted.kind.contains("private-payload"));
        }
    }
}
