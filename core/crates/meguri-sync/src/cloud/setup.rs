//! `sync/cloudkit-recovery#V27.2`: what choosing iCloud runs, as a pure
//! decision over explicit facts. Nothing here reads a collection, tracking or
//! CloudKit, and nothing here starts work: `setup_runtime` gathers the facts
//! and the existing `publish`, `adopt` and `resume_binding` checks still guard
//! every operation the decision names.
//!
//! The rule that orders every branch: a side that is not proved to hold no
//! user content gets the question. A wrong "holds user content" costs one
//! question; no branch errs the other way (V27.3).
use serde::{Deserialize, Serialize};

/// V27.1's test result for one side. There is no "probably empty": anything
/// unread, unreadable, summarised or stale is `HoldsOrUnproved`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentProof {
    /// Computed from rows and the media inventory, inside the flow that uses it.
    ProvedNone,
    HoldsOrUnproved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocalSide {
    /// No collection file on this device.
    Absent,
    Present(ContentProof),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CloudSide {
    /// A record-ID read found no `MGActive`.
    NoActive,
    /// `MGActive` names a `complete` epoch. `ProvedNone` needs the staged
    /// collection of a finished prefetch; `MGEpoch.counts` never gives it.
    Complete(ContentProof),
    /// `MGActive` exists and cannot be read or decoded.
    Unreadable,
}

/// V27.6, "not after the other service": whether Meguri Sync reached a ready
/// binding on this device since the iCloud binding stopped. A fact of the
/// pure decision; the runtime accepts it only inside `OtherServiceEvidence`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OtherServiceHistory {
    /// Durably known: no other service was ready since the stop.
    NoneSinceStop,
    /// Durably known: the other service was ready since the stop.
    ReadySinceStop,
    /// No durable record answers it. Never treated as `NoneSinceStop`.
    Unknown,
}

/// The history as the runtime's decision and starts take it. A caller cannot
/// state one: it has no public field, no `Default` and no decoder, and its
/// only constructor outside tests is `unknown()`. So row 3 starts with no
/// question only after a constructor that reads the durable record of
/// `sync/meguri-provider#MP1` is added here; until that record exists, no
/// build without `test-hooks` can make a `NoneSinceStop`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OtherServiceEvidence(OtherServiceHistory);
impl OtherServiceEvidence {
    /// No durable record answers it: Resume keeps its confirmation.
    pub const fn unknown() -> Self {
        Self(OtherServiceHistory::Unknown)
    }
    pub const fn history(self) -> OtherServiceHistory {
        self.0
    }
    /// A history that a test states. Never compiled into a shipped core.
    #[cfg(any(test, feature = "test-hooks"))]
    pub const fn for_testing(history: OtherServiceHistory) -> Self {
        Self(history)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupInputs {
    pub local: LocalSide,
    pub cloud: CloudSide,
    /// V9.2's conditions hold for a retained binding: the same account, the
    /// same collection, the same epoch. Computed by the resume checks.
    pub resume_offered: bool,
    pub other_service: OtherServiceHistory,
    /// The publication fence is pending (V12.2): the sheet, whatever the sides hold.
    pub fence_pending: bool,
    /// V13.6: a required ordinal map is unproved, or own unsent work is unknown.
    pub layout_unproved: bool,
    /// V27.6: the newest binding other than a salvage binding is another account's.
    pub other_account_binding: bool,
    /// A binding other than a salvage binding is retained in this build scope.
    /// With no resume it is isolated, and V27.4 lists it under "shown for":
    /// its epoch is gone, switched or untrusted, so an upload could recreate
    /// lost iCloud content (V27.6, R2054) and neither row 4, 5 nor 6 is taken.
    pub retained_binding: bool,
    /// Unsent, held, isolated or fetched-and-unapplied work that a replacement
    /// would lose (V13.4, R2056).
    pub work_at_risk: bool,
}

/// The rows of V27.2's table, in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetupRow {
    /// Row 1: create an empty collection, then publish it (V15 from step 2).
    CreateAndPublish,
    /// Row 2: V14 into a device with no collection.
    AdoptInitial,
    /// Row 3: `resume_binding`.
    Resume,
    /// Row 4: V15 from step 2; `MGActive` names no epoch.
    Publish,
    /// Row 5: V14 over a local collection proved to hold no user content.
    AdoptOverNoContent,
    /// Row 6: V15 over an iCloud collection proved to hold no user content.
    PublishOverNoContent,
    /// Row 7: V27.4's sheet.
    KeepCollection,
}
impl SetupRow {
    pub fn number(self) -> u8 {
        match self {
            Self::CreateAndPublish => 1,
            Self::AdoptInitial => 2,
            Self::Resume => 3,
            Self::Publish => 4,
            Self::AdoptOverNoContent => 5,
            Self::PublishOverNoContent => 6,
            Self::KeepCollection => 7,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetupQuestion {
    /// The row's recovery call is made at once.
    None,
    /// Row 3 only, while `OtherServiceHistory` is `Unknown`: V13.3's
    /// "Continue syncing this collection" confirmation is kept.
    ConfirmResume,
    /// Which collection to keep (V27.4).
    Keep,
}

/// Why a question is asked. For the sheet's wording and for diagnostics; it
/// grants nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AskReason {
    /// Both sides hold user content, or the iCloud side is not proved empty.
    TwoCollections,
    Fence,
    UnprovedLayout,
    OtherAccount,
    /// A retained binding that cannot resume (V27.4): an upload could recreate
    /// iCloud content that it knew, and a download would pass over its state.
    RetainedBinding,
    WorkAtRisk,
    CloudUnreadable,
    /// Row 3 with no durable record of the other service.
    OtherServiceUnknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupDecision {
    pub row: SetupRow,
    pub question: SetupQuestion,
    pub reason: Option<AskReason>,
    /// V27.4: "Keep this device's collection" can be offered (V15).
    pub keep_device_offered: bool,
    /// V27.4: "Keep the iCloud collection" can be offered (V14, active source).
    pub keep_cloud_offered: bool,
}

/// The recovery call a decision may make with no confirmation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SilentStart {
    /// Row 4: `publish` while `MGActive` names no epoch.
    Publication,
    /// Row 2: `adopt` into a device with no collection.
    AdoptionInitial,
    /// Row 5: `adopt`, tested again inside its exclusive phase.
    AdoptionOverNoContent,
    /// Row 3: `resume_binding`.
    Resume,
}

impl SetupDecision {
    /// The only gate of the no-question starts. A question of any kind gives
    /// `None`, and so do row 1 (the app creates the collection first), row 6
    /// (this build has no prefetch and no switch-time fetch, so it cannot
    /// make or re-check that proof) and row 7.
    pub fn silent_start(&self) -> Option<SilentStart> {
        if self.question != SetupQuestion::None {
            return None;
        }
        match self.row {
            SetupRow::AdoptInitial => Some(SilentStart::AdoptionInitial),
            SetupRow::Resume => Some(SilentStart::Resume),
            SetupRow::Publish => Some(SilentStart::Publication),
            SetupRow::AdoptOverNoContent => Some(SilentStart::AdoptionOverNoContent),
            SetupRow::CreateAndPublish | SetupRow::PublishOverNoContent | SetupRow::KeepCollection => None,
        }
    }
}

/// The first matching row of V27.2, after the cases that always ask.
pub fn decide(inputs: &SetupInputs) -> SetupDecision {
    let local_present = matches!(inputs.local, LocalSide::Present(_));
    let cloud_complete = matches!(inputs.cloud, CloudSide::Complete(_));
    let run = |row| SetupDecision {
        row,
        question: SetupQuestion::None,
        reason: None,
        keep_device_offered: false,
        keep_cloud_offered: false,
    };
    let ask = |reason| SetupDecision {
        row: SetupRow::KeepCollection,
        question: SetupQuestion::Keep,
        reason: Some(reason),
        keep_device_offered: local_present,
        keep_cloud_offered: cloud_complete,
    };
    let local = match (inputs.local, inputs.cloud) {
        // An unreadable `MGActive` proves nothing about iCloud: never a row.
        (_, CloudSide::Unreadable) => return ask(AskReason::CloudUnreadable),
        (LocalSide::Absent, CloudSide::NoActive) => return run(SetupRow::CreateAndPublish),
        (LocalSide::Absent, CloudSide::Complete(_)) => return run(SetupRow::AdoptInitial),
        (LocalSide::Present(local), _) => local,
    };
    // V13.6 and V12.2 never take a row of the table.
    if inputs.layout_unproved {
        return ask(AskReason::UnprovedLayout);
    }
    if inputs.fence_pending {
        return ask(AskReason::Fence);
    }
    if inputs.resume_offered {
        match inputs.other_service {
            OtherServiceHistory::NoneSinceStop => return run(SetupRow::Resume),
            OtherServiceHistory::Unknown => {
                return SetupDecision {
                    row: SetupRow::Resume,
                    question: SetupQuestion::ConfirmResume,
                    reason: Some(AskReason::OtherServiceUnknown),
                    keep_device_offered: false,
                    keep_cloud_offered: false,
                };
            }
            // V27.6: Resume is not offered; go on to rows 4 to 7.
            OtherServiceHistory::ReadySinceStop => {}
        }
    }
    if inputs.other_account_binding {
        return ask(AskReason::OtherAccount);
    }
    // A binding that could resume is a retained binding, whatever the caller
    // says of `retained_binding`: where Resume is not offered it asks.
    let retained_binding = inputs.retained_binding || inputs.resume_offered;
    match (local, inputs.cloud) {
        (_, CloudSide::Unreadable) => ask(AskReason::CloudUnreadable),
        (_, CloudSide::NoActive) if retained_binding => ask(AskReason::RetainedBinding),
        (_, CloudSide::NoActive) => run(SetupRow::Publish),
        // V27.4 shows the sheet for an isolated binding, also when this
        // device holds no user content: row 5 is not taken over one.
        (ContentProof::ProvedNone, CloudSide::Complete(_)) if retained_binding => {
            ask(AskReason::RetainedBinding)
        }
        (ContentProof::ProvedNone, CloudSide::Complete(_)) if inputs.work_at_risk => ask(AskReason::WorkAtRisk),
        (ContentProof::ProvedNone, CloudSide::Complete(_)) => run(SetupRow::AdoptOverNoContent),
        (ContentProof::HoldsOrUnproved, CloudSide::Complete(ContentProof::ProvedNone)) => {
            if retained_binding {
                ask(AskReason::RetainedBinding)
            } else if inputs.work_at_risk {
                ask(AskReason::WorkAtRisk)
            } else {
                run(SetupRow::PublishOverNoContent)
            }
        }
        (ContentProof::HoldsOrUnproved, CloudSide::Complete(ContentProof::HoldsOrUnproved)) => {
            ask(AskReason::TwoCollections)
        }
    }
}

/// What row 6's re-fetch of the replaced epoch's zone found, right before
/// K18.3 step 5's `MGActive` save (V27.2, "Row 6, checked at the switch").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplacedZoneFetch {
    /// The fetch from the prefetch's change token drained with no change.
    Unchanged,
    /// Any record change or deletion.
    Changed,
    /// The fetch failed, did not drain, or was not made.
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SwitchGate {
    /// The conditional `MGActive` save may run.
    Save,
    /// Cancel the op as V15.3 says for its step; row 7 then asks.
    CancelAndAsk,
    /// Stay at `complete`; the op's normal retry fetches again.
    Wait,
}

/// The save runs only after an unchanged fetch. No other input lets it run.
pub fn switch_gate(fetch: ReplacedZoneFetch) -> SwitchGate {
    match fetch {
        ReplacedZoneFetch::Unchanged => SwitchGate::Save,
        ReplacedZoneFetch::Changed => SwitchGate::CancelAndAsk,
        ReplacedZoneFetch::Failed => SwitchGate::Wait,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ContentProof::{HoldsOrUnproved as Holds, ProvedNone as NoContent};

    fn inputs(local: LocalSide, cloud: CloudSide) -> SetupInputs {
        SetupInputs {
            local,
            cloud,
            resume_offered: false,
            other_service: OtherServiceHistory::NoneSinceStop,
            fence_pending: false,
            layout_unproved: false,
            other_account_binding: false,
            retained_binding: false,
            work_at_risk: false,
        }
    }
    fn every_input() -> Vec<SetupInputs> {
        let mut all = Vec::new();
        let local_sides = [LocalSide::Absent, LocalSide::Present(NoContent), LocalSide::Present(Holds)];
        let cloud_sides = [
            CloudSide::NoActive,
            CloudSide::Complete(NoContent),
            CloudSide::Complete(Holds),
            CloudSide::Unreadable,
        ];
        let histories = [
            OtherServiceHistory::NoneSinceStop,
            OtherServiceHistory::ReadySinceStop,
            OtherServiceHistory::Unknown,
        ];
        for local in local_sides {
            for cloud in cloud_sides {
                for other_service in histories {
                    for bits in 0..64u8 {
                        all.push(SetupInputs {
                            local,
                            cloud,
                            resume_offered: bits & 1 != 0,
                            other_service,
                            fence_pending: bits & 2 != 0,
                            layout_unproved: bits & 4 != 0,
                            other_account_binding: bits & 8 != 0,
                            retained_binding: bits & 16 != 0,
                            work_at_risk: bits & 32 != 0,
                        });
                    }
                }
            }
        }
        all
    }
    fn silent(row: SetupRow) -> SetupDecision {
        SetupDecision {
            row,
            question: SetupQuestion::None,
            reason: None,
            keep_device_offered: false,
            keep_cloud_offered: false,
        }
    }

    #[test]
    fn row_1_no_collection_anywhere_creates_and_publishes_with_no_question() {
        assert_eq!(
            decide(&inputs(LocalSide::Absent, CloudSide::NoActive)),
            silent(SetupRow::CreateAndPublish)
        );
    }
    #[test]
    fn row_2_no_local_collection_adopts_the_epoch_with_no_question() {
        for cloud in [CloudSide::Complete(Holds), CloudSide::Complete(NoContent)] {
            assert_eq!(decide(&inputs(LocalSide::Absent, cloud)), silent(SetupRow::AdoptInitial));
        }
    }
    #[test]
    fn row_3_a_stopped_binding_resumes_with_no_question_only_with_known_history() {
        let mut i = inputs(LocalSide::Present(Holds), CloudSide::Complete(Holds));
        i.resume_offered = true;
        i.retained_binding = true;
        assert_eq!(decide(&i), silent(SetupRow::Resume));
        i.other_service = OtherServiceHistory::Unknown;
        let unknown = decide(&i);
        assert_eq!((unknown.row, unknown.question), (SetupRow::Resume, SetupQuestion::ConfirmResume));
        assert_eq!(unknown.reason, Some(AskReason::OtherServiceUnknown));
    }
    #[test]
    fn row_3_is_not_offered_after_the_other_service_and_two_collections_ask() {
        let mut i = inputs(LocalSide::Present(Holds), CloudSide::Complete(Holds));
        i.resume_offered = true;
        i.retained_binding = true;
        i.other_service = OtherServiceHistory::ReadySinceStop;
        let d = decide(&i);
        assert_eq!((d.row, d.question), (SetupRow::KeepCollection, SetupQuestion::Keep));
        assert!(d.keep_device_offered && d.keep_cloud_offered);
    }
    #[test]
    fn row_4_a_collection_and_no_active_epoch_publishes_with_no_question() {
        for local in [NoContent, Holds] {
            assert_eq!(
                decide(&inputs(LocalSide::Present(local), CloudSide::NoActive)),
                silent(SetupRow::Publish)
            );
        }
    }
    #[test]
    fn row_5_a_local_collection_with_no_user_content_adopts_with_no_question() {
        for cloud in [NoContent, Holds] {
            assert_eq!(
                decide(&inputs(LocalSide::Present(NoContent), CloudSide::Complete(cloud))),
                silent(SetupRow::AdoptOverNoContent)
            );
        }
    }
    #[test]
    fn row_6_needs_the_cloud_side_proved_to_hold_no_user_content() {
        assert_eq!(
            decide(&inputs(LocalSide::Present(Holds), CloudSide::Complete(NoContent))),
            silent(SetupRow::PublishOverNoContent)
        );
    }
    #[test]
    fn row_7_two_collections_ask_which_to_keep() {
        let d = decide(&inputs(LocalSide::Present(Holds), CloudSide::Complete(Holds)));
        assert_eq!(
            d,
            SetupDecision {
                row: SetupRow::KeepCollection,
                question: SetupQuestion::Keep,
                reason: Some(AskReason::TwoCollections),
                keep_device_offered: true,
                keep_cloud_offered: true,
            }
        );
    }
    #[test]
    fn the_fence_and_an_unproved_layout_always_show_the_sheet() {
        for cloud in [CloudSide::NoActive, CloudSide::Complete(NoContent), CloudSide::Complete(Holds)] {
            for local in [NoContent, Holds] {
                for resume in [false, true] {
                    let mut i = inputs(LocalSide::Present(local), cloud);
                    i.resume_offered = resume;
                    i.fence_pending = true;
                    assert_eq!(decide(&i).reason, Some(AskReason::Fence));
                    i.fence_pending = false;
                    i.layout_unproved = true;
                    assert_eq!(decide(&i).reason, Some(AskReason::UnprovedLayout));
                }
            }
        }
    }
    #[test]
    fn another_account_never_takes_rows_4_5_or_6() {
        for i in every_input() {
            let d = decide(&i);
            if i.other_account_binding {
                assert!(
                    !matches!(
                        d.row,
                        SetupRow::Publish | SetupRow::AdoptOverNoContent | SetupRow::PublishOverNoContent
                    ),
                    "{i:?}"
                );
            }
        }
        let mut i = inputs(LocalSide::Present(Holds), CloudSide::NoActive);
        i.other_account_binding = true;
        let d = decide(&i);
        // The one-direction form: nothing in iCloud can be kept.
        assert_eq!((d.reason, d.keep_device_offered, d.keep_cloud_offered), (Some(AskReason::OtherAccount), true, false));
    }
    /// V27.4 and V27.6: with a local collection, a retained binding takes
    /// row 3 or the sheet, never row 4, 5 or 6.
    #[test]
    fn a_retained_binding_never_leads_to_a_silent_upload_or_download() {
        for i in every_input() {
            let d = decide(&i);
            if i.retained_binding && matches!(i.local, LocalSide::Present(_)) {
                assert!(
                    !matches!(
                        d.row,
                        SetupRow::Publish | SetupRow::AdoptOverNoContent | SetupRow::PublishOverNoContent
                    ),
                    "{i:?}"
                );
                assert!(d.question != SetupQuestion::None || d.row == SetupRow::Resume, "{i:?}");
            }
        }
    }
    #[test]
    fn row_5_asks_over_a_retained_binding_that_cannot_resume() {
        for cloud in [NoContent, Holds] {
            let mut i = inputs(LocalSide::Present(NoContent), CloudSide::Complete(cloud));
            i.retained_binding = true;
            assert_eq!(
                decide(&i),
                SetupDecision {
                    row: SetupRow::KeepCollection,
                    question: SetupQuestion::Keep,
                    reason: Some(AskReason::RetainedBinding),
                    keep_device_offered: true,
                    keep_cloud_offered: true,
                }
            );
            // The same binding after the other service was ready (V27.6).
            i.resume_offered = true;
            i.other_service = OtherServiceHistory::ReadySinceStop;
            i.work_at_risk = true;
            assert_eq!(decide(&i).reason, Some(AskReason::RetainedBinding));
        }
    }
    /// A sheet shown where Resume is not offered has both of its buttons.
    #[test]
    fn a_resumable_binding_after_the_other_service_offers_both_collections() {
        for i in every_input() {
            if i.resume_offered
                && i.other_service == OtherServiceHistory::ReadySinceStop
                && matches!(i.local, LocalSide::Present(_))
                && matches!(i.cloud, CloudSide::Complete(_))
            {
                let d = decide(&i);
                assert_eq!((d.row, d.question), (SetupRow::KeepCollection, SetupQuestion::Keep), "{i:?}");
                assert!(d.keep_device_offered && d.keep_cloud_offered, "{i:?}");
            }
        }
    }
    #[test]
    fn evidence_outside_a_test_is_only_unknown() {
        assert_eq!(OtherServiceEvidence::unknown().history(), OtherServiceHistory::Unknown);
        let stated = OtherServiceEvidence::for_testing(OtherServiceHistory::NoneSinceStop);
        assert_eq!(stated.history(), OtherServiceHistory::NoneSinceStop);
    }
    #[test]
    fn an_unreadable_active_record_never_takes_a_row() {
        for i in every_input() {
            if i.cloud == CloudSide::Unreadable {
                let d = decide(&i);
                assert_eq!((d.row, d.question, d.keep_cloud_offered), (SetupRow::KeepCollection, SetupQuestion::Keep, false));
            }
        }
    }
    /// V22 invariant 5 as changed by V27.3, over every input: a row that
    /// starts with no question replaces nothing that is not proved empty.
    #[test]
    fn invariant_5_no_silent_row_replaces_a_side_that_is_not_proved_empty() {
        for i in every_input() {
            let d = decide(&i);
            if d.question != SetupQuestion::None {
                continue;
            }
            match d.row {
                SetupRow::CreateAndPublish => {
                    assert_eq!((i.local, i.cloud), (LocalSide::Absent, CloudSide::NoActive), "{i:?}")
                }
                SetupRow::AdoptInitial => {
                    assert!(i.local == LocalSide::Absent && matches!(i.cloud, CloudSide::Complete(_)), "{i:?}")
                }
                SetupRow::Resume => assert!(
                    i.resume_offered
                        && i.other_service == OtherServiceHistory::NoneSinceStop
                        && matches!(i.local, LocalSide::Present(_)),
                    "{i:?}"
                ),
                SetupRow::Publish => {
                    assert!(matches!(i.local, LocalSide::Present(_)) && i.cloud == CloudSide::NoActive, "{i:?}")
                }
                SetupRow::AdoptOverNoContent => assert!(
                    i.local == LocalSide::Present(NoContent)
                        && matches!(i.cloud, CloudSide::Complete(_))
                        && !i.work_at_risk
                        && !i.retained_binding,
                    "{i:?}"
                ),
                SetupRow::PublishOverNoContent => assert!(
                    i.local == LocalSide::Present(Holds) && i.cloud == CloudSide::Complete(NoContent) && !i.work_at_risk,
                    "{i:?}"
                ),
                SetupRow::KeepCollection => panic!("row 7 always asks: {i:?}"),
            }
            assert!(!i.fence_pending && !i.layout_unproved || i.local == LocalSide::Absent, "{i:?}");
        }
    }
    /// The other direction of the same rule: two sides that are not proved
    /// empty are never settled without the user, whatever else holds, except
    /// by continuing the same collection on the same epoch (row 3).
    #[test]
    fn two_unproved_sides_always_ask_unless_the_same_collection_resumes() {
        for i in every_input() {
            if i.local == LocalSide::Present(Holds) && i.cloud == CloudSide::Complete(Holds) {
                let d = decide(&i);
                assert!(
                    d.question != SetupQuestion::None || d.row == SetupRow::Resume,
                    "{i:?}"
                );
            }
        }
    }
    #[test]
    fn a_question_offers_only_a_side_that_exists_and_can_be_read() {
        for i in every_input() {
            let d = decide(&i);
            if d.keep_device_offered {
                assert!(matches!(i.local, LocalSide::Present(_)), "{i:?}");
            }
            if d.keep_cloud_offered {
                assert!(matches!(i.cloud, CloudSide::Complete(_)), "{i:?}");
            }
            if d.question == SetupQuestion::None {
                assert!(!d.keep_device_offered && !d.keep_cloud_offered && d.reason.is_none(), "{i:?}");
            } else {
                assert!(d.reason.is_some(), "{i:?}");
            }
        }
    }
    /// Over the whole table: no start is admitted for a question, and an
    /// admitted start is exactly its row's, from that row's two sides.
    #[test]
    fn no_start_is_admitted_while_the_decision_is_a_question() {
        let mut admitted = [0usize; 4];
        for i in every_input() {
            let d = decide(&i);
            let start = d.silent_start();
            if d.question != SetupQuestion::None {
                assert_eq!(start, None, "{i:?}");
                continue;
            }
            match (d.row, start) {
                (SetupRow::Publish, Some(SilentStart::Publication)) => {
                    assert!(matches!(i.local, LocalSide::Present(_)) && i.cloud == CloudSide::NoActive, "{i:?}");
                    assert!(!i.retained_binding && !i.other_account_binding && !i.fence_pending, "{i:?}");
                    admitted[0] += 1;
                }
                (SetupRow::AdoptInitial, Some(SilentStart::AdoptionInitial)) => {
                    assert!(i.local == LocalSide::Absent && matches!(i.cloud, CloudSide::Complete(_)), "{i:?}");
                    admitted[1] += 1;
                }
                (SetupRow::AdoptOverNoContent, Some(SilentStart::AdoptionOverNoContent)) => {
                    assert!(i.local == LocalSide::Present(NoContent) && !i.work_at_risk, "{i:?}");
                    assert!(!i.other_account_binding && !i.fence_pending && !i.layout_unproved, "{i:?}");
                    assert!(!i.retained_binding, "{i:?}");
                    admitted[2] += 1;
                }
                (SetupRow::Resume, Some(SilentStart::Resume)) => {
                    assert!(i.resume_offered && i.other_service == OtherServiceHistory::NoneSinceStop, "{i:?}");
                    admitted[3] += 1;
                }
                // Row 1 is the app's create; row 6 has no proof in this build.
                (SetupRow::CreateAndPublish | SetupRow::PublishOverNoContent, None) => {}
                other => panic!("{other:?} for {i:?}"),
            }
        }
        // The property is not vacuous: every start is reached.
        assert!(admitted.iter().all(|count| *count != 0), "{admitted:?}");
    }
    /// The same gate over every decision value, also ones `decide` never gives.
    #[test]
    fn a_question_never_admits_a_start_whatever_row_it_names() {
        let rows = [
            SetupRow::CreateAndPublish,
            SetupRow::AdoptInitial,
            SetupRow::Resume,
            SetupRow::Publish,
            SetupRow::AdoptOverNoContent,
            SetupRow::PublishOverNoContent,
            SetupRow::KeepCollection,
        ];
        for row in rows {
            for question in [SetupQuestion::ConfirmResume, SetupQuestion::Keep] {
                let decision = SetupDecision {
                    row,
                    question,
                    reason: None,
                    keep_device_offered: true,
                    keep_cloud_offered: true,
                };
                assert_eq!(decision.silent_start(), None, "{decision:?}");
            }
        }
    }
    #[test]
    fn rows_are_numbered_as_the_table() {
        let rows = [
            SetupRow::CreateAndPublish,
            SetupRow::AdoptInitial,
            SetupRow::Resume,
            SetupRow::Publish,
            SetupRow::AdoptOverNoContent,
            SetupRow::PublishOverNoContent,
            SetupRow::KeepCollection,
        ];
        assert_eq!(rows.map(SetupRow::number), [1, 2, 3, 4, 5, 6, 7]);
    }
    #[test]
    fn the_switch_runs_only_after_an_unchanged_fetch() {
        assert_eq!(switch_gate(ReplacedZoneFetch::Unchanged), SwitchGate::Save);
        assert_eq!(switch_gate(ReplacedZoneFetch::Changed), SwitchGate::CancelAndAsk);
        // A failed fetch never lets the save run.
        assert_eq!(switch_gate(ReplacedZoneFetch::Failed), SwitchGate::Wait);
    }
}
