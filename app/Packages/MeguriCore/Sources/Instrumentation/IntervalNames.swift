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

    /// Every interval, in registry order.
    public static let all: [IntervalName] = [
        answerToNextVisible, revealToAnswerVisible, launchToDeckListInteractive, keystrokeToResultsVisible,
        browserOpenToFirstPage, statsFirstGraphs, editorOpenToEditable, syncStartToEnd,
        reviewStartToFirstCard, reviewPresentToNextSettled, reviewOnDemandRender, reviewAnswerMainThread,
        reviewUndoToCardVisible, reviewActionToNextVisible, reviewTouchDownToTint, searchFirstPage,
        statsAllGraphs, statsScopeChange, heatmapRender, cardInfoOpen,
        probeSyntheticDelay,
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

    public static let coreVersion = BridgeCall("core_version")
    public static let coreBuildInfo = BridgeCall("core_build_info")
    public static let setEventListener = BridgeCall("set_event_listener")
    public static let cancelTokenNew = BridgeCall("CancelToken.new")
    public static let cancelTokenCancel = BridgeCall("CancelToken.cancel")
    public static let cancelTokenIsCancelled = BridgeCall("CancelToken.isCancelled")

    /// Every bridge call, in registry order.
    public static let all: [BridgeCall] = [
        coreVersion, coreBuildInfo, setEventListener, cancelTokenNew, cancelTokenCancel, cancelTokenIsCancelled,
    ]
}
