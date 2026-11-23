/// The single registry of timing intervals (R1861, U12). The app emits only
/// these names (`Signposts`), Instruments shows them, the UI performance
/// tests measure them (`XCTOSSignpostMetric`), field diagnostics store them
/// (MetricKit) and U241's budget catalog refers to them. A name that is not
/// here cannot be emitted: `IntervalName` and `BridgeCall` have no public
/// initializer, and `PerfTestCase` fails at setup on a name it cannot find.
///
/// One subsystem, one category per area: Instruments' "os_signpost" track,
/// `XCTOSSignpostMetric(subsystem:category:name:)` and MetricKit's
/// `signpostCategory` all use `subsystem`, `category.rawValue` and `name`.
///
/// Add an interval here when a unit starts emitting it, with what starts
/// and ends it; add a bridge call when a CoreBridge export starts calling
/// the core (`CoreLane.call` takes only these).
public enum IntervalNames {
    public static let subsystem = "com.antoinefink.meguri"

    // MARK: R1861: the user-facing intervals (also reported to MetricKit)

    /// Answer input (button, tap zone, key) → the display frame that presents
    /// the swap to the next card's question. The post-swap confirmation must
    /// name the same card (R1807, R1862 read per KTD11; VisibleConfirmation).
    public static let answerToNextVisible = IntervalName(
        "review.answerToNextVisible", .review, fieldReported: true)
    /// Reveal input → the frame that presents the answer side (R1809).
    public static let revealToAnswerVisible = IntervalName(
        "review.revealToAnswerVisible", .review, fieldReported: true)
    /// Launch → the deck list accepts input, with correct counts (R1818).
    public static let launchToDeckListInteractive = IntervalName(
        "launch.toDeckListInteractive", .launch, fieldReported: true)
    /// A keystroke in the search field → its results visible (R1824, R1825).
    public static let keystrokeToResultsVisible = IntervalName(
        "search.keystrokeToResultsVisible", .search, fieldReported: true)
    /// Opening the browser → its first page of rows visible (R1827).
    public static let browserOpenToFirstPage = IntervalName(
        "browser.openToFirstPage", .browse, fieldReported: true)
    /// Opening Stats → the first graphs drawn (R1837).
    public static let statsFirstGraphs = IntervalName(
        "stats.firstGraphs", .stats, fieldReported: true)
    /// Opening the editor on a note → its fields accept typing (R1833).
    public static let editorOpenToEditable = IntervalName(
        "editor.openToEditable", .editor, fieldReported: true)
    /// A sync starts → it ends, whatever the outcome (R1842, R1843).
    public static let syncStartToEnd = IntervalName(
        "sync.startToEnd", .sync, fieldReported: true)

    // MARK: Parts and other budgets (Instruments and tests only)

    /// Starting a review → the first card visible (R1813).
    public static let reviewStartToFirstCard = IntervalName("review.startToFirstCard", .review)
    /// Starting a review → both card pages loaded for its session: next to
    /// nothing when warm pages were waiting (a part of `startToFirstCard`).
    public static let reviewStartPagesReady = IntervalName("review.startPagesReady", .review)
    /// The session's first head → its first card ready in the idle page:
    /// render and page update, or a check of the preloaded card (a part of
    /// `startToFirstCard`).
    public static let reviewStartCardReady = IntervalName("review.startCardReady", .review)
    /// Card N presented → card N+1 settled in the idle page (U176, U241).
    public static let reviewPresentToNextSettled = IntervalName("review.presentToNextSettled", .review)
    /// A card rendered on demand because nothing was prepared (U176).
    public static let reviewOnDemandRender = IntervalName("review.onDemandRender", .review)
    /// The main-thread part of an answer (U175).
    public static let reviewAnswerMainThread = IntervalName("review.answerMainThread", .review)
    /// Undo input → the restored card visible (R1811).
    public static let reviewUndoToCardVisible = IntervalName("review.undoToCardVisible", .review)
    /// A ⋯-menu action (bury, suspend, flag, ...) → the next card visible
    /// (R1812).
    public static let reviewActionToNextVisible = IntervalName("review.actionToNextVisible", .review)
    /// Touch-down on the card → the press tint committed (R1816).
    public static let reviewTouchDownToTint = IntervalName("review.touchDownToTint", .review)
    /// Query dispatch (the debounce fired) → the first page visible (R1825).
    public static let searchFirstPage = IntervalName("search.firstPage", .search)
    /// Opening Stats → every graph card's data in the model (R1837, U237).
    public static let statsAllGraphs = IntervalName("stats.allGraphs", .stats)
    /// A scope change → the graphs redrawn (R1838).
    public static let statsScopeChange = IntervalName("stats.scopeChange", .stats)
    /// The Statistics page (U296, `stats/statistics-page` S19). Each starts when the page
    /// asks (it appears, or its deck or period changes) and ends in the frame that shows it.
    /// The page shown with a kept result → every box filled (R2076).
    public static let statsKeptOpen = IntervalName("stats.keptOpen", .stats)
    /// The page shown with nothing kept → every box filled (R2077).
    public static let statsColdOpen = IntervalName("stats.coldOpen", .stats)
    /// A refresh starts → the present tier's boxes and figures filled (S10.1 1, R2067).
    public static let statsPresentTier = IntervalName("stats.presentTier", .stats)
    /// A refresh starts → the review-log tier's boxes filled (S10.1 2, R1837).
    public static let statsLogTier = IntervalName("stats.logTier", .stats)
    /// A scroll of the page while a refresh reads (R2078): the window its hitches are measured in.
    public static let statsScrollDuringRead = IntervalName("stats.scrollDuringRead", .stats)
    /// A scroll of the filled page with nothing reading (R2078's settled case, the owner's report
    /// of 2026-11-23): the window its hitches are measured in.
    public static let statsScrollSettled = IntervalName("stats.scrollSettled", .stats)
    /// A switch or a period within the year → the boxes redrawn with no core call (R2079).
    public static let statsSelection = IntervalName("stats.selection", .stats)
    /// Heatmap data arrival → the heatmap drawn (R1839).
    public static let heatmapRender = IntervalName("heatmap.render", .stats)
    /// Opening Card Info → its content visible (R1841).
    public static let cardInfoOpen = IntervalName("cardInfo.open", .cardInfo)

    // MARK: Test probes

    /// The test hooks' probe route: a synthetic delay between a tap and its
    /// label changing (U12's smoke performance test). Never emitted outside
    /// Debug and Profile builds.
    public static let probeSyntheticDelay = IntervalName("probe.syntheticDelay", .probe)
    /// The rendering spike's baseline (U65, S1): input → the display frame
    /// that presents a swap of two plain native views, ended the way
    /// `answerToNextVisible` ends, so R1807 compares the page swap with it.
    /// Never emitted outside Debug and Profile builds.
    public static let probeNativeSwap = IntervalName("probe.nativeSwap", .probe)

    /// Every interval, in registry order.
    public static let all: [IntervalName] = [
        answerToNextVisible, revealToAnswerVisible, launchToDeckListInteractive, keystrokeToResultsVisible,
        browserOpenToFirstPage, statsFirstGraphs, editorOpenToEditable, syncStartToEnd,
        reviewStartToFirstCard, reviewStartPagesReady, reviewStartCardReady,
        reviewPresentToNextSettled, reviewOnDemandRender, reviewAnswerMainThread,
        reviewUndoToCardVisible, reviewActionToNextVisible, reviewTouchDownToTint, searchFirstPage,
        statsAllGraphs, statsScopeChange, statsKeptOpen, statsColdOpen, statsPresentTier, statsLogTier,
        statsScrollDuringRead, statsScrollSettled, statsSelection, heatmapRender, cardInfoOpen,
        probeSyntheticDelay, probeNativeSwap,
    ] + BridgeCall.all.map(\.interval)

    /// The interval registered under `name`, if any.
    public static func named(_ name: String) -> IntervalName? {
        all.first { $0.name == name }
    }
}

/// An area of the app: the signpost category its intervals use.
public enum IntervalCategory: String, Sendable, CaseIterable {
    case review = "Review"
    case launch = "Launch"
    case search = "Search"
    case browse = "Browse"
    case stats = "Stats"
    case cardInfo = "CardInfo"
    case editor = "Editor"
    case sync = "Sync"
    /// One interval per call into the core (CoreBridge lanes, U6).
    case bridge = "CoreBridge"
    /// Test-hook probes.
    case probe = "Probe"
}

/// A registered interval. Only `IntervalNames` and `BridgeCall` create
/// them.
public struct IntervalName: Sendable, Hashable, CustomStringConvertible {
    /// The signpost name, as OSSignposter and MetricKit take it.
    public let staticName: StaticString
    public let category: IntervalCategory
    /// Whether `Signposts` also emits it to MetricKit, so field diagnostics
    /// see it (R1861, R1867): the user-facing intervals only, never bridge
    /// calls.
    public let fieldReported: Bool

    init(_ name: StaticString, _ category: IntervalCategory, fieldReported: Bool = false) {
        staticName = name
        self.category = category
        self.fieldReported = fieldReported
    }

    public var name: String { staticName.description }
    public var subsystem: String { IntervalNames.subsystem }
    public var description: String { name }

    public static func == (lhs: IntervalName, rhs: IntervalName) -> Bool {
        lhs.name == rhs.name && lhs.category == rhs.category
    }

    public func hash(into hasher: inout Hasher) {
        hasher.combine(name)
        hasher.combine(category)
    }
}

/// A call into the core: CoreBridge's lanes signpost each one under its
/// name in the CoreBridge category (R1861, KTD9). The name is the export's.
public struct BridgeCall: Sendable, Hashable, CustomStringConvertible {
    public static let migrateProvider = BridgeCall("migrate_provider")
    public static let providerOwner = BridgeCall("provider_owner")
    public static let providerSnapshot = BridgeCall("ProviderOwner.snapshot")
    public static let providerRefresh = BridgeCall("ProviderOwner.refresh")
    public static let providerSaveSettings = BridgeCall("ProviderOwner.save_settings")
    public static let providerBeginDiscovery = BridgeCall("ProviderOwner.begin_icloud_discovery")
    public static let providerAcceptICloud = BridgeCall("ProviderOwner.accept_icloud_connection")
    public static let providerBeginHandoff = BridgeCall("ProviderOwner.begin_handoff")
    public static let providerFinishHandoff = BridgeCall("ProviderOwner.finish_handoff")
    public static let providerRecoverHandoffToLocalOnly = BridgeCall("ProviderOwner.recover_handoff_to_local_only")
    public static let providerRetryFailedPublication = BridgeCall("ProviderOwner.retry_failed_publication")
    public static let providerConfigureMeguri = BridgeCall("ProviderOwner.configure_meguri")
    public static let providerConfiguredMeguri = BridgeCall("ProviderOwner.configured_meguri")
    public static let providerConnectionInformation = BridgeCall("ProviderConnection.information")
    public static let providerStartLogin = BridgeCall("ProviderConnection.start_login")
    public static let providerLoginResult = BridgeCall("ProviderLoginResult.result")
    public static let providerBootstrapPrepare = BridgeCall("ProviderConnection.start_bootstrap_prepare")
    public static let providerBootstrapConfirm = BridgeCall("ProviderConnection.start_bootstrap_confirm")
    public static let providerBootstrapMediaPrepare = BridgeCall("ProviderConnection.start_bootstrap_media_prepare")
    public static let providerBootstrapMedia = BridgeCall("ProviderConnection.start_bootstrap_media")
    public static let providerBootstrapComplete = BridgeCall("ProviderConnection.complete_bootstrap")
    public static let providerBootstrapReadiness = BridgeCall("ProviderConnection.bootstrap_readiness")
    public static let providerBootstrapPreviewRecord = BridgeCall("ProviderBootstrapPreview.record")
    public static let providerBootstrapMediaConsentRecord = BridgeCall("ProviderBootstrapMediaConsent.record")
    public static let providerBootstrapOperationRecord = BridgeCall("ProviderBootstrapOperation.record")
    public static let providerBootstrapOutcome = BridgeCall("ProviderBootstrapResult.outcome")
    public static let providerBootstrapMediaSnapshot = BridgeCall("ProviderBootstrapResult.media_snapshot")
    public static let providerBootstrapRequestStop = BridgeCall("ProviderBootstrapResult.request_stop")
    public static let providerBootstrapRetainedOperation = BridgeCall("ProviderBootstrapResult.retained_operation")
    public static let providerStartCollectionRound = BridgeCall("ProviderConnection.start_collection_round")
    public static let providerStartMediaRound = BridgeCall("ProviderConnection.start_media_round")
    public static let providerRoundOutcome = BridgeCall("ProviderRoundResult.outcome")
    public static let providerRoundRetryFinalization = BridgeCall("ProviderRoundResult.retry_finalization")
    public static let providerStartStructureSave = BridgeCall("ProviderConnection.start_structure_save")
    public static let providerStartStructureUpload = BridgeCall("ProviderConnection.start_structure_upload")
    public static let providerStructureAdvance = BridgeCall("ProviderStructureJob.advance")
    public static let providerStructureCancel = BridgeCall("ProviderStructureJob.cancel")
    public static let providerStructureSaved = BridgeCall("ProviderStructureJob.saved")
    public static let providerStructureUploadPending = BridgeCall("ProviderOwner.structure_upload_pending")
    public static let providerBootstrapPendingPreparation = BridgeCall("ProviderConnection.pending_bootstrap_preparation")
    public static let providerBootstrapDiscardPreview = BridgeCall("ProviderConnection.discard_bootstrap_preview")
    public static let providerBootstrapDiscardOperation = BridgeCall("ProviderConnection.discard_bootstrap_operation")
    public static let providerBootstrapRecovery = BridgeCall("ProviderConnection.start_bootstrap_recovery")
    public let interval: IntervalName

    init(_ name: StaticString) {
        interval = IntervalName(name, .bridge)
    }

    public var name: String { interval.name }
    public var description: String { interval.name }

    public static let startCollectionInspection = BridgeCall("start_collection_inspection")
    public static let collectionInspectResultOutcome = BridgeCall("CollectionInspectResult.outcome")
    public static let collectionInspectionFacts = BridgeCall("CollectionInspection.facts")
    public static let collectionInspectionDiscard = BridgeCall("CollectionInspection.discard")
    public static let startCollectionActivation = BridgeCall("start_collection_activation")
    public static let collectionActivationResultOutcome = BridgeCall("CollectionActivationResult.outcome")
    public static let jobProgressSetListener = BridgeCall("JobProgress.setListener")

    public static let testCloudSelectedControl = BridgeCall("test_cloud_selected_control")
    public static let testCloudSelectedPublicationEvent = BridgeCall("test_cloud_selected_publication_event")
    public static let testCloudOriginalRole = BridgeCall("test_cloud_original_role")
    public static let cloudOriginalRoleFacts = BridgeCall("CloudOriginalRole.facts")
    public static let cloudOriginalRoleStartFixedEdit = BridgeCall("CloudOriginalRole.start_fixed_edit")
    public static let cloudOriginalEditResult = BridgeCall("CloudOriginalEditResult.result")
    public static let cloudOriginalRoleValidateOwned = BridgeCall("CloudOriginalRole.validate_owned")
    public static let cloudOriginalRoleReadOriginalAck = BridgeCall("CloudOriginalRole.read_original_ack")
    public static let cloudOriginalRoleReadOriginalEdit = BridgeCall("CloudOriginalRole.read_original_edit")
    public static let testCloudOriginalControlWitness = BridgeCall("test_cloud_original_control_witness")
    public static let testCloudRestoredControl = BridgeCall("test_cloud_restored_control")
    public static let testCloudOriginalExchangeWitness = BridgeCall("test_cloud_original_exchange_witness")
    public static let testCloudOriginalExchangeCapture = BridgeCall("test_cloud_original_exchange_capture")
    public static let testCloudOriginalExchangeWork = BridgeCall("test_cloud_original_exchange_work")
    public static let testCloudReadOriginalExchange = BridgeCall("test_cloud_read_original_exchange")
    public static let testCloudCrosscheckOriginalAckSet = BridgeCall("test_cloud_crosscheck_original_ack_set")
    public static let cloudOriginalRoleObserve = BridgeCall("CloudOriginalRole.observe")
    public static let testSmokeIntegrity = BridgeCall("test_smoke_integrity")
    public static let testCloudSmokeBatch = BridgeCall("test_cloud_smoke_batch")
    public static let testCloudSmokeAcknowledgements = BridgeCall("test_cloud_smoke_acknowledgements")
    public static let testCloudSmokeReview = BridgeCall("test_cloud_smoke_review")
    public static let testCloudSmokeObservation = BridgeCall("test_cloud_smoke_observation")
    public static let testCloudStartNativePolicyOpen = BridgeCall("test_cloud_start_native_policy_open")
    public static let testCloudRequireNativePolicyBaseline = BridgeCall("test_cloud_require_native_policy_baseline")
    public static let testCloudPolicyExchangeObservation = BridgeCall("test_cloud_policy_exchange_observation")
    public static let startOpen = BridgeCall("start_open")
    public static let startCreateEmpty = BridgeCall("start_create_empty")
    public static let prepareCloudConnectionHost = BridgeCall("prepare_cloud_connection_host")
    public static let closeCollection = BridgeCall("close_collection")
    public static let collectionLifecycle = BridgeCall("collection_lifecycle")
    public static let waitCollectionAvailable = BridgeCall("wait_collection_available")
    public static let deckTreeWithCounts = BridgeCall("deck_tree_with_counts")
    public static let allDeckNames = BridgeCall("all_deck_names")
    public static let matchDeckName = BridgeCall("match_deck_name")
    public static let deckStudyState = BridgeCall("deck_study_state")
    public static let deckDescriptionHtml = BridgeCall("deck_description_html")
    public static let activityHeatmap = BridgeCall("activity_heatmap")
    public static let studiedTodayNumbers = BridgeCall("studied_today_numbers")
    public static let startStatsPage = BridgeCall("start_stats_page")
    public static let startReleaseStatsCaches = BridgeCall("start_release_stats_caches")
    public static let statsHomeIdle = BridgeCall("stats_home_idle")
    public static let statsDayCards = BridgeCall("stats_day_cards")

    public static let startSession = BridgeCall("start_session")
    public static let reviewSessionJobResult = BridgeCall("ReviewSessionJob.result")
    public static let peekFirstCard = BridgeCall("peek_first_card")
    public static let sessionHead = BridgeCall("session_head")
    public static let refreshSessionCard = BridgeCall("refresh_session_card")
    public static let renderSessionHead = BridgeCall("render_session_head")
    public static let answerSession = BridgeCall("answer_session")
    public static let endSession = BridgeCall("end_session")
    public static let peekReviewUndo = BridgeCall("peek_review_undo")
    public static let flushReviewForBackground = BridgeCall("flush_review_for_background")

    public static let compareTypedAnswer = BridgeCall("compare_typed_answer")
    public static let renderCard = BridgeCall("render_card")
    public static let cardExcerpts = BridgeCall("card_excerpts")
    public static let coreVersion = BridgeCall("core_version")
    public static let coreBuildInfo = BridgeCall("core_build_info")
    public static let setEventListener = BridgeCall("set_event_listener")
    public static let cancelTokenNew = BridgeCall("CancelToken.new")
    public static let cancelTokenCancel = BridgeCall("CancelToken.cancel")
    public static let cancelTokenIsCancelled = BridgeCall("CancelToken.isCancelled")

    public static let deckDeletionJobResult = BridgeCall("DeckDeletionJob.result")
    public static let addDeck = BridgeCall("add_deck")
    public static let startRenameDeck = BridgeCall("start_rename_deck")
    public static let startMoveDecks = BridgeCall("start_move_decks")
    public static let startCollapseDecks = BridgeCall("start_collapse_decks")
    public static let selectDeck = BridgeCall("select_deck")
    public static let deckDeletionSummary = BridgeCall("deck_deletion_summary")
    public static let startDeleteDecks = BridgeCall("start_delete_decks")
    public static let noteDeletionJobResult = BridgeCall("NoteDeletionJob.result")
    public static let addNote = BridgeCall("add_note")
    public static let addNoteForScreen = BridgeCall("add_note_for_screen")
    public static let updateNote = BridgeCall("update_note")
    public static let patchNote = BridgeCall("patch_note")
    public static let cardDeck = BridgeCall("card_deck")
    public static let readNote = BridgeCall("read_note")
    public static let checkNoteFields = BridgeCall("check_note_fields")
    public static let addDefaults = BridgeCall("add_defaults")
    public static let addNotetypeSelection = BridgeCall("add_notetype_selection")
    public static let notetypeLastDeck = BridgeCall("notetype_last_deck")
    public static let tagComplete = BridgeCall("tag_complete")
    public static let editorFieldHTML = BridgeCall("editor_field_html")
    public static let startDeleteNotes = BridgeCall("start_delete_notes")
    public static let notetypeNames = BridgeCall("notetype_names")
    public static let readNotetype = BridgeCall("read_notetype")
    public static let addNotetype = BridgeCall("add_notetype")
    public static let addStockNotetype = BridgeCall("add_stock_notetype")
    public static let cloneNotetype = BridgeCall("clone_notetype")
    public static let ensureImageOcclusionNotetype = BridgeCall("ensure_image_occlusion_notetype")
    public static let renameNotetype = BridgeCall("rename_notetype")
    public static let setNotetypeCss = BridgeCall("set_notetype_css")
    public static let setNotetypeFieldOptions = BridgeCall("set_notetype_field_options")
    public static let setNotetypeTemplateOptions = BridgeCall("set_notetype_template_options")
    public static let notetypeCardText = BridgeCall("notetype_card_text")
    public static let notetypeCardTextEdit = BridgeCall("notetype_card_text_edit")
    public static let tagMutationJobResult = BridgeCall("TagMutationJob.result")
    public static let tagTree = BridgeCall("tag_tree")
    public static let flagNames = BridgeCall("flag_names")
    public static let setTagsCollapsed = BridgeCall("set_tags_collapsed")
    public static let renameFlag = BridgeCall("rename_flag")
    public static let restoreFlagName = BridgeCall("restore_flag_name")
    public static let addNoteTags = BridgeCall("add_note_tags")
    public static let startAddNoteTags = BridgeCall("start_add_note_tags")
    public static let removeNoteTags = BridgeCall("remove_note_tags")
    public static let startRemoveNoteTags = BridgeCall("start_remove_note_tags")
    public static let setCardFlag = BridgeCall("set_card_flag")
    public static let startSetCardFlag = BridgeCall("start_set_card_flag")
    public static let toggleCardFlag = BridgeCall("toggle_card_flag")
    public static let startToggleCardFlag = BridgeCall("start_toggle_card_flag")
    public static let markNotes = BridgeCall("mark_notes")
    public static let startMarkNotes = BridgeCall("start_mark_notes")
    public static let unmarkNotes = BridgeCall("unmark_notes")
    public static let startUnmarkNotes = BridgeCall("start_unmark_notes")
    public static let toggleNoteMark = BridgeCall("toggle_note_mark")
    public static let startToggleNoteMark = BridgeCall("start_toggle_note_mark")
    public static let startRenameTag = BridgeCall("start_rename_tag")
    public static let startReparentTags = BridgeCall("start_reparent_tags")
    public static let startDeleteTags = BridgeCall("start_delete_tags")
    public static let startClearUnusedTags = BridgeCall("start_clear_unused_tags")
    public static let trackedMediaNames = BridgeCall("tracked_media_names")
    public static let addMedia = BridgeCall("add_media")
    public static let flushForBackground = BridgeCall("flush_for_background")
    public static let suspend = BridgeCall("suspend")
    public static let resume = BridgeCall("resume")
    public static let checkRollover = BridgeCall("check_rollover")
    public static let fsrsPolicyStatus = BridgeCall("fsrs_policy_status")
    public static let enableFsrs = BridgeCall("enable_fsrs")
    public static let undoJobResult = BridgeCall("UndoJob.result")
    public static let peekUndo = BridgeCall("peek_undo")
    public static let peekRedo = BridgeCall("peek_redo")
    public static let tryPendingReviewRestore = BridgeCall("try_pending_review_restore")
    public static let deckRenameJobResult = BridgeCall("DeckRenameJob.result")
    public static let deckMoveJobResult = BridgeCall("DeckMoveJob.result")
    public static let deckCollapseJobResult = BridgeCall("DeckCollapseJob.result")
    public static let startUndo = BridgeCall("start_undo")
    public static let startRedo = BridgeCall("start_redo")


    public static let startAddNotes = BridgeCall("start_add_notes")
    public static let noteAdditionJobResult = BridgeCall("NoteAdditionJob.result")

    public static let parseDueDays = BridgeCall("parse_due_days")
    public static let startSchedOp = BridgeCall("start_sched_op")
    public static let schedOpJobResult = BridgeCall("SchedOpJob.result")
    public static let startBuriedKinds = BridgeCall("start_buried_kinds")
    public static let buriedKindsJobResult = BridgeCall("BuriedKindsJob.result")

    public static let deckOptionsEditorSnapshot = BridgeCall("DeckOptionsEditor.snapshot")
    public static let deckOptionsEditorEdit = BridgeCall("DeckOptionsEditor.edit")
    public static let deckOptionsEditorValidate = BridgeCall("DeckOptionsEditor.validate")
    public static let parseOptionSteps = BridgeCall("parse_option_steps")
    public static let formatOptionSteps = BridgeCall("format_option_steps")
    public static let startDeckOptions = BridgeCall("start_deck_options")
    public static let startSaveDeckOptions = BridgeCall("start_save_deck_options")
    public static let startOptionIgnoreCount = BridgeCall("start_option_ignore_count")
    public static let startSchedulingPreferences = BridgeCall("start_scheduling_preferences")
    public static let startSaveSchedulingPreferences = BridgeCall("start_save_scheduling_preferences")
    public static let deckOptionsReadJobResult = BridgeCall("DeckOptionsReadJob.result")
    public static let deckOptionsSaveJobResult = BridgeCall("DeckOptionsSaveJob.result")
    public static let optionIgnoreCountJobResult = BridgeCall("OptionIgnoreCountJob.result")
    public static let schedulingPreferencesJobResult = BridgeCall("SchedulingPreferencesJob.result")
    public static let schedulingPreferencesSaveJobResult = BridgeCall("SchedulingPreferencesSaveJob.result")

    public static let validateSchedulingPreferences = BridgeCall("validate_scheduling_preferences")

    public static let notetypeList = BridgeCall("notetype_list")
    public static let startNotetypeNoteCounts = BridgeCall("start_notetype_note_counts")
    public static let notetypeNoteCountsJobResult = BridgeCall("NotetypeNoteCountsJob.result")
    public static let startNotetypeFieldContentCounts = BridgeCall("start_notetype_field_content_counts")
    public static let notetypeFieldContentCountsJobResult = BridgeCall("NotetypeFieldContentCountsJob.result")
    public static let startNotetypeDraftSummary = BridgeCall("start_notetype_draft_summary")
    public static let notetypeDraftSummaryJobResult = BridgeCall("NotetypeDraftSummaryJob.result")
    public static let startNotetypeNotesWithoutCard = BridgeCall("start_notetype_notes_without_card")
    public static let notetypeNotesWithoutCardJobResult = BridgeCall("NotetypeNotesWithoutCardJob.result")
    public static let startNotetypeRemovalCounts = BridgeCall("start_notetype_removal_counts")
    public static let notetypeRemovalCountsJobResult = BridgeCall("NotetypeRemovalCountsJob.result")
    public static let notetypeNearFieldName = BridgeCall("notetype_near_field_name")

    public static let startNotetypeStructureSave = BridgeCall("start_notetype_structure_save")
    public static let notetypeStructureJobResult = BridgeCall("NotetypeStructureJob.result")
    public static let notetypeCheckFieldName = BridgeCall("notetype_check_field_name")
    public static let templateFieldReferences = BridgeCall("template_field_references")
    public static let startNotetypeTemplateCardCounts = BridgeCall("start_notetype_template_card_counts")
    public static let notetypeTemplateCardCountsJobResult = BridgeCall("NotetypeTemplateCardCountsJob.result")
    public static let notetypeTemplateIssues = BridgeCall("notetype_template_issues")
    public static let notetypeReplaceFieldReferences = BridgeCall("notetype_replace_field_references")
    public static let notetypeTemplateSyntax = BridgeCall("notetype_template_syntax")
    public static let notetypeStylingSyntax = BridgeCall("notetype_styling_syntax")

    public static let openSearchSession = BridgeCall("open_search_session")
    public static let browseStartSearch = BridgeCall("BrowseSearchSession.start_search")
    public static let browseResort = BridgeCall("BrowseSearchSession.resort")
    public static let browseToggle = BridgeCall("BrowseSearchSession.toggle")
    public static let browseReverse = BridgeCall("BrowseSearchSession.reverse")
    public static let browseInvalidatedRows = BridgeCall("BrowseSearchSession.invalidated_rows")
    public static let browseRows = BridgeCall("BrowseSearchSession.rows")
    public static let browseQaCells = BridgeCall("BrowseSearchSession.qa_cells")
    public static let browseIndexOf = BridgeCall("BrowseSearchSession.index_of")
    public static let browseMapSelection = BridgeCall("BrowseSearchSession.map_selection")
    public static let browseResolveSelection = BridgeCall("BrowseSearchSession.resolve_selection")
    public static let browseSelection = BridgeCall("BrowseSearchSession.selection")
    public static let browseHandleState = BridgeCall("BrowseSearchSession.handle_state")
    public static let browseClose = BridgeCall("BrowseSearchSession.close")
    public static let browseSelectionResolve = BridgeCall("BrowseSelection.resolve")

    public static let filterRunText = BridgeCall("filter_run_text")
    public static let filterReadText = BridgeCall("filter_read_text")
    public static let filterCommit = BridgeCall("filter_commit")
    public static let filterTextIncomplete = BridgeCall("filter_text_incomplete")
    public static let filterDeckValue = BridgeCall("filter_deck_value")
    public static let filterSuggestions = BridgeCall("filter_suggestions")
    public static let startFilterCounts = BridgeCall("start_filter_counts")
    public static let filterCountsJobResult = BridgeCall("FilterCountsJob.result")
    public static let browserColumnSet = BridgeCall("browser_column_set")
    public static let changeBrowserColumnSet = BridgeCall("change_browser_column_set")
    public static let useStandardBrowserColumns = BridgeCall("use_standard_browser_columns")
    public static let setBrowserNotesMode = BridgeCall("set_browser_notes_mode")
    public static let saveBrowserSearch = BridgeCall("save_browser_search")
    public static let startBrowseSelectionCounts = BridgeCall("start_browse_selection_counts")
    public static let browseSelectionCountsJobResult = BridgeCall("BrowseSelectionCountsJob.result")
    public static let browseOpening = BridgeCall("browse_opening")
    public static let startFindDuplicates = BridgeCall("start_find_duplicates")
    public static let findDuplicatesJobResult = BridgeCall("FindDuplicatesJob.result")
    public static let findReplacePreviewsNew = BridgeCall("FindReplacePreviews.new")
    public static let findReplacePreviewsStart = BridgeCall("FindReplacePreviews.start")
    public static let findReplacePreviewJobResult = BridgeCall("FindReplacePreviewJob.result")
    public static let startFindReplace = BridgeCall("start_find_replace")
    public static let bulkActionJobResult = BridgeCall("BulkActionJob.result")

    public static let startCreateBackup = BridgeCall("start_create_backup")
    public static let createBackupResultOutcome = BridgeCall("CreateBackupResult.outcome")
    public static let startRestoreBackup = BridgeCall("start_restore_backup")
    public static let restoreBackupResultOutcome = BridgeCall("RestoreBackupResult.outcome")
    public static let startRunDueMaintenance = BridgeCall("start_run_due_maintenance")
    public static let dueMaintenanceResultOutcome = BridgeCall("DueMaintenanceResult.outcome")
    public static let listBackups = BridgeCall("list_backups")
    public static let backupLimits = BridgeCall("backup_limits")
    public static let setBackupLimits = BridgeCall("set_backup_limits")
    public static let maintenanceStatus = BridgeCall("maintenance_status")

    public static let startCollectionExport = BridgeCall("start_collection_export")
    public static let collectionExportResultOutcome = BridgeCall("CollectionExportResult.outcome")
    public static let collectionExportArtifactMetadata = BridgeCall("CollectionExportArtifact.metadata")
    public static let collectionExportArtifactAcquire = BridgeCall("CollectionExportArtifact.acquire")
    public static let collectionExportArtifactDiscard = BridgeCall("CollectionExportArtifact.discard")
    public static let collectionExportLeaseFinish = BridgeCall("CollectionExportLease.finish")
    public static let collectionExportLeasePath = BridgeCall("CollectionExportLease.path")
    public static let startCollectionExportSave = BridgeCall("start_collection_export_save")
    public static let collectionExportSaveResultOutcome = BridgeCall("CollectionExportSaveResult.outcome")

    public static let normalizeManualEndpoint = BridgeCall("normalize_manual_endpoint")
    /// Media decisions and their worker controls, keyed by the FFI export names.
    public enum ManualMediaCalls {
        public static let preview = BridgeCall("ManualTransferSession.start_media_preview")
        public static let run = BridgeCall("ManualTransferSession.start_media")
        public static let cancelPrepared = BridgeCall("ManualTransferSession.start_cancel_prepared_media")
        public static let forgetOwner = BridgeCall("ManualTransferSession.start_invalidate_media_owner")
        public static let readPreview = BridgeCall("ManualMediaPreview.record")
        public static let readProgress = BridgeCall("ManualTransferResult.media_snapshot")
        public static let stop = BridgeCall("ManualTransferResult.request_stop")
    }
    public static let manualTransferSessionNew = BridgeCall("ManualTransferSession.new")
    public static let manualTransferSessionStartSetup = BridgeCall("ManualTransferSession.start_setup")
    public static let manualTransferSessionStartLogin = BridgeCall("ManualTransferSession.start_login")
    public static let manualTransferSessionStartHistory = BridgeCall("ManualTransferSession.start_history")
    public static let manualTransferSessionStartPrepare = BridgeCall("ManualTransferSession.start_prepare")
    public static let manualTransferSessionStartConfirm = BridgeCall("ManualTransferSession.start_confirm")
    public static let manualTransferSessionStartRecoveryPreview = BridgeCall("ManualTransferSession.start_recovery_preview")
    public static let manualTransferSessionStartCancelHistory = BridgeCall("ManualTransferSession.start_cancel_history")
    public static let manualTransferSessionStartCancelPreview = BridgeCall("ManualTransferSession.start_cancel_preview")
    public static let manualTransferSessionStartSaveCopy = BridgeCall("ManualTransferSession.start_save_copy")
    public static let manualTransferResultOperationID = BridgeCall("ManualTransferResult.operation_id")
    public static let manualTransferResultOutcome = BridgeCall("ManualTransferResult.outcome")
    public static let manualTransferPreviewRecord = BridgeCall("ManualTransferPreview.record")
    public static let manualTransferHistoryEntryRecord = BridgeCall("ManualTransferHistoryEntry.record")

    public static let cloudWorkflowPolicy = BridgeCall("CloudWorkflow.policy")
    public static let cloudPolicyValidateOriginalOwner = BridgeCall("CloudPolicy.validate_original_owner")
    public static let cloudPolicyStartInput = BridgeCall("CloudPolicy.start_input")
    public static let cloudPolicyStartRefreshAccount = BridgeCall("CloudPolicy.start_refresh_account")
    public static let cloudPolicyStartProjectIdentityRead = BridgeCall("CloudPolicy.start_project_identity_read")
    public static let cloudPolicyStartStatus = BridgeCall("CloudPolicy.start_status")
    public static let cloudPolicyStartExecuteCore = BridgeCall("CloudPolicy.start_execute_core")
    public static let cloudPolicyStartSubmitNative = BridgeCall("CloudPolicy.start_submit_native")
    public static let cloudPolicyStartCaptureExchange = BridgeCall("CloudPolicy.start_capture_exchange")
    public static let cloudPolicyStartCompleteExchange = BridgeCall("CloudPolicy.start_complete_exchange")
    public static let cloudPolicyStartEndBeforeCapture = BridgeCall("CloudPolicy.start_end_before_capture")
    public static let cloudPolicyStartExchangeReadiness = BridgeCall("CloudPolicy.start_exchange_readiness")
    public static let cloudPolicyStartPrepareExecution = BridgeCall("CloudPolicy.start_prepare_execution")
    public static let cloudPolicyMediaDetailedResult = BridgeCall("CloudPolicyMediaResult.detailed_result")
    public static let cloudPolicyOutputDetailedResult = BridgeCall("CloudPolicyOutputResult.detailed_result")
    public static let cloudPolicyExecutionDetailedResult = BridgeCall("CloudPolicyExecutionResult.detailed_result")
    public static let cloudPolicyOutputResult = BridgeCall("CloudPolicyOutputResult.result")
    public static let cloudPolicyStatusResult = BridgeCall("CloudPolicyStatusResult.result")
    public static let cloudPolicyAdmissionResult = BridgeCall("CloudPolicyAdmissionResult.result")
    public static let cloudPolicyCaptureResult = BridgeCall("CloudPolicyCaptureResult.result")
    public static let cloudPolicyReadinessResult = BridgeCall("CloudPolicyReadinessResult.result")
    public static let cloudPolicyExecutionResult = BridgeCall("CloudPolicyExecutionResult.result")
    public static let cloudPolicyExecutionStartRegisterSdk = BridgeCall("CloudPolicyExecution.start_register_sdk")
    public static let cloudPolicyExecutionStartPrepareRuntimeAttempt = BridgeCall("CloudPolicyExecution.start_prepare_runtime_attempt")
    public static let cloudPolicyPreparationDetailedResult = BridgeCall("CloudPolicyPreparationResult.detailed_result")
    public static let cloudPolicyExecutionStartRetireOriginal = BridgeCall("CloudPolicyExecution.start_retire_original")
    public static let cloudPolicyRegistrationResult = BridgeCall("CloudPolicyRegistrationResult.result")
    public static let cloudPolicyRetirementResult = BridgeCall("CloudPolicyRetirementResult.result")
    public static let cloudPolicyExecutionStartReportInvocation = BridgeCall("CloudPolicyExecution.start_report_invocation")
    public static let cloudPolicyExecutionStartReportStarted = BridgeCall("CloudPolicyExecution.start_report_started")
    public static let cloudPolicyExecutionStartReportCompletion = BridgeCall("CloudPolicyExecution.start_report_completion")
    public static let cloudPolicyExecutionStartBeginMedia = BridgeCall("CloudPolicyExecution.start_begin_media")
    public static let cloudPolicyMediaResult = BridgeCall("CloudPolicyMediaResult.result")
    public static let cloudPolicyExecutionStartBeginFetch = BridgeCall("CloudPolicyExecution.start_begin_fetch")
    public static let cloudPolicyFetchResult = BridgeCall("CloudPolicyFetchResult.result")
    public static let cloudPolicyExecutionStartBeginSend = BridgeCall("CloudPolicyExecution.start_begin_send")
    public static let cloudPolicySendResult = BridgeCall("CloudPolicySendResult.result")
    public static let cloudPolicyExecutionStartValidateControl = BridgeCall("CloudPolicyExecution.start_validate_control")
    public static let cloudPolicyControlResult = BridgeCall("CloudPolicyControlResult.result")
    public static let cloudPolicyExecutionStartContinueMediaAfterSend = BridgeCall("CloudPolicyExecution.start_continue_media_after_send")
    public static let cloudPolicyMediaContinuationResult = BridgeCall("CloudPolicyMediaContinuationResult.result")
    public static let cloudPolicyExecutionStartPrepareFlush = BridgeCall("CloudPolicyExecution.start_prepare_flush")
    public static let cloudPolicyExecutionStartBeginAssetReadPass = BridgeCall("CloudPolicyExecution.start_begin_asset_read_pass")
    public static let cloudWorkflowStartBeginFirstAssetReadPass = BridgeCall("CloudWorkflow.start_begin_first_asset_read_pass")
    public static let cloudPolicyAssetReadPassResult = BridgeCall("CloudPolicyAssetReadPassResult.result")
    public static let cloudPolicyInput = BridgeCall("CloudPolicy.input")
    public static let cloudPolicyRefreshAccount = BridgeCall("CloudPolicy.refresh_account")
    public static let cloudPolicyProjectIdentityRead = BridgeCall("CloudPolicy.project_identity_read")
    public static let cloudPolicyStatus = BridgeCall("CloudPolicy.status")
    public static let cloudPolicyExecuteCore = BridgeCall("CloudPolicy.execute_core")
    public static let cloudPolicySubmitNative = BridgeCall("CloudPolicy.submit_native")
    public static let cloudPolicyCaptureExchange = BridgeCall("CloudPolicy.capture_exchange")
    public static let cloudPolicyCompleteExchange = BridgeCall("CloudPolicy.complete_exchange")
    public static let cloudPolicyEndBeforeCapture = BridgeCall("CloudPolicy.end_before_capture")
    public static let cloudPolicyExchangeReadiness = BridgeCall("CloudPolicy.exchange_readiness")
    public static let cloudPolicyPrepareExecution = BridgeCall("CloudPolicy.prepare_execution")
    public static let cloudPolicyExecutionOriginalSdk = BridgeCall("CloudPolicyExecution.original_sdk")
    public static let cloudPolicyExecutionValidateExecution = BridgeCall("CloudPolicyExecution.validate_execution")
    public static let cloudPolicyExecutionRegisterSdk = BridgeCall("CloudPolicyExecution.register_sdk")
    public static let cloudPolicyExecutionRetireOriginal = BridgeCall("CloudPolicyExecution.retire_original")
    public static let cloudPolicyExecutionOperationActivity = BridgeCall("CloudPolicyExecution.operation_activity")
    public static let cloudPolicyExecutionBeginFetch = BridgeCall("CloudPolicyExecution.begin_fetch")
    public static let cloudPolicyExecutionValidateControl = BridgeCall("CloudPolicyExecution.validate_control")
    public static let cloudPolicyExecutionPrepareFlush = BridgeCall("CloudPolicyExecution.prepare_flush")
    public static let cloudPolicyExecutionBeginSend = BridgeCall("CloudPolicyExecution.begin_send")

    public static let cloudWorkflowConnection = BridgeCall("CloudWorkflow.connection")
    public static let cloudWorkflowMedia = BridgeCall("CloudWorkflow.media")
    public static let cloudWorkflowBeginFirstAssetReadPass = BridgeCall("CloudWorkflow.begin_first_asset_read_pass")
    public static let cloudPolicyExecutionBeginAssetReadPass = BridgeCall("CloudPolicyExecution.begin_asset_read_pass")
    public static let testCloudZoneDeletionFixture = BridgeCall("test_cloud_zone_deletion_fixture")
    public static let testCloudZoneDeletionRows = BridgeCall("test_cloud_zone_deletion_rows")
    public static let cloudPolicyExecutionBeginCleanupPass = BridgeCall("CloudPolicyExecution.begin_cleanup_pass")
    public static let cloudCleanupPassSubmitInventory = BridgeCall("CloudCleanupPass.submit_inventory")
    public static let cloudCleanupPassCompleteInventory = BridgeCall("CloudCleanupPass.complete_inventory")
    public static let cloudCleanupPassFinish = BridgeCall("CloudCleanupPass.finish")
    public static let cloudPolicyExecutionBeginZoneDeletionPass = BridgeCall("CloudPolicyExecution.begin_zone_deletion_pass")
    public static let cloudZoneDeletionPassActiveTarget = BridgeCall("CloudZoneDeletionPass.active_target")
    public static let cloudZoneDeletionPassSubmitRead = BridgeCall("CloudZoneDeletionPass.submit_read")
    public static let cloudZoneDeletionPassCompleteRead = BridgeCall("CloudZoneDeletionPass.complete_read")
    public static let cloudZoneDeletionPassSubmitDeletion = BridgeCall("CloudZoneDeletionPass.submit_deletion")
    public static let cloudZoneDeletionPassCompleteDeletion = BridgeCall("CloudZoneDeletionPass.complete_deletion")
    public static let cloudZoneDeletionPassFinish = BridgeCall("CloudZoneDeletionPass.finish")
    public static let cloudAssetReadPassNext = BridgeCall("CloudAssetReadPass.next")
    public static let cloudAssetReadPassStartNext = BridgeCall("CloudAssetReadPass.start_next")
    public static let cloudAssetReadNextResult = BridgeCall("CloudAssetReadNextResult.result")
    public static let cloudAssetReadPassStartSubmit = BridgeCall("CloudAssetReadPass.start_submit")
    public static let cloudAssetReadSubmitResult = BridgeCall("CloudAssetReadSubmitResult.result")
    public static let cloudAssetReadPassStartComplete = BridgeCall("CloudAssetReadPass.start_complete")
    public static let cloudAssetReadCompleteResult = BridgeCall("CloudAssetReadCompleteResult.result")
    public static let cloudAssetReadPassStartCancel = BridgeCall("CloudAssetReadPass.start_cancel")
    public static let cloudAssetReadCancelResult = BridgeCall("CloudAssetReadCancelResult.result")
    public static let cloudAssetReadPassStartFinish = BridgeCall("CloudAssetReadPass.start_finish")
    public static let cloudAssetReadFinishResult = BridgeCall("CloudAssetReadFinishResult.result")
    public static let cloudAssetReadPassStartOperationActivity = BridgeCall("CloudAssetReadPass.start_operation_activity")
    public static let cloudAssetReadActivityResult = BridgeCall("CloudAssetReadActivityResult.result")
    public static let cloudAssetReadPassSubmit = BridgeCall("CloudAssetReadPass.submit")
    public static let cloudAssetReadPassComplete = BridgeCall("CloudAssetReadPass.complete")
    public static let cloudAssetReadPassCancel = BridgeCall("CloudAssetReadPass.cancel")
    public static let cloudAssetReadPassOperationActivity = BridgeCall("CloudAssetReadPass.operation_activity")
    public static let cloudAssetReadPassFinish = BridgeCall("CloudAssetReadPass.finish")
    public static let cloudWorkflowSubmitMediaBatch = BridgeCall("CloudWorkflow.submit_media_batch")
    public static let cloudMediaDownloadsBlocked = BridgeCall("CloudMedia.downloads_blocked")
    public static let cloudPolicyExecutionContinueMediaAfterSend = BridgeCall("CloudPolicyExecution.continue_media_after_send")
    public static let cloudPolicyExecutionBeginMedia = BridgeCall("CloudPolicyExecution.begin_media")
    public static let cloudMediaNextWork = BridgeCall("CloudMedia.next_work")
    public static let cloudMediaStartNextWork = BridgeCall("CloudMedia.start_next_work")
    public static let cloudMediaNextWorkResult = BridgeCall("CloudMediaNextWorkResult.result")
    public static let cloudMediaSubmitRead = BridgeCall("CloudMedia.submit_read")
    public static let cloudMediaQueueUpload = BridgeCall("CloudMedia.queue_upload")
    public static let cloudMediaReply = BridgeCall("CloudMedia.reply")
    public static let cloudConnectionBeginConnectionPreview = BridgeCall("CloudConnection.begin_connection_preview")
    public static let cloudConnectionLocalPublicationInformation = BridgeCall("CloudConnection.local_publication_information")
    public static let localPublicationInformation = BridgeCall("local_publication_information")
    public static let cloudConnectionConnectionActiveRead = BridgeCall("CloudConnection.connection_active_read")
    public static let cloudConnectionConnectionEpochRead = BridgeCall("CloudConnection.connection_epoch_read")
    public static let cloudConnectionKeepLocalOnly = BridgeCall("CloudConnection.keep_local_only")
    public static let cloudConnectionCancelPreview = BridgeCall("CloudConnection.cancel_preview")
    public static let cloudConnectionConfirmPublication = BridgeCall("CloudConnection.confirm_publication")
    public static let cloudConnectionConfirmAdoption = BridgeCall("CloudConnection.confirm_adoption")
    public static let cloudConnectionResumeBinding = BridgeCall("CloudConnection.resume_binding")
    public static let cloudConnectionStartNextPublicationWork = BridgeCall("CloudConnection.start_next_publication_work")
    public static let cloudConnectionStartResumeAdoption = BridgeCall("CloudConnection.start_resume_adoption")
    public static let cloudAdoptionResumeResult = BridgeCall("CloudAdoptionResumeResult.result")
    public static let cloudConnectionStartAdoptionResult = BridgeCall("CloudConnection.start_adoption_result")
    public static let cloudPublicationWorkResult = BridgeCall("CloudPublicationWorkResult.result")
    public static let cloudAdoptionAppliedResult = BridgeCall("CloudAdoptionAppliedResult.result")
    public static let cloudAdoptionFailureDiagnostic = BridgeCall("CloudAdoptionAppliedResult.failure_diagnostic")
    public static let cloudConnectionNextPublicationWork = BridgeCall("CloudConnection.next_publication_work")
    public static let cloudConnectionPerformLocal = BridgeCall("CloudConnection.perform_local")
    public static let cloudConnectionSubmitPublicationRequest = BridgeCall("CloudConnection.submit_publication_request")
    public static let cloudConnectionPublicationResult = BridgeCall("CloudConnection.publication_result")
    public static let cloudConnectionAdoptionResult = BridgeCall("CloudConnection.adoption_result")
    public static let cloudConnectionPerformAdoptionLocal = BridgeCall("CloudConnection.perform_adoption_local")
    public static let cloudConnectionCancelOperation = BridgeCall("CloudConnection.cancel_operation")
    public static let cloudConnectionTrigger = BridgeCall("CloudConnection.trigger")
    public static let cloudConnectionRecoveryStatus = BridgeCall("CloudConnection.recovery_status")

    public static let configureCloudPlatform = BridgeCall("configure_cloud_platform")
    public static let cloudWorkflowNew = BridgeCall("CloudWorkflow.new")
    public static let cloudWorkflowInstance = BridgeCall("CloudWorkflow.instance")
    public static let cloudWorkflowCurrentGuard = BridgeCall("CloudWorkflow.current_guard")
    public static let cloudWorkflowEntityFacts = BridgeCall("CloudWorkflow.entity_facts")
    public static let cloudWorkflowAwaitTracking = BridgeCall("CloudWorkflow.await_tracking")
    public static let cloudWorkflowTrackingSendables = BridgeCall("CloudWorkflow.tracking_sendables")
    public static let cloudWorkflowPendingTransition = BridgeCall("CloudWorkflow.pending_transition")
    public static let cloudWorkflowEmittedUpdateIds = BridgeCall("CloudWorkflow.emitted_update_ids")
    public static let cloudWorkflowObservations = BridgeCall("CloudWorkflow.observations")
    public static let cloudWorkflowPollReview = BridgeCall("CloudWorkflow.poll_review")
    public static let cloudWorkflowSetPaused = BridgeCall("CloudWorkflow.set_paused")
    public static let cloudWorkflowStopSyncing = BridgeCall("CloudWorkflow.stop_syncing")
    public static let cloudWorkflowDurableSnapshot = BridgeCall("CloudWorkflow.durable_snapshot")
    public static let cloudWorkflowFetchDrains = BridgeCall("CloudWorkflow.fetch_drains")
    public static let cloudDiagnosticsExport = BridgeCall("cloud_diagnostics_export")
    public static let cloudWorkflowDiagnostics = BridgeCall("CloudWorkflow.diagnostics")
    public static let cloudWorkflowStatus = BridgeCall("CloudWorkflow.status")
    public static let cloudWorkflowExchangeResult = BridgeCall("CloudWorkflow.exchange_result")
    public static let cloudWorkflowBeginIdentityRead = BridgeCall("CloudWorkflow.begin_identity_read")
    public static let cloudWorkflowCompleteIdentityRead = BridgeCall("CloudWorkflow.complete_identity_read")
    public static let cloudWorkflowPrepareRuntime = BridgeCall("CloudWorkflow.prepare_runtime")
    public static let cloudWorkflowRegisterSdk = BridgeCall("CloudWorkflow.register_sdk")
    public static let cloudWorkflowValidateSdk = BridgeCall("CloudWorkflow.validate_sdk")
    public static let cloudWorkflowRetireSdk = BridgeCall("CloudWorkflow.retire_sdk")
    public static let cloudWorkflowReleaseSdk = BridgeCall("CloudWorkflow.release_sdk")
    public static let cloudWorkflowAccountChanged = BridgeCall("CloudWorkflow.account_changed")
    public static let cloudWorkflowTakePendingUpdates = BridgeCall("CloudWorkflow.take_pending_updates")
    public static let cloudWorkflowUpdateApplied = BridgeCall("CloudWorkflow.update_applied")
    public static let cloudWorkflowBuildBatch = BridgeCall("CloudWorkflow.build_batch")
    public static let cloudWorkflowSent = BridgeCall("CloudWorkflow.sent")
    public static let cloudWorkflowSdkSavedRemoved = BridgeCall("CloudWorkflow.sdk_saved_removed")
    public static let cloudWorkflowHandlerReturned = BridgeCall("CloudWorkflow.handler_returned")
    public static let cloudWorkflowTrackingFailure = BridgeCall("CloudWorkflow.tracking_failure")
    public static let cloudWorkflowStateUpdate = BridgeCall("CloudWorkflow.state_update")
    public static let cloudWorkflowFetchedZoneLosses = BridgeCall("CloudWorkflow.fetched_zone_losses")
    public static let cloudWorkflowKnownLossRead = BridgeCall("CloudWorkflow.known_loss_read")
    public static let cloudWorkflowValidatePreparedBatch = BridgeCall("CloudWorkflow.validate_prepared_batch")
    public static let cloudWorkflowValidateNetworkSdk = BridgeCall("CloudWorkflow.validate_network_sdk")
    public static let cloudWorkflowFetchedCollectionZoneDeletions = BridgeCall("CloudWorkflow.fetched_collection_zone_deletions")
    public static let cloudWorkflowFetched = BridgeCall("CloudWorkflow.fetched")
    public static let cloudWorkflowBeginFetch = BridgeCall("CloudWorkflow.begin_fetch")
    public static let cloudWorkflowWillFetch = BridgeCall("CloudWorkflow.will_fetch")
    public static let cloudWorkflowFetchZone = BridgeCall("CloudWorkflow.fetch_zone")
    public static let cloudWorkflowDidFetch = BridgeCall("CloudWorkflow.did_fetch")
    public static let cloudWorkflowFetchReadPlan = BridgeCall("CloudWorkflow.fetch_read_plan")
    public static let cloudWorkflowSyncNowEpochRead = BridgeCall("CloudWorkflow.sync_now_epoch_read")
    public static let cloudPolicyExecutionSyncNowEpochRead = BridgeCall("CloudPolicyExecution.sync_now_epoch_read")
    public static let cloudWorkflowSyncNowRequest = BridgeCall("CloudWorkflow.sync_now_request")
    public static let cloudSyncNowRequestEpochRead = BridgeCall("CloudSyncNowRequest.epoch_read")
    public static let cloudSyncNowRequestBeginExchange = BridgeCall("CloudSyncNowRequest.begin_exchange")
    public static let cloudFetchReadCancel = BridgeCall("CloudFetchRead.cancel")
    public static let cloudFetchReadValidateEnqueue = BridgeCall("CloudFetchRead.validate_enqueue")
    public static let cloudFetchReadEpochPreparationOutcome = BridgeCall("CloudFetchRead.epoch_preparation_outcome")
    public static let cloudFetchReadTarget = BridgeCall("CloudFetchRead.target")
    public static let cloudFetchReadValidate = BridgeCall("CloudFetchRead.validate")
    public static let cloudFetchReadComplete = BridgeCall("CloudFetchRead.complete")
    public static let cloudWorkflowFinishFetch = BridgeCall("CloudWorkflow.finish_fetch")
    public static let cloudWorkflowAbortFetch = BridgeCall("CloudWorkflow.abort_fetch")
    public static let cloudWorkflowBeginExchange = BridgeCall("CloudWorkflow.begin_exchange")
    public static let cloudWorkflowExchangeFetched = BridgeCall("CloudWorkflow.exchange_fetched")
    public static let cloudWorkflowCompleteExchange = BridgeCall("CloudWorkflow.complete_exchange")
    public static let cloudWorkflowSendDue = BridgeCall("CloudWorkflow.send_due")
    public static let cloudWorkflowBeginSend = BridgeCall("CloudWorkflow.begin_send")
    public static let cloudWorkflowFinishSend = BridgeCall("CloudWorkflow.finish_send")

    /// Every bridge call, in registry order.
    public static let all: [BridgeCall] = [
        migrateProvider, providerOwner, providerSnapshot, providerRefresh, providerSaveSettings,
        providerBeginDiscovery, providerAcceptICloud, providerBeginHandoff, providerFinishHandoff,
        providerRecoverHandoffToLocalOnly, providerRetryFailedPublication,
        providerConfigureMeguri, providerConfiguredMeguri, providerConnectionInformation, providerStartLogin, providerLoginResult,
        providerBootstrapPrepare, providerBootstrapConfirm, providerBootstrapMediaPrepare,
        providerBootstrapMedia, providerBootstrapComplete, providerBootstrapReadiness,
        providerBootstrapPreviewRecord, providerBootstrapMediaConsentRecord, providerBootstrapOperationRecord,
        providerBootstrapOutcome, providerBootstrapMediaSnapshot, providerBootstrapRequestStop,
        providerBootstrapRetainedOperation,
        providerStartCollectionRound, providerStartMediaRound, providerRoundOutcome, providerRoundRetryFinalization,
        providerStartStructureSave, providerStartStructureUpload, providerStructureAdvance, providerStructureCancel,
        providerStructureSaved, providerStructureUploadPending,
        providerBootstrapPendingPreparation, providerBootstrapDiscardPreview, providerBootstrapDiscardOperation, providerBootstrapRecovery,
        testCloudZoneDeletionFixture, testCloudZoneDeletionRows,
        cloudPolicyExecutionBeginCleanupPass, cloudCleanupPassSubmitInventory, cloudCleanupPassCompleteInventory, cloudCleanupPassFinish,
        cloudPolicyExecutionBeginZoneDeletionPass, cloudZoneDeletionPassActiveTarget, cloudZoneDeletionPassSubmitRead, cloudZoneDeletionPassCompleteRead, cloudZoneDeletionPassSubmitDeletion, cloudZoneDeletionPassCompleteDeletion, cloudZoneDeletionPassFinish,
        cloudAssetReadPassStartSubmit, cloudAssetReadSubmitResult, cloudAssetReadPassStartComplete, cloudAssetReadCompleteResult, cloudAssetReadPassStartCancel, cloudAssetReadCancelResult, cloudAssetReadPassStartFinish, cloudAssetReadFinishResult, cloudAssetReadPassStartOperationActivity, cloudAssetReadActivityResult,
        cloudWorkflowBeginFirstAssetReadPass, cloudPolicyExecutionBeginAssetReadPass, cloudAssetReadPassNext, cloudAssetReadPassStartNext, cloudAssetReadNextResult, cloudAssetReadPassSubmit, cloudAssetReadPassComplete, cloudAssetReadPassCancel, cloudAssetReadPassFinish, cloudAssetReadPassOperationActivity,
        cloudWorkflowMedia, cloudWorkflowSubmitMediaBatch, cloudMediaDownloadsBlocked, cloudPolicyExecutionBeginMedia, cloudPolicyExecutionContinueMediaAfterSend, cloudMediaNextWork, cloudMediaStartNextWork, cloudMediaNextWorkResult, cloudMediaSubmitRead, cloudMediaQueueUpload, cloudMediaReply,
        cloudPolicyExecutionStartRegisterSdk, cloudPolicyExecutionStartRetireOriginal, cloudPolicyRegistrationResult, cloudPolicyRetirementResult,
        cloudPolicyExecutionStartPrepareRuntimeAttempt, cloudPolicyPreparationDetailedResult,
        cloudPolicyExecutionStartReportInvocation, cloudPolicyExecutionStartReportStarted, cloudPolicyExecutionStartReportCompletion, cloudPolicyExecutionStartBeginMedia, cloudPolicyMediaResult, cloudPolicyExecutionStartBeginFetch, cloudPolicyFetchResult, cloudPolicyExecutionStartBeginSend, cloudPolicySendResult,
        cloudPolicyExecutionStartValidateControl, cloudPolicyControlResult, cloudPolicyExecutionStartContinueMediaAfterSend, cloudPolicyMediaContinuationResult, cloudPolicyExecutionStartPrepareFlush,
        cloudPolicyExecutionStartBeginAssetReadPass, cloudWorkflowStartBeginFirstAssetReadPass, cloudPolicyAssetReadPassResult,
        cloudPolicyStartInput, cloudPolicyStartRefreshAccount, cloudPolicyStartProjectIdentityRead, cloudPolicyStartStatus, cloudPolicyStartExecuteCore, cloudPolicyStartSubmitNative, cloudPolicyStartCaptureExchange, cloudPolicyStartCompleteExchange, cloudPolicyStartEndBeforeCapture, cloudPolicyStartExchangeReadiness, cloudPolicyStartPrepareExecution, cloudPolicyOutputDetailedResult, cloudPolicyExecutionDetailedResult, cloudPolicyMediaDetailedResult, cloudPolicyOutputResult, cloudPolicyStatusResult, cloudPolicyAdmissionResult, cloudPolicyCaptureResult, cloudPolicyReadinessResult, cloudPolicyExecutionResult,
        cloudWorkflowPolicy, cloudPolicyValidateOriginalOwner, cloudPolicyInput, cloudPolicyRefreshAccount, cloudPolicyProjectIdentityRead, cloudPolicyStatus, cloudPolicyExecuteCore, cloudPolicySubmitNative, cloudPolicyCaptureExchange, cloudPolicyCompleteExchange, cloudPolicyEndBeforeCapture,
        cloudPolicyExecutionValidateControl, cloudPolicyExecutionPrepareFlush, cloudPolicyExchangeReadiness, cloudPolicyPrepareExecution, cloudPolicyExecutionOriginalSdk, cloudPolicyExecutionValidateExecution, cloudPolicyExecutionRegisterSdk, cloudPolicyExecutionRetireOriginal, cloudPolicyExecutionBeginFetch, cloudPolicyExecutionBeginSend, cloudPolicyExecutionOperationActivity,
        cloudConnectionStartNextPublicationWork, cloudConnectionStartResumeAdoption, cloudAdoptionResumeResult, cloudConnectionStartAdoptionResult, cloudPublicationWorkResult, cloudAdoptionAppliedResult, cloudAdoptionFailureDiagnostic,
        cloudWorkflowConnection, cloudConnectionBeginConnectionPreview, cloudConnectionLocalPublicationInformation, localPublicationInformation, cloudConnectionConnectionActiveRead, cloudConnectionConnectionEpochRead, cloudConnectionCancelPreview, cloudConnectionKeepLocalOnly, cloudConnectionConfirmPublication, cloudConnectionConfirmAdoption, cloudConnectionResumeBinding, cloudConnectionNextPublicationWork, cloudConnectionPerformLocal, cloudConnectionSubmitPublicationRequest, cloudConnectionPublicationResult, cloudConnectionAdoptionResult, cloudConnectionPerformAdoptionLocal, cloudConnectionCancelOperation, cloudConnectionTrigger, cloudConnectionRecoveryStatus,
        configureCloudPlatform, cloudWorkflowNew, cloudWorkflowInstance, cloudWorkflowCurrentGuard, cloudWorkflowEntityFacts, cloudWorkflowAwaitTracking, cloudWorkflowTrackingSendables, cloudWorkflowPendingTransition, cloudWorkflowEmittedUpdateIds, cloudWorkflowObservations, cloudWorkflowPollReview, cloudWorkflowSetPaused, cloudWorkflowStopSyncing, cloudWorkflowDurableSnapshot, cloudWorkflowFetchDrains, cloudDiagnosticsExport, cloudWorkflowDiagnostics, cloudWorkflowStatus, cloudWorkflowExchangeResult, cloudWorkflowBeginIdentityRead, cloudWorkflowCompleteIdentityRead, cloudWorkflowPrepareRuntime, cloudWorkflowRegisterSdk, cloudWorkflowValidateSdk, cloudWorkflowRetireSdk, cloudWorkflowReleaseSdk, cloudWorkflowAccountChanged, cloudWorkflowTakePendingUpdates, cloudWorkflowUpdateApplied, cloudWorkflowBuildBatch, cloudWorkflowSent, cloudWorkflowHandlerReturned, cloudWorkflowSdkSavedRemoved, cloudWorkflowTrackingFailure, cloudWorkflowStateUpdate, cloudWorkflowFetchedZoneLosses, cloudWorkflowKnownLossRead, cloudWorkflowValidateNetworkSdk, cloudWorkflowValidatePreparedBatch, cloudWorkflowFetchedCollectionZoneDeletions, cloudWorkflowFetched, cloudWorkflowBeginFetch, cloudWorkflowWillFetch, cloudWorkflowFetchZone, cloudWorkflowDidFetch, cloudWorkflowFetchReadPlan, cloudWorkflowSyncNowEpochRead, cloudPolicyExecutionSyncNowEpochRead, cloudWorkflowSyncNowRequest, cloudSyncNowRequestEpochRead, cloudSyncNowRequestBeginExchange, cloudFetchReadCancel, cloudFetchReadValidateEnqueue, cloudFetchReadEpochPreparationOutcome, cloudFetchReadTarget, cloudFetchReadValidate, cloudFetchReadComplete, cloudWorkflowFinishFetch, cloudWorkflowAbortFetch, cloudWorkflowBeginExchange, cloudWorkflowExchangeFetched, cloudWorkflowCompleteExchange, cloudWorkflowSendDue, cloudWorkflowBeginSend, cloudWorkflowFinishSend,
        ManualMediaCalls.preview, ManualMediaCalls.run, ManualMediaCalls.cancelPrepared, ManualMediaCalls.forgetOwner, ManualMediaCalls.readPreview, ManualMediaCalls.readProgress, ManualMediaCalls.stop,
        normalizeManualEndpoint, manualTransferSessionNew, manualTransferSessionStartSetup, manualTransferSessionStartLogin, manualTransferSessionStartHistory, manualTransferSessionStartPrepare, manualTransferSessionStartConfirm, manualTransferSessionStartRecoveryPreview, manualTransferSessionStartCancelHistory, manualTransferSessionStartCancelPreview, manualTransferSessionStartSaveCopy, manualTransferResultOutcome, manualTransferResultOperationID, manualTransferPreviewRecord, manualTransferHistoryEntryRecord,
        startCreateBackup, createBackupResultOutcome, startRestoreBackup, restoreBackupResultOutcome, startRunDueMaintenance, dueMaintenanceResultOutcome, listBackups, backupLimits, setBackupLimits, maintenanceStatus,
        startCollectionExport, collectionExportResultOutcome, collectionExportArtifactMetadata, collectionExportArtifactAcquire, collectionExportArtifactDiscard, collectionExportLeasePath, collectionExportLeaseFinish, startCollectionExportSave, collectionExportSaveResultOutcome,
        startCollectionInspection, collectionInspectResultOutcome, collectionInspectionFacts, collectionInspectionDiscard, startCollectionActivation, collectionActivationResultOutcome, jobProgressSetListener,
        cloudOriginalRoleReadOriginalAck, cloudOriginalRoleReadOriginalEdit, testCloudOriginalControlWitness, testCloudRestoredControl, testCloudOriginalExchangeWitness, testCloudOriginalExchangeCapture, testCloudOriginalExchangeWork, testCloudReadOriginalExchange, testCloudCrosscheckOriginalAckSet, testCloudSelectedControl, testCloudSelectedPublicationEvent, testCloudOriginalRole, cloudOriginalRoleFacts, cloudOriginalRoleValidateOwned, cloudOriginalRoleObserve, cloudOriginalRoleStartFixedEdit, cloudOriginalEditResult, testSmokeIntegrity, testCloudSmokeObservation, testCloudSmokeBatch, testCloudSmokeAcknowledgements, testCloudSmokeReview, testCloudStartNativePolicyOpen, testCloudRequireNativePolicyBaseline, testCloudPolicyExchangeObservation, startOpen, startCreateEmpty, prepareCloudConnectionHost, closeCollection, collectionLifecycle, waitCollectionAvailable, deckTreeWithCounts, allDeckNames, matchDeckName, deckStudyState, deckDescriptionHtml, activityHeatmap, studiedTodayNumbers, startStatsPage, startReleaseStatsCaches, statsHomeIdle, statsDayCards,
        startSession, reviewSessionJobResult, peekFirstCard, sessionHead, refreshSessionCard, renderSessionHead, answerSession, endSession, peekReviewUndo, flushReviewForBackground,
        coreVersion, coreBuildInfo, setEventListener, cancelTokenNew, cancelTokenCancel, cancelTokenIsCancelled, renderCard, compareTypedAnswer, cardExcerpts,
        deckDeletionJobResult, addDeck, startRenameDeck, startMoveDecks, startCollapseDecks,
        selectDeck, deckDeletionSummary, startDeleteDecks, noteDeletionJobResult, addNote,
        addNoteForScreen, updateNote, patchNote, cardDeck, readNote, checkNoteFields, addDefaults, addNotetypeSelection, notetypeLastDeck, tagComplete, editorFieldHTML, startDeleteNotes, notetypeNames,
        readNotetype, addNotetype, addStockNotetype, cloneNotetype, ensureImageOcclusionNotetype,
        renameNotetype, setNotetypeCss, setNotetypeFieldOptions, setNotetypeTemplateOptions, notetypeCardText, notetypeCardTextEdit, tagMutationJobResult,
        tagTree, flagNames, setTagsCollapsed, renameFlag, restoreFlagName,
        addNoteTags, startAddNoteTags, removeNoteTags, startRemoveNoteTags, setCardFlag,
        startSetCardFlag, toggleCardFlag, startToggleCardFlag, markNotes, startMarkNotes,
        unmarkNotes, startUnmarkNotes, toggleNoteMark, startToggleNoteMark, startRenameTag,
        startReparentTags, startDeleteTags, startClearUnusedTags, addMedia, trackedMediaNames, flushForBackground,
        suspend, resume, checkRollover, fsrsPolicyStatus, enableFsrs, undoJobResult, peekUndo, peekRedo,
        tryPendingReviewRestore, deckRenameJobResult, deckMoveJobResult, deckCollapseJobResult, startUndo, startRedo, startAddNotes, noteAdditionJobResult,
        parseDueDays, startSchedOp, schedOpJobResult, startBuriedKinds, buriedKindsJobResult,
        deckOptionsEditorSnapshot, deckOptionsEditorEdit, deckOptionsEditorValidate, parseOptionSteps, formatOptionSteps, startDeckOptions, startSaveDeckOptions, startOptionIgnoreCount, startSchedulingPreferences, startSaveSchedulingPreferences, deckOptionsReadJobResult, deckOptionsSaveJobResult, optionIgnoreCountJobResult, schedulingPreferencesJobResult, schedulingPreferencesSaveJobResult, validateSchedulingPreferences,
        notetypeList, startNotetypeNoteCounts, notetypeNoteCountsJobResult, startNotetypeFieldContentCounts, notetypeFieldContentCountsJobResult, startNotetypeDraftSummary, notetypeDraftSummaryJobResult, startNotetypeNotesWithoutCard, notetypeNotesWithoutCardJobResult, startNotetypeRemovalCounts, notetypeRemovalCountsJobResult, notetypeNearFieldName,
        startNotetypeStructureSave, notetypeStructureJobResult, notetypeCheckFieldName, templateFieldReferences,
        startNotetypeTemplateCardCounts, notetypeTemplateCardCountsJobResult, notetypeTemplateIssues, notetypeReplaceFieldReferences,
        notetypeTemplateSyntax, notetypeStylingSyntax,
        openSearchSession, browseStartSearch, browseResort, browseToggle, browseReverse, browseInvalidatedRows, browseRows, browseQaCells, browseIndexOf, browseMapSelection, browseResolveSelection, browseSelection, browseHandleState, browseClose, browseSelectionResolve,
        filterRunText, filterReadText, filterCommit, filterTextIncomplete, filterDeckValue, filterSuggestions, startFilterCounts, filterCountsJobResult, browserColumnSet, changeBrowserColumnSet, useStandardBrowserColumns, setBrowserNotesMode, saveBrowserSearch, startBrowseSelectionCounts, browseSelectionCountsJobResult, browseOpening,
        startFindDuplicates, findDuplicatesJobResult,
        findReplacePreviewsNew, findReplacePreviewsStart, findReplacePreviewJobResult, startFindReplace, bulkActionJobResult,
    ]
}
