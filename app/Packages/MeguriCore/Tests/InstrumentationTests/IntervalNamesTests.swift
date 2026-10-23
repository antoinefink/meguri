import Testing

@testable import Instrumentation

/// The registry (R1861, U12): one name per interval, the R1861 set present
/// under the names the plan's units use, bridge calls in their own category.
@Suite struct IntervalNamesTests {
    /// R1861's intervals, as the plan names them (U175, U176, U237, U241)
    /// or as U12 named them.
    static let r1861 = [
        "review.answerToNextVisible",
        "review.revealToAnswerVisible",
        "launch.toDeckListInteractive",
        "search.keystrokeToResultsVisible",
        "browser.openToFirstPage",
        "stats.firstGraphs",
        "editor.openToEditable",
        "sync.startToEnd",
    ]

    @Test func everyR1861IntervalIsRegisteredAndReportedToFieldDiagnostics() throws {
        for name in Self.r1861 {
            let interval = try #require(IntervalNames.named(name), "\(name) is not registered")
            #expect(interval.fieldReported, "\(name) must reach MetricKit (R1867)")
            #expect(interval.category != .bridge && interval.category != .probe)
        }
        // Only R1861's intervals go to MetricKit, which keeps a few per app.
        #expect(Set(IntervalNames.all.filter(\.fieldReported).map(\.name)) == Set(Self.r1861))
    }

    @Test func namesAreUniqueAndAreaQualified() {
        let names = IntervalNames.all.map(\.name)
        #expect(Set(names).count == names.count)
        for interval in IntervalNames.all where interval.category != .bridge {
            #expect(
                interval.name.wholeMatch(of: /[a-z][A-Za-z]*\.[a-z][A-Za-z]*/) != nil,
                "\(interval.name) is not area.camelCase")
        }
    }

    @Test func unitsRegisteredNamesTheyEmitLater() {
        // Named in the plan as registered here (U176, U237, U241).
        for name in [
            "review.startToFirstCard", "review.presentToNextSettled", "review.onDemandRender",
            "review.answerMainThread", "review.undoToCardVisible", "review.actionToNextVisible",
            "review.touchDownToTint", "search.firstPage", "stats.allGraphs", "stats.scopeChange",
            "heatmap.render", "cardInfo.open",
        ] {
            #expect(IntervalNames.named(name) != nil, "\(name)")
        }
        // The rendering spike's baseline swap (U65), a test probe.
        #expect(IntervalNames.named("probe.nativeSwap")?.category == .probe)
    }

    @Test func bridgeCallsAreSignpostedUnderTheirExportNamesOnly() {
        let mediaExports = [
            "ManualTransferSession.start_media_preview",
            "ManualTransferSession.start_media",
            "ManualTransferSession.start_cancel_prepared_media",
            "ManualTransferSession.start_invalidate_media_owner",
            "ManualMediaPreview.record",
            "ManualTransferResult.media_snapshot",
            "ManualTransferResult.request_stop",
        ]
        #expect(BridgeCall.all.map(\.name) == mediaExports + [
            "normalize_manual_endpoint", "ManualTransferSession.new", "ManualTransferSession.start_setup", "ManualTransferSession.start_login", "ManualTransferSession.start_history", "ManualTransferSession.start_prepare", "ManualTransferSession.start_confirm", "ManualTransferSession.start_recovery_preview", "ManualTransferSession.start_cancel_history", "ManualTransferSession.start_cancel_preview", "ManualTransferSession.start_save_copy", "ManualTransferResult.outcome", "ManualTransferResult.operation_id", "ManualTransferPreview.record", "ManualTransferHistoryEntry.record",
            "start_collection_export", "CollectionExportResult.outcome", "CollectionExportArtifact.metadata", "CollectionExportArtifact.acquire", "CollectionExportArtifact.discard", "CollectionExportLease.path", "CollectionExportLease.finish", "start_collection_export_save", "CollectionExportSaveResult.outcome",
            "start_collection_inspection", "CollectionInspectResult.outcome", "CollectionInspection.facts", "CollectionInspection.discard", "start_collection_activation", "CollectionActivationResult.outcome", "JobProgress.setListener",
            "start_open", "close_collection", "collection_lifecycle", "wait_collection_available", "deck_tree_with_counts", "deck_study_state", "deck_description_html",
            "start_session", "ReviewSessionJob.result", "peek_first_card", "session_head", "refresh_session_card", "render_session_head", "answer_session", "end_session", "peek_review_undo", "flush_review_for_background",
            "core_version", "core_build_info", "set_event_listener",
            "CancelToken.new", "CancelToken.cancel", "CancelToken.isCancelled", "render_card",
            "compare_typed_answer", "card_excerpts",
            "DeckDeletionJob.result", "add_deck", "start_rename_deck", "start_move_decks",
            "start_collapse_decks", "select_deck", "deck_deletion_summary", "start_delete_decks",
            "NoteDeletionJob.result", "add_note", "update_note", "read_note",
            "check_note_fields", "start_delete_notes", "notetype_names", "read_notetype",
            "add_notetype", "add_stock_notetype", "clone_notetype", "ensure_image_occlusion_notetype",
            "rename_notetype", "set_notetype_css", "set_notetype_field_options", "set_notetype_template_options",
            "TagMutationJob.result", "tag_tree", "flag_names", "set_tags_collapsed",
            "rename_flag", "restore_flag_name", "add_note_tags", "start_add_note_tags",
            "remove_note_tags", "start_remove_note_tags", "set_card_flag", "start_set_card_flag",
            "toggle_card_flag", "start_toggle_card_flag", "mark_notes", "start_mark_notes",
            "unmark_notes", "start_unmark_notes", "toggle_note_mark", "start_toggle_note_mark",
            "start_rename_tag", "start_reparent_tags", "start_delete_tags", "start_clear_unused_tags",
            "add_media", "tracked_media_names", "flush_for_background", "suspend", "resume", "check_rollover", "fsrs_policy_status", "enable_fsrs",
            "UndoJob.result", "peek_undo", "peek_redo", "try_pending_review_restore",
            "DeckRenameJob.result", "DeckMoveJob.result", "DeckCollapseJob.result", "start_undo", "start_redo", "start_add_notes", "NoteAdditionJob.result",
            "parse_due_days", "start_sched_op", "SchedOpJob.result", "start_buried_kinds", "BuriedKindsJob.result",
            "DeckOptionsEditor.snapshot", "DeckOptionsEditor.edit", "DeckOptionsEditor.validate", "parse_option_steps", "format_option_steps", "start_deck_options", "start_save_deck_options", "start_option_ignore_count", "start_scheduling_preferences", "start_save_scheduling_preferences", "DeckOptionsReadJob.result", "DeckOptionsSaveJob.result", "OptionIgnoreCountJob.result", "SchedulingPreferencesJob.result", "SchedulingPreferencesSaveJob.result", "validate_scheduling_preferences",
        ])
        for call in BridgeCall.all {
            #expect(call.interval.category == .bridge)
            #expect(!call.interval.fieldReported, "bridge calls never go to MetricKit")
            #expect(IntervalNames.named(call.name) == call.interval)
        }
    }

    @Test func anUnknownNameIsNotFound() {
        #expect(IntervalNames.named("review.answerToNext") == nil)
        #expect(IntervalNames.named("") == nil)
    }

    @Test func oneSubsystemForEveryInterval() {
        #expect(Set(IntervalNames.all.map(\.subsystem)) == ["com.antoinefink.meguri"])
        #expect(IntervalNames.answerToNextVisible.category.rawValue == "Review")
        #expect(BridgeCall.coreVersion.interval.category.rawValue == "CoreBridge")
    }

    @Test func signpostsBeginAndEndEveryKindOfInterval() throws {
        for interval in IntervalNames.all {
            Signposts.end(Signposts.begin(interval))
        }
        #expect(Signposts.measure(IntervalNames.probeSyntheticDelay) { 42 } == 42)
        #expect(Signposts.bridge(.coreVersion, lane: "control") { "v" } == "v")
        struct Failure: Error {}
        #expect(throws: Failure.self) {
            try Signposts.measure(IntervalNames.answerToNextVisible) { () throws(Failure) in throw Failure() }
        }
    }
}
