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
            "ProviderOwner.begin_icloud_discovery", "ProviderOwner.accept_icloud_connection", "ProviderOwner.begin_handoff", "ProviderOwner.finish_handoff",
            "ProviderOwner.recover_handoff_to_local_only", "ProviderOwner.retry_failed_publication",
            "ProviderOwner.configure_meguri", "ProviderOwner.configured_meguri", "ProviderConnection.information", "ProviderConnection.start_login", "ProviderLoginResult.result",
            "ProviderConnection.start_bootstrap_prepare", "ProviderConnection.start_bootstrap_confirm", "ProviderConnection.start_bootstrap_media_prepare",
            "ProviderConnection.start_bootstrap_media", "ProviderConnection.complete_bootstrap", "ProviderConnection.bootstrap_readiness",
            "ProviderBootstrapPreview.record", "ProviderBootstrapMediaConsent.record", "ProviderBootstrapOperation.record",
            "ProviderBootstrapResult.outcome", "ProviderBootstrapResult.media_snapshot", "ProviderBootstrapResult.request_stop",
            "ProviderBootstrapResult.retained_operation",
            "ProviderConnection.start_collection_round", "ProviderConnection.start_media_round", "ProviderRoundResult.outcome", "ProviderRoundResult.retry_finalization",
            "ProviderConnection.start_structure_save", "ProviderConnection.start_structure_upload", "ProviderStructureJob.advance", "ProviderStructureJob.cancel",
            "ProviderStructureJob.saved", "ProviderOwner.structure_upload_pending",
            "ProviderConnection.pending_bootstrap_preparation", "ProviderConnection.discard_bootstrap_preview",
            "ProviderConnection.discard_bootstrap_operation", "ProviderConnection.start_bootstrap_recovery",
            "test_cloud_zone_deletion_fixture", "test_cloud_zone_deletion_rows",
            "CloudPolicyExecution.begin_cleanup_pass", "CloudCleanupPass.submit_inventory", "CloudCleanupPass.complete_inventory", "CloudCleanupPass.finish",
            "CloudPolicyExecution.begin_zone_deletion_pass", "CloudZoneDeletionPass.active_target", "CloudZoneDeletionPass.submit_read", "CloudZoneDeletionPass.complete_read", "CloudZoneDeletionPass.submit_deletion", "CloudZoneDeletionPass.complete_deletion", "CloudZoneDeletionPass.finish",
            "CloudAssetReadPass.start_submit", "CloudAssetReadSubmitResult.result", "CloudAssetReadPass.start_complete", "CloudAssetReadCompleteResult.result", "CloudAssetReadPass.start_cancel", "CloudAssetReadCancelResult.result", "CloudAssetReadPass.start_finish", "CloudAssetReadFinishResult.result", "CloudAssetReadPass.start_operation_activity", "CloudAssetReadActivityResult.result",
            "CloudWorkflow.begin_first_asset_read_pass", "CloudPolicyExecution.begin_asset_read_pass", "CloudAssetReadPass.next", "CloudAssetReadPass.start_next", "CloudAssetReadNextResult.result", "CloudAssetReadPass.submit", "CloudAssetReadPass.complete", "CloudAssetReadPass.cancel", "CloudAssetReadPass.finish", "CloudAssetReadPass.operation_activity",
            "CloudWorkflow.media", "CloudWorkflow.submit_media_batch", "CloudMedia.downloads_blocked", "CloudPolicyExecution.begin_media", "CloudPolicyExecution.continue_media_after_send", "CloudMedia.next_work", "CloudMedia.start_next_work", "CloudMediaNextWorkResult.result", "CloudMedia.submit_read", "CloudMedia.queue_upload", "CloudMedia.reply",
            "CloudPolicyExecution.start_register_sdk", "CloudPolicyExecution.start_retire_original", "CloudPolicyRegistrationResult.result", "CloudPolicyRetirementResult.result",
            "CloudPolicyExecution.start_prepare_runtime_attempt", "CloudPolicyPreparationResult.detailed_result",
            "CloudPolicyExecution.start_report_invocation", "CloudPolicyExecution.start_report_started", "CloudPolicyExecution.start_report_completion", "CloudPolicyExecution.start_begin_media", "CloudPolicyMediaResult.result", "CloudPolicyExecution.start_begin_fetch", "CloudPolicyFetchResult.result", "CloudPolicyExecution.start_begin_send", "CloudPolicySendResult.result",
            "CloudPolicyExecution.start_validate_control", "CloudPolicyControlResult.result", "CloudPolicyExecution.start_continue_media_after_send", "CloudPolicyMediaContinuationResult.result", "CloudPolicyExecution.start_prepare_flush",
            "CloudPolicyExecution.start_begin_asset_read_pass", "CloudWorkflow.start_begin_first_asset_read_pass", "CloudPolicyAssetReadPassResult.result",
            "CloudPolicy.start_input", "CloudPolicy.start_refresh_account", "CloudPolicy.start_project_identity_read", "CloudPolicy.start_status", "CloudPolicy.start_execute_core", "CloudPolicy.start_submit_native", "CloudPolicy.start_capture_exchange", "CloudPolicy.start_complete_exchange", "CloudPolicy.start_end_before_capture", "CloudPolicy.start_exchange_readiness", "CloudPolicy.start_prepare_execution", "CloudPolicyOutputResult.detailed_result", "CloudPolicyExecutionResult.detailed_result", "CloudPolicyMediaResult.detailed_result", "CloudPolicyOutputResult.result", "CloudPolicyStatusResult.result", "CloudPolicyAdmissionResult.result", "CloudPolicyCaptureResult.result", "CloudPolicyReadinessResult.result", "CloudPolicyExecutionResult.result",
            "CloudWorkflow.policy", "CloudPolicy.validate_original_owner", "CloudPolicy.input", "CloudPolicy.refresh_account", "CloudPolicy.project_identity_read", "CloudPolicy.status", "CloudPolicy.execute_core", "CloudPolicy.submit_native", "CloudPolicy.capture_exchange", "CloudPolicy.complete_exchange", "CloudPolicy.end_before_capture",
            "CloudPolicyExecution.validate_control", "CloudPolicyExecution.prepare_flush", "CloudPolicy.exchange_readiness", "CloudPolicy.prepare_execution", "CloudPolicyExecution.original_sdk", "CloudPolicyExecution.validate_execution", "CloudPolicyExecution.register_sdk", "CloudPolicyExecution.retire_original", "CloudPolicyExecution.begin_fetch", "CloudPolicyExecution.begin_send", "CloudPolicyExecution.operation_activity",
            "CloudConnection.start_next_publication_work", "CloudConnection.start_resume_adoption", "CloudAdoptionResumeResult.result", "CloudConnection.start_adoption_result", "CloudPublicationWorkResult.result", "CloudAdoptionAppliedResult.result", "CloudAdoptionAppliedResult.failure_diagnostic",
            "CloudWorkflow.connection", "CloudConnection.begin_connection_preview", "CloudConnection.local_publication_information", "local_publication_information", "CloudConnection.connection_active_read", "CloudConnection.connection_epoch_read", "CloudConnection.cancel_preview", "CloudConnection.keep_local_only", "CloudConnection.confirm_publication", "CloudConnection.confirm_adoption", "CloudConnection.resume_binding", "CloudConnection.next_publication_work", "CloudConnection.perform_local", "CloudConnection.submit_publication_request", "CloudConnection.publication_result", "CloudConnection.adoption_result", "CloudConnection.perform_adoption_local", "CloudConnection.cancel_operation", "CloudConnection.trigger", "CloudConnection.recovery_status",
            "configure_cloud_platform", "CloudWorkflow.new", "CloudWorkflow.instance", "CloudWorkflow.current_guard", "CloudWorkflow.entity_facts", "CloudWorkflow.await_tracking", "CloudWorkflow.tracking_sendables", "CloudWorkflow.pending_transition", "CloudWorkflow.emitted_update_ids", "CloudWorkflow.observations", "CloudWorkflow.poll_review", "CloudWorkflow.set_paused", "CloudWorkflow.stop_syncing", "CloudWorkflow.durable_snapshot", "CloudWorkflow.fetch_drains", "cloud_diagnostics_export", "CloudWorkflow.diagnostics", "CloudWorkflow.status", "CloudWorkflow.exchange_result", "CloudWorkflow.begin_identity_read", "CloudWorkflow.complete_identity_read", "CloudWorkflow.prepare_runtime", "CloudWorkflow.register_sdk", "CloudWorkflow.validate_sdk", "CloudWorkflow.retire_sdk", "CloudWorkflow.release_sdk", "CloudWorkflow.account_changed", "CloudWorkflow.take_pending_updates", "CloudWorkflow.update_applied", "CloudWorkflow.build_batch", "CloudWorkflow.sent", "CloudWorkflow.handler_returned", "CloudWorkflow.sdk_saved_removed", "CloudWorkflow.tracking_failure", "CloudWorkflow.state_update", "CloudWorkflow.fetched_zone_losses", "CloudWorkflow.known_loss_read", "CloudWorkflow.validate_network_sdk", "CloudWorkflow.validate_prepared_batch", "CloudWorkflow.fetched_collection_zone_deletions", "CloudWorkflow.fetched", "CloudWorkflow.begin_fetch", "CloudWorkflow.will_fetch", "CloudWorkflow.fetch_zone", "CloudWorkflow.did_fetch", "CloudWorkflow.fetch_read_plan", "CloudWorkflow.sync_now_epoch_read", "CloudPolicyExecution.sync_now_epoch_read", "CloudWorkflow.sync_now_request", "CloudSyncNowRequest.epoch_read", "CloudSyncNowRequest.begin_exchange", "CloudFetchRead.cancel", "CloudFetchRead.validate_enqueue", "CloudFetchRead.epoch_preparation_outcome", "CloudFetchRead.target", "CloudFetchRead.validate", "CloudFetchRead.complete", "CloudWorkflow.finish_fetch", "CloudWorkflow.abort_fetch", "CloudWorkflow.begin_exchange", "CloudWorkflow.exchange_fetched", "CloudWorkflow.complete_exchange", "CloudWorkflow.send_due", "CloudWorkflow.begin_send", "CloudWorkflow.finish_send"
        ] + mediaExports + [
            "normalize_manual_endpoint", "ManualTransferSession.new", "ManualTransferSession.start_setup", "ManualTransferSession.start_login", "ManualTransferSession.start_history", "ManualTransferSession.start_prepare", "ManualTransferSession.start_confirm", "ManualTransferSession.start_recovery_preview", "ManualTransferSession.start_cancel_history", "ManualTransferSession.start_cancel_preview", "ManualTransferSession.start_save_copy", "ManualTransferResult.outcome", "ManualTransferResult.operation_id", "ManualTransferPreview.record", "ManualTransferHistoryEntry.record",
            "start_create_backup", "CreateBackupResult.outcome", "start_restore_backup", "RestoreBackupResult.outcome", "start_run_due_maintenance", "DueMaintenanceResult.outcome", "list_backups", "backup_limits", "set_backup_limits", "maintenance_status",
            "start_collection_export", "CollectionExportResult.outcome", "CollectionExportArtifact.metadata", "CollectionExportArtifact.acquire", "CollectionExportArtifact.discard", "CollectionExportLease.path", "CollectionExportLease.finish", "start_collection_export_save", "CollectionExportSaveResult.outcome",
            "start_collection_inspection", "CollectionInspectResult.outcome", "CollectionInspection.facts", "CollectionInspection.discard", "start_collection_activation", "CollectionActivationResult.outcome", "JobProgress.setListener",
            "CloudOriginalRole.read_original_ack", "CloudOriginalRole.read_original_edit", "test_cloud_original_control_witness", "test_cloud_restored_control", "test_cloud_original_exchange_witness", "test_cloud_original_exchange_capture", "test_cloud_original_exchange_work", "test_cloud_read_original_exchange", "test_cloud_crosscheck_original_ack_set", "test_cloud_selected_control", "test_cloud_selected_publication_event", "test_cloud_original_role", "CloudOriginalRole.facts", "CloudOriginalRole.validate_owned", "CloudOriginalRole.observe", "CloudOriginalRole.start_fixed_edit", "CloudOriginalEditResult.result", "test_smoke_integrity", "test_cloud_smoke_observation", "test_cloud_smoke_batch", "test_cloud_smoke_acknowledgements", "test_cloud_smoke_review", "test_cloud_start_native_policy_open", "test_cloud_require_native_policy_baseline", "test_cloud_policy_exchange_observation", "start_open", "start_create_empty", "prepare_cloud_connection_host", "close_collection", "collection_lifecycle", "wait_collection_available", "deck_tree_with_counts", "all_deck_names", "match_deck_name", "deck_study_state", "deck_description_html", "activity_heatmap", "studied_today_numbers",
            "start_session", "ReviewSessionJob.result", "peek_first_card", "session_head", "refresh_session_card", "render_session_head", "answer_session", "end_session", "peek_review_undo", "flush_review_for_background",
            "core_version", "core_build_info", "set_event_listener",
            "CancelToken.new", "CancelToken.cancel", "CancelToken.isCancelled", "render_card",
            "compare_typed_answer", "card_excerpts",
            "DeckDeletionJob.result", "add_deck", "start_rename_deck", "start_move_decks",
            "start_collapse_decks", "select_deck", "deck_deletion_summary", "start_delete_decks",
            "NoteDeletionJob.result", "add_note", "add_note_for_screen", "update_note", "patch_note", "card_deck", "read_note",
            "check_note_fields", "add_defaults", "add_notetype_selection", "notetype_last_deck", "tag_complete", "editor_field_html", "start_delete_notes", "notetype_names", "read_notetype",
            "add_notetype", "add_stock_notetype", "clone_notetype", "ensure_image_occlusion_notetype",
            "rename_notetype", "set_notetype_css", "set_notetype_field_options", "set_notetype_template_options", "notetype_card_text", "notetype_card_text_edit",
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
            "notetype_list", "start_notetype_note_counts", "NotetypeNoteCountsJob.result", "start_notetype_field_content_counts", "NotetypeFieldContentCountsJob.result", "start_notetype_draft_summary", "NotetypeDraftSummaryJob.result", "start_notetype_notes_without_card", "NotetypeNotesWithoutCardJob.result", "start_notetype_removal_counts", "NotetypeRemovalCountsJob.result", "notetype_near_field_name",
            "start_notetype_structure_save", "NotetypeStructureJob.result", "notetype_check_field_name", "template_field_references",
            "start_notetype_template_card_counts", "NotetypeTemplateCardCountsJob.result", "notetype_template_issues",
            "notetype_replace_field_references", "notetype_template_syntax", "notetype_styling_syntax",
            "open_search_session", "BrowseSearchSession.start_search", "BrowseSearchSession.resort", "BrowseSearchSession.toggle", "BrowseSearchSession.reverse", "BrowseSearchSession.invalidated_rows", "BrowseSearchSession.rows", "BrowseSearchSession.qa_cells", "BrowseSearchSession.index_of", "BrowseSearchSession.map_selection", "BrowseSearchSession.resolve_selection", "BrowseSearchSession.selection", "BrowseSearchSession.handle_state", "BrowseSearchSession.close", "BrowseSelection.resolve",
            "filter_run_text", "filter_read_text", "filter_commit", "filter_text_incomplete", "filter_deck_value", "filter_suggestions", "start_filter_counts", "FilterCountsJob.result", "browser_column_set", "change_browser_column_set", "use_standard_browser_columns", "set_browser_notes_mode", "save_browser_search", "start_browse_selection_counts", "BrowseSelectionCountsJob.result", "browse_opening",
            "start_find_duplicates", "FindDuplicatesJob.result",
            "FindReplacePreviews.new", "FindReplacePreviews.start", "FindReplacePreviewJob.result", "start_find_replace", "BulkActionJob.result",
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
