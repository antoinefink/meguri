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
        // The parts of the start of a review (R1813), in its category.
        for name in ["review.startPagesReady", "review.startCardReady"] {
            #expect(IntervalNames.named(name)?.category == .review, "\(name)")
            #expect(IntervalNames.named(name)?.fieldReported == false, "\(name)")
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
        #expect(BridgeCall.all.map(\.name) == [
            "migrate_provider", "provider_owner", "ProviderOwner.snapshot", "ProviderOwner.refresh", "ProviderOwner.save_settings",
            "ProviderOwner.begin_handoff", "ProviderOwner.finish_handoff",
            "ProviderOwner.recover_handoff_to_local_only", "ProviderOwner.retry_failed_publication",
            "ProviderOwner.configure_meguri", "ProviderOwner.configured_meguri", "ProviderConnection.information", "ProviderConnection.start_login", "ProviderLoginResult.result",
            "ProviderConnection.start_bootstrap_prepare", "ProviderConnection.start_bootstrap_confirm", "ProviderConnection.start_bootstrap_media_prepare",
            "ProviderConnection.start_bootstrap_media", "ProviderConnection.complete_bootstrap", "ProviderConnection.bootstrap_readiness",
            "ProviderBootstrapPreview.record", "ProviderBootstrapMediaConsent.record", "ProviderBootstrapOperation.record",
            "ProviderBootstrapResult.outcome", "ProviderBootstrapResult.media_snapshot", "ProviderBootstrapResult.request_stop",
            "ProviderBootstrapResult.retained_operation",
            "ProviderConnection.start_collection_round", "ProviderConnection.start_media_round", "ProviderRoundResult.outcome", "ProviderRoundResult.retry_finalization",
            "ProviderConnection.start_structure_save", "ProviderConnection.start_structure_save_for_generation", "ProviderConnection.start_structure_upload", "ProviderStructureJob.advance", "ProviderStructureJob.cancel",
            "ProviderStructureJob.saved", "ProviderOwner.structure_upload_pending",
            "ProviderStructureJob.import_outcome", "package_file_class_of_name", "package_file_class", "deck_import_stored_options",
            "start_deck_import_inspect", "DeckImportInspectJob.outcome", "DeckImportSession.start_apply", "DeckImportApplyJob.outcome",
            "DeckImportSession.result", "DeckImportSession.log_page", "DeckImportSession.close",
            "ProviderConnection.pending_bootstrap_preparation", "ProviderConnection.discard_bootstrap_preview",
            "ProviderConnection.discard_bootstrap_operation", "ProviderConnection.start_bootstrap_recovery",
            ] + mediaExports + [
            "normalize_manual_endpoint", "ManualTransferSession.new", "ManualTransferSession.start_setup", "ManualTransferSession.start_login", "ManualTransferSession.start_history", "ManualTransferSession.start_prepare", "ManualTransferSession.start_confirm", "ManualTransferSession.start_recovery_preview", "ManualTransferSession.start_cancel_history", "ManualTransferSession.start_cancel_preview", "ManualTransferSession.start_save_copy", "ManualTransferResult.outcome", "ManualTransferResult.operation_id", "ManualTransferPreview.record", "ManualTransferHistoryEntry.record",
            "start_create_backup", "CreateBackupResult.outcome", "start_restore_backup", "RestoreBackupResult.outcome", "start_run_due_maintenance", "DueMaintenanceResult.outcome", "list_backups", "backup_limits", "set_backup_limits", "update_backup_limit", "maintenance_status",
            "start_deck_export", "start_collection_export", "CollectionExportResult.outcome", "CollectionExportArtifact.metadata", "CollectionExportArtifact.acquire", "CollectionExportArtifact.discard", "CollectionExportLease.path", "CollectionExportLease.finish", "start_collection_export_save", "CollectionExportSaveResult.outcome",
            "start_collection_inspection", "CollectionInspectResult.outcome", "CollectionInspection.facts", "CollectionInspection.discard", "start_collection_activation", "CollectionActivationResult.outcome", "JobProgress.setListener",
            "start_open", "start_create_empty", "prepare_provider_host", "close_collection", "collection_lifecycle", "wait_collection_available", "deck_tree_with_counts", "filtered_study_deck_scope", "filtered_study_definition", "custom_study_defaults", "start_filtered_study", "FilteredStudyJob.result", "all_deck_names", "match_deck_name", "deck_study_state", "deck_description_html", "activity_heatmap", "activity_heatmap_history", "studied_today_numbers", "start_stats_page", "start_release_stats_caches", "stats_home_idle", "stats_day_cards", "card_info", "card_info_debug_json",
            "start_session", "ReviewSessionJob.result", "peek_first_card", "session_head", "refresh_session_card", "render_session_head", "answer_session", "end_session", "peek_review_undo", "flush_review_for_background",
            "core_version", "core_build_info", "set_event_listener",
            "CancelToken.new", "CancelToken.cancel", "CancelToken.isCancelled", "render_card",
            "compare_typed_answer", "card_excerpts",
            "DeckDeletionJob.result", "add_deck", "start_rename_deck", "start_move_decks",
            "start_collapse_decks", "select_deck", "deck_deletion_summary", "start_delete_decks",
            "NoteDeletionJob.result", "add_note", "add_note_for_screen", "add_mined_note", "update_note", "patch_note", "card_deck", "note_cards", "read_note",
            "check_note_fields", "add_defaults", "add_notetype_selection", "notetype_last_deck", "tag_complete", "editor_field_html", "editor_paste_images_as_png", "start_delete_notes", "notetype_names", "read_notetype",
            "add_notetype", "add_stock_notetype", "clone_notetype", "ensure_image_occlusion_notetype", "editor_ensure_image_occlusion_notetype",
            "image_occlusion_parse", "image_occlusion_reading", "image_occlusion_allocate",
            "image_occlusion_write", "image_occlusion_numbers", "image_occlusion_detect",
            "rename_notetype", "set_notetype_css", "set_notetype_field_options", "set_notetype_template_options", "notetype_card_text", "notetype_card_text_edit",
            "TagMutationJob.result", "tag_tree", "flag_names", "set_tags_collapsed",
            "rename_flag", "restore_flag_name", "add_note_tags", "start_add_note_tags",
            "remove_note_tags", "start_remove_note_tags", "set_card_flag", "start_set_card_flag",
            "toggle_card_flag", "start_toggle_card_flag", "mark_notes", "start_mark_notes",
            "unmark_notes", "start_unmark_notes", "toggle_note_mark", "start_toggle_note_mark",
            "start_rename_tag", "start_reparent_tags", "start_delete_tags", "start_delete_tags_for_generation", "start_clear_unused_tags",
            "add_media", "editor_add_picture", "start_editor_image_fetch", "EditorImageFetchJob.result", "tracked_media_names", "flush_for_background", "suspend", "resume", "check_rollover", "fsrs_policy_status", "enable_fsrs",
            "UndoJob.result", "peek_undo", "peek_redo", "try_pending_review_restore",
            "DeckRenameJob.result", "DeckMoveJob.result", "DeckCollapseJob.result", "start_undo", "start_undo_step", "start_redo", "start_add_notes", "NoteAdditionJob.result",
            "parse_due_days", "start_sched_op", "SchedOpJob.result", "start_buried_kinds", "BuriedKindsJob.result",
            "DeckOptionsEditor.snapshot", "DeckOptionsEditor.edit", "DeckOptionsEditor.validate", "parse_option_steps", "format_option_steps", "start_deck_options", "start_save_deck_options", "start_option_ignore_count", "start_scheduling_preferences", "start_save_scheduling_preferences", "DeckOptionsReadJob.result", "DeckOptionsSaveJob.result", "OptionIgnoreCountJob.result", "SchedulingPreferencesJob.result", "SchedulingPreferencesSaveJob.result", "validate_scheduling_preferences",
            "notetype_list", "start_notetype_note_counts", "NotetypeNoteCountsJob.result", "start_notetype_field_content_counts", "NotetypeFieldContentCountsJob.result", "start_notetype_draft_summary", "NotetypeDraftSummaryJob.result", "start_notetype_notes_without_card", "NotetypeNotesWithoutCardJob.result", "start_notetype_removal_counts", "NotetypeRemovalCountsJob.result", "notetype_near_field_name",
            "start_notetype_removal", "start_notetype_removal_for_generation", "NotetypeRemovalJob.result", "start_notetype_structure_save", "NotetypeStructureJob.result", "notetype_check_field_name", "template_field_references",
            "start_notetype_template_card_counts", "NotetypeTemplateCardCountsJob.result", "notetype_template_issues",
            "notetype_replace_field_references", "notetype_template_syntax", "notetype_styling_syntax", "render_notetype_draft", "notetype_note_ids",
            "open_search_session", "BrowseSearchSession.start_search", "BrowseSearchSession.resort", "BrowseSearchSession.toggle", "BrowseSearchSession.reverse", "BrowseSearchSession.invalidated_rows", "BrowseSearchSession.rows", "BrowseSearchSession.qa_cells", "BrowseSearchSession.index_of", "BrowseSearchSession.map_selection", "BrowseSearchSession.resolve_selection", "BrowseSearchSession.selection", "BrowseSearchSession.handle_state", "BrowseSearchSession.close", "BrowseSelection.resolve",
            "filter_run_text", "filter_read_text", "filter_commit", "filter_text_incomplete", "filter_deck_value", "filter_suggestions", "start_filter_counts", "FilterCountsJob.result", "browser_column_set", "change_browser_column_set", "use_standard_browser_columns", "set_browser_notes_mode", "save_browser_search", "rename_browser_saved_search", "delete_browser_saved_searches", "start_browse_selection_counts", "BrowseSelectionCountsJob.result", "start_browse_selection_read", "BrowseSelectionReadJob.result", "BrowseSelection.sibling_rows", "browse_sheet_defaults", "browse_opening",
            "start_find_duplicates", "FindDuplicatesJob.result",
            "FindReplacePreviews.new", "FindReplacePreviews.start", "FindReplacePreviewJob.result", "start_find_replace", "BulkActionJob.result",
            "start_bulk_card_action_on_selection", "start_bulk_note_action_on_selection",
        ])
        for call in BridgeCall.all {
            #expect(call.interval.category == .bridge)
            #expect(!call.interval.fieldReported, "bridge calls never go to MetricKit")
            #expect(IntervalNames.named(call.name) == call.interval)
        }
    }

    @Test func imageOcclusionCallsUseExactOperationNames() {
        let calls: [BridgeCall] = [
            .imageOcclusionParse, .imageOcclusionReading, .imageOcclusionAllocate,
            .imageOcclusionWrite, .imageOcclusionNumbers, .imageOcclusionDetect,
        ]
        #expect(calls.map(\.name) == [
            "image_occlusion_parse", "image_occlusion_reading", "image_occlusion_allocate",
            "image_occlusion_write", "image_occlusion_numbers", "image_occlusion_detect",
        ])
        for call in calls {
            #expect(BridgeCall.all.filter { $0 == call } == [call])
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
