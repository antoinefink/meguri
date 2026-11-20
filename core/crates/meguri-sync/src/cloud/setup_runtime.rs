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
        other_service: OtherServiceHistory,
    ) -> Result<SetupDecision, RuntimeError> {
        Ok(decide(&self.connection_setup_inputs(preview, other_service)?))
    }

    pub fn connection_setup_inputs(
        &self,
        preview: &Preview,
        other_service: OtherServiceHistory,
    ) -> Result<SetupInputs, RuntimeError> {
        let s = self.state();
        if s.publication.preview.as_ref() != Some(&(preview.clone(), true)) {
            return Err(PublicationError::AuthorityChanged.into());
        }
        let bindings = with_tracking_access(|| match &s.store {
            Some(store) => store.setup_bindings(),
            None => Ok(Vec::new()),
        })?;
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
            other_service,
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

    fn require_setup_row(
        &self,
        preview: &Preview,
        other_service: OtherServiceHistory,
        rows: &[SetupRow],
    ) -> Result<SetupRow, RuntimeError> {
        let decision = self.connection_setup_decision(preview, other_service)?;
        if decision.question != SetupQuestion::None || !rows.contains(&decision.row) {
            return Err(PublicationError::NotOffered.into());
        }
        Ok(decision.row)
    }

    /// Row 4, with no confirmation. Row 6 needs a proof this build cannot
    /// make, so it is refused here whatever the decision says.
    pub fn start_setup_publication(
        &self,
        preview: &Preview,
        other_service: OtherServiceHistory,
        collection: &Collection,
    ) -> Result<[u8; 16], RuntimeError> {
        self.require_setup_row(preview, other_service, &[SetupRow::Publish])?;
        if preview.active.is_some() || preview.active_epoch.is_some() {
            return Err(PublicationError::NotOffered.into());
        }
        self.confirm_connection_publication(preview, collection)
    }

    /// Rows 2 and 5, with no confirmation. Row 5's op records that it needs
    /// the local collection to hold no user content when it is replaced.
    pub fn start_setup_adoption(
        &self,
        preview: &Preview,
        other_service: OtherServiceHistory,
        collection: Option<&Collection>,
        engine: &meguri_engine::Engine,
    ) -> Result<[u8; 16], RuntimeError> {
        let row = self.require_setup_row(
            preview,
            other_service,
            &[SetupRow::AdoptInitial, SetupRow::AdoptOverNoContent],
        )?;
        if (row == SetupRow::AdoptInitial) != collection.is_none() {
            return Err(PublicationError::AuthorityChanged.into());
        }
        self.record_cloud_adoption(preview, collection, engine, row == SetupRow::AdoptOverNoContent)
    }

    /// Row 3, with no confirmation: only with a durable record that no other
    /// service was ready since the stop (V27.6).
    pub fn start_setup_resume(
        &self,
        preview: &Preview,
        other_service: OtherServiceHistory,
        collection: &Collection,
    ) -> Result<(), RuntimeError> {
        self.require_setup_row(preview, other_service, &[SetupRow::Resume])?;
        self.confirm_connection_resume(preview, collection)
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
}
