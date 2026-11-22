//! `sync/meguri-provider#MP6.3` and `#MP6.9`: when Meguri Sync setup may run
//! with no question, and how an unknown acceptance is settled. Every decision
//! is computed here from rows and retained artifacts; the retained transfer
//! code still owns each request, backup and replacement.
use crate::{CoreHost, ManualTransferTask, MeguriAccess, MeguriBootstrapDirection, MeguriBootstrapError};
use meguri_engine::{
    CancelToken, Collection, EngineError,
    ports::{
        BackupProtection, BackupReceipt, BackupService, GuardedBackup, GuardedTransferBackup, TransferBackup,
    },
};
use meguri_storage::{
    ReaderRole,
    media::MediaStore,
    one_way::{ContentWitness, UserContentRows},
};
use meguri_sync::{
    Job,
    oneway::{Preview, TransferError},
    policy::{
        AccountRef, Acceptance, CopyKind, Direction, LocalFacts, Operation, OperationId, Phase,
        RemoteFacts, UploadSettlement,
    },
    provider::{MeguriConnection, MeguriReadyRefusal, canonical_username, native_endpoint},
};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

type Result<T> = std::result::Result<T, MeguriBootstrapError>;

/// The row of MP6.3's table that the two sides give.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeguriSetupRow {
    /// Row 1: no collection on this device.
    Download,
    /// Row 2: this device's collection holds no user content.
    DownloadOverEmptyDevice,
    /// Row 3: the server is proved to hold no user content, on a first binding.
    UploadToEmptyServer,
    /// Row 4: the user chooses which collection to keep.
    Ask,
    /// A changed endpoint or account with an empty server: upload or Cancel.
    AskUploadOnly,
    /// No collection on this device, but media files, or a media folder that
    /// cannot be read: download or Cancel. The media reconciliation that
    /// follows a download touches those files, so row 1 does not apply.
    AskDownloadOnly,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeguriSetupChoice {
    /// Rows 1 to 3. Refused when the core's own row needs an answer.
    NoQuestion,
    KeepDevice,
    KeepServer,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeguriSetupChange {
    /// This device's collection is not the one the decision saw.
    Local,
    /// The server's collection is not the one the decision saw.
    Remote,
    /// The choice does not fit the row that the facts give now.
    Choice,
}
/// Counts of one side, for the sheet. `media_files` and `last_change_ms` are
/// absent where no fact gives them: the server reports no media count, and
/// serving a download rewrites the server's own modification time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeguriSetupSide {
    pub notes: u64,
    pub cards: u64,
    pub reviews: u64,
    pub media_files: Option<u64>,
    pub last_change_ms: Option<i64>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeguriSetupDecision {
    pub row: MeguriSetupRow,
    pub device: Option<MeguriSetupSide>,
    pub server: Option<MeguriSetupSide>,
    /// This device holds changes that were never sent.
    pub unsent: bool,
    operation: OperationId,
    revision: u64,
    local: Option<LocalFacts>,
    served: Option<LocalFacts>,
    served_rows: Option<UserContentRows>,
    served_content: Option<ServedContent>,
}
/// What the retained server copy holds, for comparing a later download with
/// it. Read through the package's own upgrade for a legacy schema, so it is
/// never a proof of emptiness: `served_rows` alone is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ServedContent {
    rows: UserContentRows,
    last_review: Option<i64>,
    /// Absent for a legacy schema, whose upgrade may write it.
    schema_modified: Option<i64>,
}
pub enum MeguriSetupSettlement {
    /// The exact acceptance is recorded. `local_unchanged` is a hint read
    /// outside any exclusive phase; only
    /// [`CoreHost::confirm_meguri_setup_activation`] decides whether local
    /// activation may run with no question.
    Accepted { operation: Operation, local_unchanged: bool },
    /// Nothing reached the server; the exclusion is released.
    NotAccepted { operation: Operation },
    /// The server holds something else. The upload is recorded as overtaken,
    /// its artifacts are kept, the exclusion is released, and the next
    /// preparation gives the sheet (never rows 2 or 3).
    Neither { operation: Operation },
}

/// What one confirmation tests again inside the replacement's exclusive phase.
// At most one value exists at a time, armed for one confirmation: its size costs nothing.
#[allow(clippy::large_enum_variant)]
pub(crate) enum Retest {
    /// Row 2: the collection about to be replaced still holds no user content.
    NoUserContent(Option<MediaStore>),
    /// Row 1: a device with no collection still has no media file.
    NoDeviceMedia(PathBuf),
    /// MP6.9 accepted: the collection about to be replaced is still the one the upload sent.
    Unchanged(ContentWitness),
}

/// Armed for one confirmation; consumed by its safety backup, which the
/// retained replacement takes inside its exclusive phase.
#[derive(Default)]
pub(crate) struct SetupRetest {
    armed: Mutex<Option<Retest>>,
}
impl SetupRetest {
    pub(crate) fn arm(&self, retest: Retest) {
        *self.armed.lock().unwrap_or_else(PoisonError::into_inner) = Some(retest);
    }
    pub(crate) fn disarm(&self) {
        *self.armed.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
    fn take(&self) -> Option<Retest> {
        self.armed.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}
/// The transfers' backup port. It forwards every call; while armed it first
/// tests the side about to be replaced, and refuses the backup, and so the
/// activation, when that side is not what the decision was made on.
pub(crate) struct RetestingBackup {
    inner: Arc<dyn BackupService>,
    retest: Arc<SetupRetest>,
}
impl RetestingBackup {
    pub(crate) fn new(inner: Arc<dyn BackupService>, retest: Arc<SetupRetest>) -> Self {
        Self { inner, retest }
    }
}
impl BackupService for RetestingBackup {
    fn before_deck_delete(&self) -> std::result::Result<(), EngineError> {
        self.inner.before_deck_delete()
    }
    fn before_restore(
        &self,
        collection: Option<&Collection>,
        damaged: bool,
        cancel: &CancelToken,
    ) -> std::result::Result<GuardedTransferBackup, EngineError> {
        self.inner.before_restore(collection, damaged, cancel)
    }
    fn backup_live(
        &self,
        collection: &Collection,
        cancel: &CancelToken,
    ) -> std::result::Result<meguri_engine::maintenance::LiveBackup, EngineError> {
        self.inner.backup_live(collection, cancel)
    }
    fn before_safety_operation(
        &self,
        collection: &Collection,
        cancel: &CancelToken,
    ) -> std::result::Result<GuardedBackup, EngineError> {
        self.inner.before_safety_operation(collection, cancel)
    }
    fn protect_backup(&self, name: &str) -> std::result::Result<Option<Arc<dyn BackupProtection>>, EngineError> {
        self.inner.protect_backup(name)
    }
    fn before_manual_transfer(
        &self,
        collection: Option<&Collection>,
        cancel: &CancelToken,
    ) -> std::result::Result<TransferBackup, EngineError> {
        if let Some(retest) = self.retest.take() {
            let holds = match (&retest, collection) {
                (Retest::NoUserContent(media), Some(collection)) => holds_no_user_content(collection, media.as_ref()),
                (Retest::NoDeviceMedia(folder), None) => folder_files(folder) == Some(0),
                (Retest::Unchanged(sent), Some(collection)) => collection
                    .reader(ReaderRole::Query)
                    .ok()
                    .and_then(|reader| reader.content_witness().ok())
                    .is_some_and(|now| &now == sent),
                // The side being replaced is not the side the decision saw.
                _ => false,
            };
            if !holds {
                // Typed as a local change: nothing is backed up or replaced,
                // and the caller returns to preparation or to the question.
                let generation = collection.map_or(meguri_engine::Generation(0), Collection::generation);
                return Err(EngineError::StaleGeneration {
                    planned: generation,
                    current: generation,
                });
            }
        }
        self.inner.before_manual_transfer(collection, cancel)
    }
    fn before_colpkg_import(
        &self,
        collection: &Collection,
        cancel: &CancelToken,
    ) -> std::result::Result<BackupReceipt, EngineError> {
        self.inner.before_colpkg_import(collection, cancel)
    }
}

/// V27.1 on the open collection. Anything unreadable is user content.
fn holds_no_user_content(collection: &Collection, media: Option<&MediaStore>) -> bool {
    let rows = collection
        .reader(ReaderRole::Query)
        .ok()
        .and_then(|reader| reader.user_content_rows().ok());
    rows.is_some_and(|rows| rows.none()) && media.is_some_and(|media| media_files(media) == Some(0))
}
/// Tracked files, and any regular file that is in the folder but not yet in
/// the journal. `None` when either cannot be read.
fn media_files(media: &MediaStore) -> Option<u64> {
    let tracked = media.tracked_names().ok()?.len() as u64;
    Some(tracked.max(folder_files(media.media_dir())?))
}
/// Entries of a media folder that are not directories. An absent folder holds
/// none; `None` when it cannot be read.
fn folder_files(folder: &Path) -> Option<u64> {
    let mut present = 0u64;
    match std::fs::read_dir(folder) {
        Ok(entries) => {
            for entry in entries {
                if !entry.ok()?.file_type().ok()?.is_dir() {
                    present += 1;
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return None,
    }
    Some(present)
}
fn plain_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0'])
}
fn side(facts: &LocalFacts, media_files: Option<u64>, last_change_ms: Option<i64>) -> MeguriSetupSide {
    MeguriSetupSide {
        notes: facts.notes,
        cards: facts.cards,
        reviews: facts.reviews,
        media_files,
        last_change_ms,
    }
}
/// A transfer that completed, or that may have reached a server: this device
/// has, or had, something in common with a server.
fn earlier_exchange(op: &Operation) -> bool {
    op.collection_complete
        || matches!(
            op.acceptance,
            Acceptance::PossiblyAccepted | Acceptance::ExactOk | Acceptance::Overtaken
        )
}

impl CoreHost {
    fn setup_account(&self, connection: &MeguriConnection, username: &str) -> Result<AccountRef> {
        let endpoint = native_endpoint(&connection.endpoint).map_err(|_| MeguriBootstrapError::ConnectionChanged)?;
        let username = canonical_username(username).map_err(|_| MeguriBootstrapError::Username)?;
        Ok(AccountRef::derive(&endpoint, &username).ok_or(MeguriReadyRefusal::AccountChanged)?)
    }
    fn setup_media(&self) -> Option<MediaStore> {
        self.media().ok().map(|media| media.store().clone())
    }
    /// Row test of the open collection, read at exactly the sequence the
    /// preview's guard pinned. `None`: changed since, or unreadable.
    fn device_rows_at(&self, operation: &Operation) -> Option<(UserContentRows, Option<u64>)> {
        let guard = operation.guard.as_ref()?;
        let collection = self.collection().ok()?;
        if collection.generation().0 != guard.generation {
            return None;
        }
        let reader = collection.reader(ReaderRole::Query).ok()?;
        let before = collection.transfer_admission().ok()?;
        let rows = reader.user_content_rows().ok()?;
        let after = collection.transfer_admission().ok()?;
        if before != after || before.pending || before.committed != guard.committed || before.admitted != guard.admitted
        {
            return None;
        }
        // The media service takes its own reader.
        drop(reader);
        Some((rows, self.setup_media().as_ref().and_then(media_files)))
    }
    /// The media folder of a device with no open collection.
    fn device_media_folder(&self) -> PathBuf {
        self.provider_collection_path.with_file_name("collection.media")
    }
    /// A collection file, or its write-ahead log, is on this device. An
    /// entry that cannot be examined counts as present.
    fn collection_file_present(&self) -> bool {
        ["collection.anki2", "collection.anki2-wal"].iter().any(|name| {
            !matches!(
                self.provider_collection_path.with_file_name(name).symlink_metadata(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound
            )
        })
    }
    /// What the retained server copy of an upload preview holds. `None`: an
    /// unverified copy, a changed file, a package that cannot be read.
    fn served_content(&self, operation: &Operation) -> Option<ServedContent> {
        let copy = operation.server_copy.as_ref()?;
        if copy.kind != CopyKind::VerifiedPackage || !plain_name(&copy.artifact.name) {
            return None;
        }
        let path = self.backup_inputs.backups_directory.join(&copy.artifact.name);
        if !copy.artifact.matches_file(&path) {
            return None;
        }
        let inputs = &self.backup_inputs;
        let context = meguri_package::CollectionPackageInspectContext {
            collection_directory: &inputs.collection_directory,
            live_collection_size: 0,
            reserved_space_bytes: 0,
            clock: inputs.clock.as_ref(),
            random: inputs.random.as_ref(),
            space: inputs.space.as_ref(),
            is_cancelled: &|| false,
        };
        let inspected = meguri_package::inspect_collection_package(&path, &context).ok()?;
        let collection = inspected.collection_path();
        let facts = meguri_storage::one_way::verify_server_copy(&collection).ok().flatten()?;
        let rows = meguri_storage::one_way::server_copy_user_content(&collection).ok().flatten()?;
        Some(ServedContent {
            rows,
            last_review: facts.last_review.map(|at| at.0),
            schema_modified: copy.served_facts.as_ref().map(|served| served.schema_modified),
        })
    }
    fn retained_candidate(&self, operation: &Operation) -> Option<PathBuf> {
        let candidate = operation.candidate.as_ref()?;
        if !plain_name(&candidate.name) || !plain_name(operation.id.as_str()) {
            return None;
        }
        let path = self
            .manual_transfer_directory
            .join(operation.id.as_str())
            .join(&candidate.name);
        candidate.matches_file(&path).then_some(path)
    }
    /// The preview must be this host's active, unconfirmed preparation for the
    /// signed-in account.
    fn check_setup_preview(&self, connection: &MeguriConnection, username: &str, preview: &Preview) -> Result<AccountRef> {
        self.check_meguri_bootstrap(connection)?;
        let account = self.setup_account(connection, username)?;
        let active = self.manual_transfer_history().active().map_err(|_| MeguriBootstrapError::Busy)?;
        let op = &preview.operation;
        if active.as_ref() != Some(op) || op.phase != Phase::Preview || op.acceptance != Acceptance::NotSent {
            return Err(MeguriBootstrapError::Busy);
        }
        if op.account != account || op.configured_endpoint != connection.endpoint {
            return Err(MeguriReadyRefusal::AccountChanged.into());
        }
        Ok(account)
    }

    /// MP6.3 preparation: the download with no collection on this device,
    /// otherwise the upload's, which retains the server copy and both sides'
    /// facts and changes nothing on either side.
    pub fn prepare_meguri_setup(&self, access: MeguriAccess) -> Result<ManualTransferTask<Preview>> {
        let direction = if self.collection().is_ok() {
            MeguriBootstrapDirection::UseDeviceCollection
        } else if self.collection_file_present() {
            // A collection that is closed or damaged is neither row 1's "no
            // collection" nor a collection this setup can read. No request is made.
            return Err(MeguriBootstrapError::LocalUnusable);
        } else {
            MeguriBootstrapDirection::UseServerCollection
        };
        self.prepare_meguri_bootstrap(access, direction)
    }

    /// The row for a finished preparation of [`Self::prepare_meguri_setup`].
    /// Reads only this device: rows, retained artifacts and local history.
    pub fn meguri_setup_decision(
        &self,
        connection: &MeguriConnection,
        username: &str,
        preview: &Preview,
    ) -> Result<MeguriSetupDecision> {
        // A download is decided only for a device with no collection file at
        // all. One that is present but not open is refused under its own name.
        if preview.operation.direction == Direction::Import && self.collection().is_err() && self.collection_file_present() {
            return Err(MeguriBootstrapError::LocalUnusable);
        }
        let account = self.check_setup_preview(connection, username, preview)?;
        let op = &preview.operation;
        let decision = |row, device, server, served_content: Option<ServedContent>| MeguriSetupDecision {
            row,
            device,
            server,
            unsent: op
                .local
                .as_ref()
                .is_some_and(|local| local.pending_reviews > 0 || local.pending_rows.iter().any(|n| *n > 0)),
            operation: op.id.clone(),
            revision: op.revision,
            local: op.local.clone(),
            served: op.server_copy.as_ref().and_then(|copy| copy.served_facts.clone()),
            // A legacy schema proves nothing about emptiness (MP6.3).
            served_rows: served_content
                .filter(|content| content.schema_modified.is_some())
                .map(|content| content.rows),
            served_content,
        };
        if op.direction == Direction::Import {
            // Row 1 only. A download prepared over a collection is judged by
            // `confirm_meguri_setup_download`.
            let absent = op.local.is_none()
                && op.actual_generation.is_none()
                && op.guard.as_ref().is_some_and(|guard| guard.closed_main.is_none() && guard.closed_wal.is_none())
                && self.collection().is_err();
            if !absent {
                return Err(if self.collection().is_err() && self.collection_file_present() {
                    MeguriBootstrapError::LocalUnusable
                } else {
                    MeguriBootstrapError::SetupChanged(MeguriSetupChange::Local)
                });
            }
            // Media files with no collection are still this device's content:
            // the reconciliation after a download can replace or delete them.
            let media = folder_files(&self.device_media_folder());
            if media == Some(0) {
                return Ok(decision(MeguriSetupRow::Download, None, None, None));
            }
            let device = MeguriSetupSide {
                notes: 0,
                cards: 0,
                reviews: 0,
                media_files: media,
                last_change_ms: None,
            };
            return Ok(decision(MeguriSetupRow::AskDownloadOnly, Some(device), None, None));
        }
        let history = self
            .manual_transfer_history()
            .history()
            .map_err(|_| MeguriBootstrapError::Busy)?;
        let earlier: Vec<&Operation> = history
            .iter()
            .filter(|other| other.id != op.id && earlier_exchange(other))
            .collect();
        let binding = self
            .provider_authority()
            .snapshot()
            .map_err(MeguriReadyRefusal::Storage)?
            .meguri_binding;
        // A first binding: no binding recorded, and no earlier exchange with
        // any server in this installation's retained history.
        let first = earlier.is_empty() && binding.is_none();
        let other_account = earlier.iter().any(|other| other.account != account)
            || binding
                .as_ref()
                .is_some_and(|binding| binding.account() != &account || binding.connection() != connection);
        let device = self.device_rows_at(op);
        // Unreadable, changed since the preview, or an unreadable media
        // inventory: all are user content.
        let device_empty = device.is_some_and(|(rows, media)| rows.none() && media == Some(0));
        let served_content = self.served_content(op);
        let server_empty = served_content
            .is_some_and(|content| content.schema_modified.is_some() && content.rows.none())
            && op.remote.as_ref().is_some_and(|remote| remote.media_usn == 0);
        let row = if first && device_empty {
            MeguriSetupRow::DownloadOverEmptyDevice
        } else if first && server_empty {
            MeguriSetupRow::UploadToEmptyServer
        } else if other_account && !device_empty && server_empty {
            MeguriSetupRow::AskUploadOnly
        } else {
            MeguriSetupRow::Ask
        };
        let device_side = op.local.as_ref().map(|facts| {
            side(facts, device.and_then(|(_, media)| media), Some(facts.modified))
        });
        let server_side = op
            .server_copy
            .as_ref()
            .and_then(|copy| copy.served_facts.as_ref())
            .map(|facts| {
                let none = op.remote.as_ref().is_some_and(|remote| remote.media_usn == 0);
                side(facts, none.then_some(0), None)
            });
        Ok(decision(row, device_side, server_side, served_content))
    }
    fn same_decision(&self, connection: &MeguriConnection, username: &str, preview: &Preview,
        shown: &MeguriSetupDecision) -> Result<()> {
        let fresh = self.meguri_setup_decision(connection, username, preview)?;
        if &fresh == shown {
            return Ok(());
        }
        Err(MeguriBootstrapError::SetupChanged(if fresh.local != shown.local || fresh.device != shown.device {
            MeguriSetupChange::Local
        } else if fresh.served != shown.served
            || fresh.served_content != shown.served_content
            || fresh.server != shown.server
        {
            MeguriSetupChange::Remote
        } else {
            MeguriSetupChange::Choice
        }))
    }

    /// Confirms the prepared direction itself: row 1's download, row 3's
    /// upload, or the sheet's "keep this device's collection". The row is
    /// computed again; a choice the facts no longer give is refused with
    /// nothing changed. The retained confirmation then checks this device
    /// again, and reads the server's facts again immediately before dispatch.
    pub fn confirm_meguri_setup(
        &self,
        access: MeguriAccess,
        shown: &MeguriSetupDecision,
        preview: Preview,
        choice: MeguriSetupChoice,
    ) -> Result<ManualTransferTask<Operation>> {
        self.same_decision(access.connection(), access.username(), &preview, shown)?;
        use MeguriSetupChoice as C;
        use MeguriSetupRow as R;
        match (shown.row, choice, preview.operation.direction) {
            // Row 1 tests the media folder again where the replacement holds
            // the collection directory.
            (R::Download, C::NoQuestion, Direction::Import) => {
                let folder = self.device_media_folder();
                self.confirm_meguri_bootstrap_retesting(access, preview, Some(Retest::NoDeviceMedia(folder)))
            }
            (R::AskDownloadOnly, C::KeepServer, Direction::Import)
            | (R::UploadToEmptyServer, C::NoQuestion, Direction::Export)
            | (R::Ask | R::AskUploadOnly, C::KeepDevice, Direction::Export) => {
                self.confirm_meguri_bootstrap(access, preview)
            }
            _ => Err(MeguriBootstrapError::SetupChanged(MeguriSetupChange::Choice)),
        }
    }

    /// Row 2, or the sheet's "keep the server's collection": discards the
    /// upload's preview, which sent nothing, and prepares the download.
    pub fn prepare_meguri_setup_download(
        &self,
        access: MeguriAccess,
        shown: &MeguriSetupDecision,
        preview: Preview,
        choice: MeguriSetupChoice,
    ) -> Result<ManualTransferTask<Preview>> {
        self.same_decision(access.connection(), access.username(), &preview, shown)?;
        Self::download_choice(shown, choice)?;
        if preview.operation.direction != Direction::Export {
            return Err(MeguriBootstrapError::SetupChanged(MeguriSetupChange::Choice));
        }
        self.discard_meguri_bootstrap_preparation(access.connection(), &preview.operation)?;
        self.prepare_meguri_bootstrap(access, MeguriBootstrapDirection::UseServerCollection)
    }
    fn download_choice(shown: &MeguriSetupDecision, choice: MeguriSetupChoice) -> Result<()> {
        match (shown.row, choice) {
            (MeguriSetupRow::DownloadOverEmptyDevice, MeguriSetupChoice::NoQuestion)
            | (MeguriSetupRow::Ask, MeguriSetupChoice::KeepServer) => Ok(()),
            _ => Err(MeguriBootstrapError::SetupChanged(MeguriSetupChange::Choice)),
        }
    }

    /// Confirms the download prepared by [`Self::prepare_meguri_setup_download`]
    /// only while this device is what `shown` held. Row 2 takes any readable
    /// server collection (MP6.3) and tests this device again here and inside
    /// the exclusive phase. "Keep the server's collection" also needs the
    /// server the sheet showed: serving a download rewrites the server's
    /// modification time and sequence number, so it is compared by content
    /// (rows, last review, and the schema time unless the retained copy had a
    /// legacy schema, whose content is read through the package's upgrade).
    pub fn confirm_meguri_setup_download(
        &self,
        access: MeguriAccess,
        shown: &MeguriSetupDecision,
        download: Preview,
        choice: MeguriSetupChoice,
    ) -> Result<ManualTransferTask<Operation>> {
        self.check_setup_preview(access.connection(), access.username(), &download)?;
        Self::download_choice(shown, choice)?;
        let op = &download.operation;
        if op.direction != Direction::Import || op.id == shown.operation {
            return Err(MeguriBootstrapError::SetupChanged(MeguriSetupChange::Choice));
        }
        if op.local.is_none() || op.local != shown.local {
            return Err(MeguriBootstrapError::SetupChanged(MeguriSetupChange::Local));
        }
        if choice == MeguriSetupChoice::KeepServer {
            let remote = MeguriBootstrapError::SetupChanged(MeguriSetupChange::Remote);
            let Some(sheet) = &shown.served_content else { return Err(remote) };
            let Some(candidate) = self.retained_candidate(op) else { return Err(remote) };
            let facts = meguri_storage::one_way::verify_server_copy(&candidate).ok().flatten().map(LocalFacts::from);
            let rows = meguri_storage::one_way::server_copy_user_content(&candidate).ok().flatten();
            let (Some(facts), Some(rows)) = (facts, rows) else { return Err(remote) };
            if rows != sheet.rows
                || facts.last_review != sheet.last_review
                || sheet.schema_modified.is_some_and(|schema| schema != facts.schema_modified)
            {
                return Err(remote);
            }
        }
        let retest = if choice == MeguriSetupChoice::NoQuestion {
            let empty = self
                .device_rows_at(op)
                .is_some_and(|(rows, media)| rows.none() && media == Some(0));
            if !empty {
                return Err(MeguriBootstrapError::SetupChanged(MeguriSetupChange::Local));
            }
            Some(Retest::NoUserContent(self.setup_media()))
        } else {
            None
        };
        self.confirm_meguri_bootstrap_retesting(access, download, retest)
    }

    /// MP6.9 "accepted": completes local activation with no question, for the
    /// recovery preview of [`Self::prepare_meguri_bootstrap_recovery`], only
    /// while this device is still what the upload sent. Refused with
    /// `SetupChanged(Local)` otherwise: nothing is replaced, the operation
    /// keeps its recorded acceptance, and finishing needs the user's own
    /// action. Three checks, each where it is used:
    /// - the facts that the recovery preparation captured inside its
    ///   exclusive phase are the facts captured for the upload;
    /// - the retained confirmation requires, inside its own exclusive phase,
    ///   the guard and facts of that recovery capture;
    /// - there too, before any backup or replacement, the open collection's
    ///   [`ContentWitness`] is the retained candidate's.
    pub fn confirm_meguri_setup_activation(
        &self,
        access: MeguriAccess,
        recovery: Preview,
    ) -> Result<ManualTransferTask<Operation>> {
        self.check_meguri_bootstrap(access.connection())?;
        let account = self.setup_account(access.connection(), access.username())?;
        let op = &recovery.operation;
        let active = self.manual_transfer_history().active().map_err(|_| MeguriBootstrapError::Busy)?;
        if active.as_ref() != Some(op) || op.phase != Phase::Preview {
            return Err(MeguriBootstrapError::Busy);
        }
        if op.account != account || op.configured_endpoint != access.connection().endpoint {
            return Err(MeguriReadyRefusal::AccountChanged.into());
        }
        if op.direction != Direction::Export || op.acceptance != Acceptance::ExactOk || op.recovery_guard.is_none() {
            return Err(MeguriReadyRefusal::OperationMismatch.into());
        }
        if op.local.is_none() || op.recovery_local != op.local {
            return Err(MeguriBootstrapError::SetupChanged(MeguriSetupChange::Local));
        }
        let sent = self
            .retained_candidate(op)
            .and_then(|path| meguri_storage::one_way::closed_content_witness(&path).ok().flatten())
            .ok_or(MeguriBootstrapError::Transfer(TransferError::Artifact))?;
        self.confirm_meguri_bootstrap_retesting(access, recovery, Some(Retest::Unchanged(sent)))
    }

    /// Deletes what a discarded preparation owned, when it sent nothing and
    /// replaced nothing: its retained candidate, the private copy of its
    /// safety backup, and the server copy it downloaded. Left in place: the
    /// published safety backup (an ordinary backup, which the backup list and
    /// its retention own, and which the last-backup record may name) and
    /// every artifact of a record that does not pass all of the tests below.
    pub(crate) fn sweep_discarded_preparation(&self, op: &Operation) {
        let unused = op.phase == Phase::Cancelled
            && op.acceptance == Acceptance::NotSent
            && op.activation_identity.is_none()
            && !op.collection_complete
            && op.resulting_publication.is_none()
            && op.receipt.is_none()
            && op.recovery_guard.is_none()
            && op.media == meguri_sync::policy::MediaOutcome::default();
        // Only the durable record decides, and never for the active operation.
        let history = self.manual_transfer_history();
        if !unused
            || !plain_name(op.id.as_str())
            || history.get(&op.id).ok().as_ref() != Some(op)
            || !history.active().is_ok_and(|active| active.as_ref().map(|active| &active.id) != Some(&op.id))
        {
            return;
        }
        let remove = |path: &Path| {
            if path.symlink_metadata().is_ok_and(|found| found.file_type().is_file()) {
                let _ = std::fs::remove_file(path);
            }
        };
        // The operation's own directory holds nothing another operation made.
        let owned = self.manual_transfer_directory.join(op.id.as_str());
        for artifact in [&op.candidate, &op.original].into_iter().flatten() {
            if plain_name(&artifact.name) {
                remove(&owned.join(&artifact.name));
            }
        }
        // Succeeds only when nothing else is left in it.
        let _ = std::fs::remove_dir(&owned);
        let _ = meguri_storage::atomic_file::sync_directory(&self.manual_transfer_directory);
        // The server copy was created under a new name by this preparation;
        // it is removed only while it is still the file that was recorded.
        if let Some(copy) = &op.server_copy {
            let name = &copy.artifact.name;
            let path = self.backup_inputs.backups_directory.join(name);
            if plain_name(name) && name.starts_with("server-copy-") && copy.artifact.matches_file(&path) {
                remove(&path);
                let _ = meguri_storage::atomic_file::sync_directory(&self.backup_inputs.backups_directory);
            }
        }
    }

    /// MP6.9: one read of the server's facts, compared with the retained
    /// candidate and with the facts read after the retained copy. It starts no
    /// upload and changes nothing on either side. A failed read is the
    /// task's error: the state stays unknown.
    pub fn settle_meguri_setup(&self, access: MeguriAccess) -> Result<ManualTransferTask<MeguriSetupSettlement>> {
        // MP7 step 1: an upload the structure save job has in flight is not
        // settled under it; a read made before the server commits would
        // record that nothing was sent.
        if self.meguri_structure_job_active() { return Err(MeguriBootstrapError::Busy); }
        let connection = access.connection().clone();
        let account = self.setup_account(&connection, access.username())?;
        let op = self
            .active_meguri_preparation(&connection)?
            .ok_or(MeguriReadyRefusal::NoIntent)?;
        if op.account != account {
            return Err(MeguriReadyRefusal::AccountChanged.into());
        }
        if op.direction != Direction::Export || op.acceptance != Acceptance::PossiblyAccepted {
            return Err(MeguriReadyRefusal::OperationMismatch.into());
        }
        let candidate = self
            .retained_candidate(&op)
            .and_then(|path| meguri_storage::one_way::verify_server_copy(&path).ok().flatten())
            .map(LocalFacts::from)
            .ok_or(MeguriBootstrapError::Transfer(TransferError::Artifact))?;
        let engine = self.engine().clone();
        let (clock, random) = self.meguri_round_sources();
        let (key, options, transport) = access.into_round_inputs();
        Ok(self.manual_worker(
            None,
            move |service, context, _| {
                let options = options.cancel(context.cancel.clone());
                let job = match transport {
                    Some(transport) => Job::with_transport(&options, Some(key), random.as_ref(), transport),
                    None => Job::new(&options, Some(key), random.as_ref()),
                };
                let mut job = job.map_err(|error| {
                    TransferError::Server(meguri_sync::JobError::from_transport(
                        error,
                        None,
                        meguri_sync::ServerKind::SelfHosted,
                    ))
                })?;
                let read = meguri_sync::transport::runtime::block_on(engine.threads(), &CancelToken::default(), async {
                    Ok(meguri_sync::status::probe(&mut job, clock.as_ref()).await)
                })
                .map_err(|_| TransferError::Storage)?;
                let server = RemoteFacts::from(&read?.server);
                let (outcome, operation) = service.policy().settle_upload(&op, &candidate, &server)?;
                Ok(match outcome {
                    // Nothing was learned or recorded: the state stays unknown.
                    UploadSettlement::Indistinct => {
                        return Err(TransferError::NeedsDecision(meguri_sync::policy::Boundary::RemoteCompletionUnknown));
                    }
                    UploadSettlement::Neither => MeguriSetupSettlement::Neither { operation },
                    UploadSettlement::NotAccepted => MeguriSetupSettlement::NotAccepted { operation },
                    UploadSettlement::Accepted => {
                        let local_unchanged = engine
                            .collection()
                            .ok()
                            .and_then(|collection| {
                                let reader = collection.reader(ReaderRole::Query).ok()?;
                                reader.transfer_facts().ok().map(LocalFacts::from)
                            })
                            .is_some_and(|facts| Some(&facts) == operation.local.as_ref());
                        // A hint for the caller only; see `confirm_meguri_setup_activation`.
                        MeguriSetupSettlement::Accepted { operation, local_unchanged }
                    }
                })
            },
            |_| Ok(()),
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use meguri_model::{clock::{FixedClock, UtcOffset}, rng::SeededRandom};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct Counting(AtomicUsize);
    impl BackupService for Counting {
        fn before_deck_delete(&self) -> std::result::Result<(), EngineError> {
            Ok(())
        }
        fn before_manual_transfer(
            &self,
            _collection: Option<&Collection>,
            _cancel: &CancelToken,
        ) -> std::result::Result<TransferBackup, EngineError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(TransferBackup::Absent)
        }
    }
    fn host(root: &std::path::Path) -> CoreHost {
        let collection = root.join("collection");
        std::fs::create_dir_all(&collection).unwrap();
        let config = crate::HostConfig::new(
            collection,
            root.join("backups"),
            root.join("widget"),
            Arc::new(FixedClock::new(1_894_701_600_000, UtcOffset::UTC)),
            Arc::new(SeededRandom::new(431)),
            Arc::new(meguri_storage::space::StatvfsSpace),
        );
        meguri_engine::bootstrap::create(
            &config.collection_directory.join("collection.anki2"),
            config.clock.as_ref(),
            config.random.as_ref(),
        )
        .unwrap();
        CoreHost::open(config).unwrap()
    }

    // MP6.3 "Proof is checked again where it is used": the test that runs
    // inside the replacement's exclusive phase, through the backup it takes there.
    #[test]
    fn armed_backup_refuses_a_collection_that_holds_user_content_and_takes_no_backup() {
        let root = tempfile::tempdir().unwrap();
        let host = host(root.path());
        let collection = host.collection().unwrap();
        let inner = Arc::new(Counting::default());
        let retest = Arc::new(SetupRetest::default());
        let backup = RetestingBackup::new(inner.clone(), retest.clone());
        let cancel = CancelToken::default();

        // Not armed: every transfer's backup passes through.
        assert!(backup.before_manual_transfer(Some(&collection), &cancel).is_ok());
        assert_eq!(inner.0.load(Ordering::SeqCst), 1);

        // Armed over a collection with no user content: the backup is taken, once armed once.
        retest.arm(Retest::NoUserContent(host.setup_media()));
        assert!(backup.before_manual_transfer(Some(&collection), &cancel).is_ok());
        assert_eq!(inner.0.load(Ordering::SeqCst), 2);
        assert!(retest.take().is_none());

        // An unreadable media inventory is user content.
        retest.arm(Retest::NoUserContent(None));
        assert!(matches!(
            backup.before_manual_transfer(Some(&collection), &cancel),
            Err(EngineError::StaleGeneration { .. })
        ));
        assert_eq!(inner.0.load(Ordering::SeqCst), 2);

        // A media file that the journal does not know yet is user content.
        let media = host.setup_media().unwrap();
        std::fs::create_dir_all(media.media_dir()).unwrap();
        let file = media.media_dir().join("sound.mp3");
        std::fs::write(&file, b"synthetic").unwrap();
        retest.arm(Retest::NoUserContent(host.setup_media()));
        assert!(backup.before_manual_transfer(Some(&collection), &cancel).is_err());
        std::fs::remove_file(&file).unwrap();

        // A deck made after the decision is user content: no backup, no activation.
        meguri_engine::decks::add(&collection, "Made late".into()).unwrap();
        retest.arm(Retest::NoUserContent(host.setup_media()));
        assert!(matches!(
            backup.before_manual_transfer(Some(&collection), &cancel),
            Err(EngineError::StaleGeneration { .. })
        ));
        assert_eq!(inner.0.load(Ordering::SeqCst), 2);
        // The refusal consumed the arming; a disarmed backup passes through again.
        assert!(backup.before_manual_transfer(Some(&collection), &cancel).is_ok());
        assert_eq!(inner.0.load(Ordering::SeqCst), 3);

        // MP6.9 accepted: only the collection the upload sent is replaced with no question.
        let witness = |collection: &Collection| collection.reader(ReaderRole::Query).unwrap().content_witness().unwrap();
        let sent = witness(&collection);
        retest.arm(Retest::Unchanged(sent));
        assert!(backup.before_manual_transfer(Some(&collection), &cancel).is_ok());
        assert_eq!(inner.0.load(Ordering::SeqCst), 4);
        meguri_engine::decks::add(&collection, "Made after the upload".into()).unwrap();
        assert_ne!(witness(&collection), sent);
        retest.arm(Retest::Unchanged(sent));
        assert!(matches!(
            backup.before_manual_transfer(Some(&collection), &cancel),
            Err(EngineError::StaleGeneration { .. })
        ));
        // A test armed for one side never passes for the other.
        retest.arm(Retest::Unchanged(sent));
        assert!(backup.before_manual_transfer(None, &cancel).is_err());
        retest.arm(Retest::NoDeviceMedia(root.path().join("absent")));
        assert!(backup.before_manual_transfer(Some(&collection), &cancel).is_err());
        assert_eq!(inner.0.load(Ordering::SeqCst), 4);

        // Row 1: a device without a collection still has no media file.
        let folder = root.path().join("loose.media");
        retest.arm(Retest::NoDeviceMedia(folder.clone()));
        assert!(backup.before_manual_transfer(None, &cancel).is_ok());
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("sound.mp3"), b"synthetic").unwrap();
        retest.arm(Retest::NoDeviceMedia(folder));
        assert!(backup.before_manual_transfer(None, &cancel).is_err());
        assert_eq!(inner.0.load(Ordering::SeqCst), 5);
        host.close().unwrap();
    }
}
