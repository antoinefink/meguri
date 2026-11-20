//! `sync/cloudkit-recovery#V27.2`: the facts of the setup decision, read
//! from the ready preview and tracking, and the starts that need no question.
//! Every start recomputes the row itself and then runs the existing
//! `publish`, `adopt` or `resume_binding` path with its checks unchanged.
use super::*;
use crate::cloud::{publication::*, setup::*};
use meguri_engine::device_state::PublicationDecision;
use meguri_storage::one_way::UserContentRows;
use std::path::Path;

/// V27.1 for a local collection. `rows` and `tracked_media` are `Err` when
/// their read failed; `media_folder` is listed here. Unreadable is content.
pub(super) fn content_proof(
    rows: Result<UserContentRows, ()>,
    tracked_media: Result<usize, ()>,
    media_folder: &Path,
) -> ContentProof {
    let folder_empty = match std::fs::read_dir(media_folder) {
        // An entry of any kind, or one that cannot be read, is content.
        Ok(mut entries) => entries.next().is_none(),
        // No folder holds no file.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(_) => false,
    };
    if folder_empty && tracked_media == Ok(0) && rows.is_ok_and(|rows| rows.none()) {
        ContentProof::ProvedNone
    } else {
        ContentProof::HoldsOrUnproved
    }
}

impl Runtime {
    /// The same test inside V14.2 step 7's exclusive phase: the working
    /// collection's rows in one read view, the media journal and the folder.
    pub(super) fn exclusive_content_proof(
        working: &Collection,
        media: &meguri_engine::media::MediaService,
    ) -> ContentProof {
        let rows = (|| -> Result<UserContentRows, RuntimeError> {
            let lease = working.reader(ReaderRole::Cloud)?;
            let snapshot = lease.snapshot()?;
            Ok(snapshot.user_content_rows()?)
        })()
        .map_err(drop);
        let tracked = media.store().tracked_names().map(|names| names.len()).map_err(drop);
        content_proof(rows, tracked, media.store().media_dir())
    }

    /// V27.2: "the core picks the row". `preview` must be this runtime's
    /// current ready preview. Reads tracking; starts and changes nothing.
    ///
    /// The iCloud side is never `ProvedNone` here: this build has no prefetch
    /// (V13.2 step 3), and `MGEpoch.counts` proves nothing (V27.1), so row 6
    /// is never chosen and its collection gets row 7's question.
    pub fn connection_setup_decision(
        &self,
        preview: &Preview,
        other_service: OtherServiceEvidence,
    ) -> Result<SetupDecision, RuntimeError> {
        Ok(decide(&self.connection_setup_inputs(preview, other_service)?))
    }

    fn connection_setup_inputs(
        &self,
        preview: &Preview,
        other_service: OtherServiceEvidence,
    ) -> Result<SetupInputs, RuntimeError> {
        Self::connection_setup_inputs_locked(&self.state(), preview, other_service)
    }

    /// The caller holds the state lock and not tracking access.
    fn connection_setup_inputs_locked(
        s: &State,
        preview: &Preview,
        other_service: OtherServiceEvidence,
    ) -> Result<SetupInputs, RuntimeError> {
        if s.publication.preview.as_ref() != Some(&(preview.clone(), true)) {
            return Err(PublicationError::AuthorityChanged.into());
        }
        // A Resume preview whose retained resume context was dropped (a
        // status pass whose provider validation failed drops it) can no
        // longer resume: no row is computed for it, and a fresh preview
        // gives the current one.
        if preview.decision == ConnectDecision::ConfirmResume
            && s.publication.resume.as_ref().map(|context| context.binding.id) != preview.resume_binding
        {
            return Err(PublicationError::AuthorityChanged.into());
        }
        let bindings =
            with_tracking_access(|| retained_setup_bindings(s.store.as_ref(), preview.native_file.is_some()))?;
        let local = match (preview.native_file, preview.local_content) {
            (None, _) => LocalSide::Absent,
            (Some(_), Some(proof)) => LocalSide::Present(proof),
            (Some(_), None) => LocalSide::Present(ContentProof::HoldsOrUnproved),
        };
        let cloud = match (&preview.active, preview.active_epoch, &preview.cloud) {
            (None, _, _) => CloudSide::NoActive,
            (Some(_), Some(_), Some(_)) if preview.decision != ConnectDecision::CloudUnreadable => {
                CloudSide::Complete(ContentProof::HoldsOrUnproved)
            }
            _ => CloudSide::Unreadable,
        };
        let resume_offered = preview.decision == ConnectDecision::ConfirmResume && preview.resume_binding.is_some();
        let work = &preview.retained_work;
        Ok(SetupInputs {
            local,
            cloud,
            resume_offered,
            other_service: other_service.history(),
            fence_pending: preview
                .decision_before
                .as_ref()
                .is_some_and(|d| d.publication.decision == PublicationDecision::Pending),
            layout_unproved: !preview.unsent_counts_exact,
            other_account_binding: bindings.first().is_some_and(|b| b.facts.account != preview.account),
            retained_binding: !bindings.is_empty(),
            // A binding that could resume holds this collection's sync state;
            // replacing either side over it is the user's choice.
            work_at_risk: preview.resume_binding.is_some()
                || preview.unsent_by_kind.values().any(|count| *count != 0)
                || preview.unpublished_by_kind.values().any(|count| *count != 0)
                || work.fetched_unverified != 0
                || work.held != 0
                || work.isolated_unsent != 0,
        })
    }

    /// Recomputes the row from this runtime's current preview and tracking,
    /// and admits only the start that `SetupDecision::silent_start` names.
    fn require_silent_start(
        &self,
        preview: &Preview,
        other_service: OtherServiceEvidence,
        starts: &[SilentStart],
    ) -> Result<SilentStart, RuntimeError> {
        match self.connection_setup_decision(preview, other_service)?.silent_start() {
            Some(start) if starts.contains(&start) => Ok(start),
            _ => Err(PublicationError::NotOffered.into()),
        }
    }

    /// Row 4, with no confirmation. Row 6 needs a proof this build cannot
    /// make, so it is refused here whatever the decision says.
    pub fn start_setup_publication(
        &self,
        preview: &Preview,
        other_service: OtherServiceEvidence,
        collection: &Collection,
    ) -> Result<[u8; 16], RuntimeError> {
        self.require_silent_start(preview, other_service, &[SilentStart::Publication])?;
        if preview.active.is_some() || preview.active_epoch.is_some() {
            return Err(PublicationError::NotOffered.into());
        }
        self.record_connection_publication(preview, other_service, collection)
    }

    /// V27.4's "Keep this device's collection" under the history the caller
    /// holds. `confirm_connection_publication` is this call with
    /// `OtherServiceEvidence::unknown()`, the only history a shipped core
    /// has. Where `MGActive` exists the publication replaces the iCloud
    /// collection, so `keep_device_admitted_locked` applies.
    pub fn confirm_setup_publication(
        &self,
        preview: &Preview,
        other_service: OtherServiceEvidence,
        collection: &Collection,
    ) -> Result<[u8; 16], RuntimeError> {
        self.record_connection_publication(preview, other_service, collection)
    }

    /// The publication guard. With no `MGActive` nothing in iCloud is
    /// replaced, and this admits the preview. With `MGActive`, the core's own
    /// decision for this preview must be V27.4's sheet with "Keep this
    /// device's collection" offered. Otherwise no such button was shown: a
    /// Resume confirmation (row 3, also while its history is unknown) or a
    /// row that starts with no question, such as row 5's download. Then
    /// this refuses with `NotOffered`. The caller holds the state lock and
    /// not tracking access, and has recorded nothing yet.
    ///
    /// The app's "Replace iCloud" (`ConnectDecision::ChooseCollection`) is
    /// always admitted: that decision needs `Counts::empty` false or inexact
    /// unsent counts, and `ProvedNone` implies `empty`, so row 5 is never its
    /// row. A change to `count_local_rows` or `content_proof` must keep
    /// `ProvedNone` stricter than `empty`. No test pins that boundary: the
    /// confirmed-flow setup test uses a collection that has notes.
    pub(super) fn keep_device_admitted_locked(
        s: &State,
        preview: &Preview,
        other_service: OtherServiceEvidence,
    ) -> Result<(), RuntimeError> {
        if preview.active.is_none() && preview.active_epoch.is_none() {
            return Ok(());
        }
        let decision = decide(&Self::connection_setup_inputs_locked(s, preview, other_service)?);
        if decision.question == SetupQuestion::Keep && decision.keep_device_offered {
            Ok(())
        } else {
            Err(PublicationError::NotOffered.into())
        }
    }

    /// Rows 2 and 5, with no confirmation. Row 5's op records that it needs
    /// the local collection to hold no user content when it is replaced.
    pub fn start_setup_adoption(
        &self,
        preview: &Preview,
        other_service: OtherServiceEvidence,
        collection: Option<&Collection>,
        engine: &meguri_engine::Engine,
    ) -> Result<[u8; 16], RuntimeError> {
        let start = self.require_silent_start(
            preview,
            other_service,
            &[SilentStart::AdoptionInitial, SilentStart::AdoptionOverNoContent],
        )?;
        if (start == SilentStart::AdoptionInitial) != collection.is_none() {
            return Err(PublicationError::AuthorityChanged.into());
        }
        let admission = if start == SilentStart::AdoptionOverNoContent {
            AdoptionAdmission::ProvedNoUserContent
        } else {
            AdoptionAdmission::Confirmed
        };
        self.record_cloud_adoption(preview, collection, engine, admission)
    }

    /// Row 3, with no confirmation: only with a durable record that no other
    /// service was ready since the stop (V27.6).
    pub fn start_setup_resume(
        &self,
        preview: &Preview,
        other_service: OtherServiceEvidence,
        collection: &Collection,
    ) -> Result<(), RuntimeError> {
        self.require_silent_start(preview, other_service, &[SilentStart::Resume])?;
        self.confirm_connection_resume(preview, collection)
    }

    /// V27.4's "Keep the iCloud collection" for a preview whose `stopped`
    /// binding could resume. The user's confirmation on the sheet, so V14
    /// runs as for any confirmed adoption: the op is bound to the shown
    /// `MGActive` and its change tag, the safety backup is step 7's, and
    /// every check of `confirm_cloud_adoption` is made. It is admitted only
    /// while the core's own decision for this preview is the sheet with that
    /// button offered (V27.6: Resume is not offered after the other
    /// service). While the decision is Resume or its confirmation, this
    /// refuses with `NotOffered` and changes nothing.
    pub fn confirm_setup_adoption_over_resumable(
        &self,
        preview: &Preview,
        other_service: OtherServiceEvidence,
        collection: &Collection,
        engine: &meguri_engine::Engine,
    ) -> Result<[u8; 16], RuntimeError> {
        let decision = self.connection_setup_decision(preview, other_service)?;
        if decision.question != SetupQuestion::Keep || !decision.keep_cloud_offered {
            return Err(PublicationError::NotOffered.into());
        }
        self.record_cloud_adoption(preview, Some(collection), engine, AdoptionAdmission::KeepOverResumable)
    }
}

/// How an adoption was admitted, for `record_cloud_adoption`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AdoptionAdmission {
    /// The user's confirmation, or row 2 where nothing local is replaced.
    Confirmed,
    /// V27.2 row 5: the op tests the local collection again when it replaces it.
    ProvedNoUserContent,
    /// The sheet's "Keep the iCloud collection" over a binding that could
    /// resume. Only `confirm_setup_adoption_over_resumable` gives it.
    KeepOverResumable,
}

/// The bindings the decision reads (V27.6). A device with a collection file
/// always has its tracking store open here, so a missing store is a failed
/// read, never "no binding": that answer would admit row 4's upload.
fn retained_setup_bindings(store: Option<&TrackingStore>, local_collection: bool) -> Result<Vec<Binding>, RuntimeError> {
    match store {
        Some(store) => Ok(store.setup_bindings()?),
        None if local_collection => Err(Error::Database.into()),
        None => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_rows() -> Result<UserContentRows, ()> {
        Ok(UserContentRows::default())
    }
    #[test]
    fn only_readable_rows_no_tracked_media_and_an_empty_folder_prove_no_user_content() {
        let folder = tempfile::tempdir().unwrap();
        assert_eq!(content_proof(no_rows(), Ok(0), folder.path()), ContentProof::ProvedNone);
        // No media folder holds no file.
        assert_eq!(
            content_proof(no_rows(), Ok(0), &folder.path().join("absent")),
            ContentProof::ProvedNone
        );
    }
    #[test]
    fn each_kind_of_row_and_each_media_file_is_user_content() {
        let folder = tempfile::tempdir().unwrap();
        let one = |set: fn(&mut UserContentRows)| {
            let mut rows = UserContentRows::default();
            set(&mut rows);
            Ok(rows)
        };
        let cases: [fn(&mut UserContentRows); 5] = [
            |r| r.notes = 1,
            |r| r.cards = 1,
            |r| r.reviews = 1,
            |r| r.other_decks = 1,
            |r| r.saved_notetypes = 1,
        ];
        for set in cases {
            assert_eq!(content_proof(one(set), Ok(0), folder.path()), ContentProof::HoldsOrUnproved);
        }
        assert_eq!(content_proof(no_rows(), Ok(1), folder.path()), ContentProof::HoldsOrUnproved);
        // A file the journal does not track yet is still a file in the folder.
        std::fs::write(folder.path().join("untracked.bin"), b"x").unwrap();
        assert_eq!(content_proof(no_rows(), Ok(0), folder.path()), ContentProof::HoldsOrUnproved);
    }
    /// A collection on this device with no tracking store is a failed read.
    /// With no collection there is no store yet, and no binding to retain.
    #[test]
    fn a_missing_tracking_store_beside_a_collection_is_an_error_not_no_binding() {
        assert!(matches!(
            retained_setup_bindings(None, true),
            Err(RuntimeError::Tracking(Error::Database))
        ));
        assert!(retained_setup_bindings(None, false).unwrap().is_empty());
        let directory = tempfile::tempdir().unwrap();
        let scope = BuildScope {
            container: ContainerId::new("test.meguri.setup.unit").unwrap(),
            environment: Environment::Development,
        };
        let store = match TrackingStore::open(directory.path(), scope).unwrap() {
            OpenOutcome::Missing(store) | OpenOutcome::Opened(store) => store,
            _ => panic!("a new tracking store opens"),
        };
        // An open store with no binding is the proved "no retained binding".
        assert!(retained_setup_bindings(Some(&store), true).unwrap().is_empty());
    }
    #[test]
    fn unreadable_counts_as_content() {
        let folder = tempfile::tempdir().unwrap();
        assert_eq!(content_proof(Err(()), Ok(0), folder.path()), ContentProof::HoldsOrUnproved);
        assert_eq!(content_proof(no_rows(), Err(()), folder.path()), ContentProof::HoldsOrUnproved);
        // A media folder that cannot be listed: here, a path that is a file.
        let file = folder.path().join("not-a-folder");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(content_proof(no_rows(), Ok(0), &file), ContentProof::HoldsOrUnproved);
    }
    /// A preview serialized before `local_content` existed decodes with the
    /// test not made, which the decision reads as holding user content.
    #[test]
    fn a_preview_written_before_the_content_proof_decodes_as_unproved() {
        let preview = Preview {
            id: 1,
            local: Counts {
                counts: Default::default(),
                empty: true,
            },
            cloud: None,
            published_at_ms: None,
            decision: ConnectDecision::ConfirmUpload,
            account: OpaqueBytes(vec![1]),
            native_file: Some(NativeFileIdentity {
                device: 1,
                inode: 2,
                created_seconds: 3,
                created_nanos: 4,
            }),
            decision_before: None,
            active: None,
            active_epoch: None,
            unpublished_by_kind: Default::default(),
            resume_binding: None,
            unsent_by_kind: Default::default(),
            unsent_counts_exact: true,
            retained_work: Default::default(),
            local_content: Some(ContentProof::ProvedNone),
        };
        let mut earlier = serde_json::to_value(&preview).unwrap();
        assert!(earlier.as_object_mut().unwrap().remove("local_content").is_some());
        let decoded: Preview = serde_json::from_value(earlier).unwrap();
        assert_eq!(decoded.local_content, None);
        assert!(
            decoded
                == Preview {
                    local_content: None,
                    ..preview
                }
        );
    }
}
