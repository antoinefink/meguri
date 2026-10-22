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
        reviewStartToFirstCard, reviewPresentToNextSettled, reviewOnDemandRender, reviewAnswerMainThread,
        reviewUndoToCardVisible, reviewActionToNextVisible, reviewTouchDownToTint, searchFirstPage,
        statsAllGraphs, statsScopeChange, heatmapRender, cardInfoOpen,
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
    public let interval: IntervalName

    init(_ name: StaticString) {
        interval = IntervalName(name, .bridge)
    }

    public var name: String { interval.name }
    public var description: String { interval.name }

    public static let startOpen = BridgeCall("start_open")
    public static let closeCollection = BridgeCall("close_collection")
    public static let collectionLifecycle = BridgeCall("collection_lifecycle")
    public static let waitCollectionAvailable = BridgeCall("wait_collection_available")
    public static let deckTreeWithCounts = BridgeCall("deck_tree_with_counts")
    public static let deckStudyState = BridgeCall("deck_study_state")
    public static let deckDescriptionHtml = BridgeCall("deck_description_html")

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
    public static let updateNote = BridgeCall("update_note")
    public static let readNote = BridgeCall("read_note")
    public static let checkNoteFields = BridgeCall("check_note_fields")
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

    /// Every bridge call, in registry order.
    public static let all: [BridgeCall] = [
        startOpen, closeCollection, collectionLifecycle, waitCollectionAvailable, deckTreeWithCounts, deckStudyState, deckDescriptionHtml,
        startSession, reviewSessionJobResult, peekFirstCard, sessionHead, refreshSessionCard, renderSessionHead, answerSession, endSession, peekReviewUndo, flushReviewForBackground,
        coreVersion, coreBuildInfo, setEventListener, cancelTokenNew, cancelTokenCancel, cancelTokenIsCancelled, renderCard, compareTypedAnswer, cardExcerpts,
        deckDeletionJobResult, addDeck, startRenameDeck, startMoveDecks, startCollapseDecks,
        selectDeck, deckDeletionSummary, startDeleteDecks, noteDeletionJobResult, addNote,
        updateNote, readNote, checkNoteFields, startDeleteNotes, notetypeNames,
        readNotetype, addNotetype, addStockNotetype, cloneNotetype, ensureImageOcclusionNotetype,
        renameNotetype, setNotetypeCss, setNotetypeFieldOptions, setNotetypeTemplateOptions, tagMutationJobResult,
        tagTree, flagNames, setTagsCollapsed, renameFlag, restoreFlagName,
        addNoteTags, startAddNoteTags, removeNoteTags, startRemoveNoteTags, setCardFlag,
        startSetCardFlag, toggleCardFlag, startToggleCardFlag, markNotes, startMarkNotes,
        unmarkNotes, startUnmarkNotes, toggleNoteMark, startToggleNoteMark, startRenameTag,
        startReparentTags, startDeleteTags, startClearUnusedTags, addMedia, trackedMediaNames, flushForBackground,
        suspend, resume, checkRollover, fsrsPolicyStatus, enableFsrs, undoJobResult, peekUndo, peekRedo,
        tryPendingReviewRestore, deckRenameJobResult, deckMoveJobResult, deckCollapseJobResult, startUndo, startRedo, startAddNotes, noteAdditionJobResult,
        parseDueDays, startSchedOp, schedOpJobResult, startBuriedKinds, buriedKindsJobResult,
        deckOptionsEditorSnapshot, deckOptionsEditorEdit, deckOptionsEditorValidate, parseOptionSteps, formatOptionSteps, startDeckOptions, startSaveDeckOptions, startOptionIgnoreCount, startSchedulingPreferences, startSaveSchedulingPreferences, deckOptionsReadJobResult, deckOptionsSaveJobResult, optionIgnoreCountJobResult, schedulingPreferencesJobResult, schedulingPreferencesSaveJobResult, validateSchedulingPreferences,
    ]
}
