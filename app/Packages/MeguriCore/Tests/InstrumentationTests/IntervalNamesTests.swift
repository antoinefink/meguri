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
        #expect(BridgeCall.all.map(\.name) == [
            "test_cloud_zone_deletion_fixture", "test_cloud_zone_deletion_rows",
            "CloudPolicyExecution.begin_cleanup_pass", "CloudCleanupPass.submit_inventory", "CloudCleanupPass.complete_inventory", "CloudCleanupPass.finish",
            "CloudPolicyExecution.begin_zone_deletion_pass", "CloudZoneDeletionPass.active_target", "CloudZoneDeletionPass.submit_read", "CloudZoneDeletionPass.complete_read", "CloudZoneDeletionPass.submit_deletion", "CloudZoneDeletionPass.complete_deletion", "CloudZoneDeletionPass.finish",
            "CloudWorkflow.begin_first_asset_read_pass", "CloudPolicyExecution.begin_asset_read_pass", "CloudAssetReadPass.next", "CloudAssetReadPass.submit", "CloudAssetReadPass.complete", "CloudAssetReadPass.cancel", "CloudAssetReadPass.finish", "CloudAssetReadPass.operation_activity",
            "CloudWorkflow.media", "CloudWorkflow.submit_media_batch", "CloudMedia.downloads_blocked", "CloudPolicyExecution.begin_media", "CloudPolicyExecution.continue_media_after_send", "CloudMedia.next_work", "CloudMedia.start_next_work", "CloudMediaNextWorkResult.result", "CloudMedia.submit_read", "CloudMedia.queue_upload", "CloudMedia.reply",
            "CloudPolicyExecution.start_register_sdk", "CloudPolicyExecution.start_retire_original", "CloudPolicyRegistrationResult.result", "CloudPolicyRetirementResult.result",
            "CloudPolicy.start_input", "CloudPolicy.start_refresh_account", "CloudPolicy.start_project_identity_read", "CloudPolicy.start_status", "CloudPolicy.start_execute_core", "CloudPolicy.start_submit_native", "CloudPolicy.start_capture_exchange", "CloudPolicy.start_complete_exchange", "CloudPolicy.start_end_before_capture", "CloudPolicy.start_exchange_readiness", "CloudPolicy.start_prepare_execution", "CloudPolicyOutputResult.result", "CloudPolicyStatusResult.result", "CloudPolicyAdmissionResult.result", "CloudPolicyCaptureResult.result", "CloudPolicyReadinessResult.result", "CloudPolicyExecutionResult.result",
            "CloudWorkflow.policy", "CloudPolicy.validate_original_owner", "CloudPolicy.input", "CloudPolicy.refresh_account", "CloudPolicy.project_identity_read", "CloudPolicy.status", "CloudPolicy.execute_core", "CloudPolicy.submit_native", "CloudPolicy.capture_exchange", "CloudPolicy.complete_exchange", "CloudPolicy.end_before_capture",
            "CloudPolicyExecution.validate_control", "CloudPolicyExecution.prepare_flush", "CloudPolicy.exchange_readiness", "CloudPolicy.prepare_execution", "CloudPolicyExecution.original_sdk", "CloudPolicyExecution.validate_execution", "CloudPolicyExecution.register_sdk", "CloudPolicyExecution.retire_original", "CloudPolicyExecution.begin_fetch", "CloudPolicyExecution.begin_send", "CloudPolicyExecution.operation_activity",
            "CloudConnection.start_next_publication_work", "CloudConnection.start_resume_adoption", "CloudAdoptionResumeResult.result", "CloudConnection.start_adoption_result", "CloudPublicationWorkResult.result", "CloudAdoptionAppliedResult.result", "CloudAdoptionAppliedResult.failure_diagnostic",
            "CloudWorkflow.connection", "CloudConnection.begin_connection_preview", "CloudConnection.local_publication_information", "local_publication_information", "CloudConnection.connection_active_read", "CloudConnection.connection_epoch_read", "CloudConnection.cancel_preview", "CloudConnection.confirm_publication", "CloudConnection.confirm_adoption", "CloudConnection.resume_binding", "CloudConnection.next_publication_work", "CloudConnection.perform_local", "CloudConnection.submit_publication_request", "CloudConnection.publication_result", "CloudConnection.adoption_result", "CloudConnection.perform_adoption_local", "CloudConnection.cancel_operation", "CloudConnection.trigger", "CloudConnection.recovery_status",
            "configure_cloud_platform", "CloudWorkflow.new", "CloudWorkflow.instance", "CloudWorkflow.current_guard", "CloudWorkflow.entity_facts", "CloudWorkflow.await_tracking", "CloudWorkflow.tracking_sendables", "CloudWorkflow.pending_transition", "CloudWorkflow.emitted_update_ids", "CloudWorkflow.observations", "CloudWorkflow.poll_review", "CloudWorkflow.set_paused", "CloudWorkflow.stop_syncing", "CloudWorkflow.durable_snapshot", "CloudWorkflow.fetch_drains", "CloudWorkflow.diagnostics", "CloudWorkflow.status", "CloudWorkflow.exchange_result", "CloudWorkflow.begin_identity_read", "CloudWorkflow.complete_identity_read", "CloudWorkflow.prepare_runtime", "CloudWorkflow.register_sdk", "CloudWorkflow.validate_sdk", "CloudWorkflow.retire_sdk", "CloudWorkflow.release_sdk", "CloudWorkflow.account_changed", "CloudWorkflow.take_pending_updates", "CloudWorkflow.update_applied", "CloudWorkflow.build_batch", "CloudWorkflow.sent", "CloudWorkflow.handler_returned", "CloudWorkflow.tracking_failure", "CloudWorkflow.state_update", "CloudWorkflow.fetched", "CloudWorkflow.begin_fetch", "CloudWorkflow.will_fetch", "CloudWorkflow.fetch_zone", "CloudWorkflow.did_fetch", "CloudWorkflow.fetch_read_plan", "CloudFetchRead.target", "CloudFetchRead.validate", "CloudFetchRead.complete", "CloudWorkflow.finish_fetch", "CloudWorkflow.begin_exchange", "CloudWorkflow.exchange_fetched", "CloudWorkflow.complete_exchange", "CloudWorkflow.send_due", "CloudWorkflow.begin_send", "CloudWorkflow.finish_send"
        ] + mediaExports + [
            "normalize_manual_endpoint", "ManualTransferSession.new", "ManualTransferSession.start_setup", "ManualTransferSession.start_login", "ManualTransferSession.start_history", "ManualTransferSession.start_prepare", "ManualTransferSession.start_confirm", "ManualTransferSession.start_recovery_preview", "ManualTransferSession.start_cancel_history", "ManualTransferSession.start_cancel_preview", "ManualTransferSession.start_save_copy", "ManualTransferResult.outcome", "ManualTransferResult.operation_id", "ManualTransferPreview.record", "ManualTransferHistoryEntry.record",
            "start_create_backup", "CreateBackupResult.outcome", "start_restore_backup", "RestoreBackupResult.outcome", "start_run_due_maintenance", "DueMaintenanceResult.outcome", "list_backups", "backup_limits", "set_backup_limits", "maintenance_status",
            "start_collection_export", "CollectionExportResult.outcome", "CollectionExportArtifact.metadata", "CollectionExportArtifact.acquire", "CollectionExportArtifact.discard", "CollectionExportLease.path", "CollectionExportLease.finish", "start_collection_export_save", "CollectionExportSaveResult.outcome",
            "start_collection_inspection", "CollectionInspectResult.outcome", "CollectionInspection.facts", "CollectionInspection.discard", "start_collection_activation", "CollectionActivationResult.outcome", "JobProgress.setListener",
            "test_smoke_integrity", "test_cloud_smoke_observation", "test_cloud_smoke_batch", "test_cloud_smoke_review", "test_cloud_start_native_policy_open", "test_cloud_require_native_policy_baseline", "test_cloud_policy_exchange_observation", "start_open", "prepare_cloud_connection_host", "close_collection", "collection_lifecycle", "wait_collection_available", "deck_tree_with_counts", "all_deck_names", "match_deck_name", "deck_study_state", "deck_description_html",
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
