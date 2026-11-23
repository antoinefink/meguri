#if MEGURI_TEST_HOOKS
import AppModel
import CoreBridge
import DesignSystem
import Home
import Instrumentation
import QuartzCore
import Stats
import SwiftUI
#if os(iOS)
import notify
#else
import AppKit
#endif

extension TestHooks {
    /// The Statistics page's routes: the chart system's gallery (U295) and the page in the
    /// shell (U296), both on SAMPLE data (`StatsSampleService`: a made-up collection of about
    /// three years, answered tier by tier). No collection is opened and nothing is written. A
    /// route turns `StatsRollout` on; the real collection's page is reached with
    /// `-MeguriFixture` and `-MeguriStatsRollout open`.
    ///
    /// The page (`stats/page/<state>`, All Decks and All unless named):
    /// - `typical`: every box filled; `contrast`: with Increase Contrast; `ax5`: the
    ///   largest text size.
    /// - `first-run`: no collection on this device (N18.3): Today's welcome and no
    ///   Statistics row; the route asks for Statistics and the shell keeps Today.
    /// - `no-collection`: the section while no collection is open (S12.1).
    /// - `no-cards` (S12.2), `no-reviews` (S12.3).
    /// - `cold`: nothing kept and no tier back yet: every box with its progress (S10.3);
    ///   `loading`: the present tier back, the review log and knowledge still reading;
    ///   `loading-knowledge`: only Knowledge Over Time still reading, its progress determinate.
    /// - `updating`: a kept result shown while a refresh reads (S10.2, "Updating…").
    /// - `slow`: two seconds between tiers (the loading-phase UI test).
    /// - `pinned`: 1M with a pinned selection and its callout in each Knowledge box (S8.3),
    ///   Retention's on its open point.
    /// - `deleted-deck` (S12.7), `unavailable` (S12.8), `failed-box` (S12.8: Retention and
    ///   Forgotten's part fails alone).
    /// - `stats/period/<period>` (`1m`, `3m`, `6m`, `1y`): the page at that period.
    /// - `stats/deck/<deck>` (`languages`, `japanese`, `review-ahead`: a filtered deck, S12.6):
    ///   the page on that deck.
    /// - `stats/chart/<box>`: the page scrolled to the box's section (`knowledge-over-time`,
    ///   `retention`, …, `by-deck`), for the section units' captures.
    ///
    /// The gallery:
    /// - `stats/charts/<page>`: the gallery's page (`bars`, `lines`, `areas`:
    ///   every mark kind; `states`: loading, empty and series drawn apart; `edges`: a
    ///   failed part, a sparse chart and empty lines; `selection`: pinned
    ///   selections with their callouts), All's monthly bins.
    /// - `…/<period>` (`1m`, `3m`, `1y`): the same with that period's bins.
    /// - `…/contrast`: Increase Contrast; `…/ax5`: the largest text size. The
    ///   appearance is the device's (`-MeguriAppearance` on the Mac).
    enum StatsRoutes {
        // Literal paths: `tools/mg xcode-check` reads the routes from their registrations.
        static func register(in registry: inout RouteRegistry) {
            registry.registerShell("stats/page/typical") { _ in page(.typical) }
            registry.registerShell("stats/page/contrast") { _ in page(.typical, option: .contrast) }
            registry.registerShell("stats/page/ax5") { _ in page(.typical, option: .ax5) }
            registry.registerShell("stats/page/first-run") { _ in firstRun() }
            registry.registerShell("stats/page/no-collection") { _ in page(.noCollection) }
            registry.registerShell("stats/page/no-cards") { _ in page(.shape(.noCards)) }
            registry.registerShell("stats/page/no-reviews") { _ in page(.shape(.noReviews)) }
            registry.registerShell("stats/page/cold") { _ in page(.ending(.stall(.present))) }
            registry.registerShell("stats/page/loading") { _ in page(.ending(.stall(.reviewLog))) }
            registry.registerShell("stats/page/loading-knowledge") { _ in page(.ending(.stall(.knowledge))) }
            registry.registerShell("stats/page/updating") { _ in page(.updating) }
            registry.registerShell("stats/page/deleted-deck") { _ in page(.typical, deck: 1_700_000_000_999) }
            registry.registerShell("stats/page/unavailable") { _ in page(.ending(.unreadable)) }
            registry.registerShell("stats/page/failed-box") { _ in page(.ending(.partFails(.retentionByDay))) }
            registry.registerShell("stats/page/slow") { _ in page(.slow) }
            registry.registerShell("stats/page/pinned") { _ in page(.typical, period: .oneMonth, pins: true) }
            registry.registerShell("stats/period/1m") { _ in page(.typical, period: .oneMonth) }
            registry.registerShell("stats/period/3m") { _ in page(.typical, period: .threeMonths) }
            registry.registerShell("stats/period/6m") { _ in page(.typical, period: .sixMonths) }
            registry.registerShell("stats/period/1y") { _ in page(.typical, period: .oneYear) }
            registry.registerShell("stats/deck/languages") { _ in page(.typical, deck: StatsSampleService.decks[0].id) }
            registry.registerShell("stats/deck/japanese") { _ in page(.typical, deck: StatsSampleService.decks[0].children[0].id) }
            registry.registerShell("stats/deck/review-ahead") { _ in page(.typical, deck: StatsSampleService.decks[3].id) }
            registry.registerShell("stats/chart/knowledge-over-time") { _ in page(.typical, box: .knowledgeOverTime) }
            registry.registerShell("stats/chart/retention") { _ in page(.typical, box: .retention) }
            registry.registerShell("stats/chart/forgotten") { _ in page(.typical, box: .forgotten) }
            registry.registerShell("stats/chart/reviews-over-time") { _ in page(.typical, box: .reviewsOverTime) }
            registry.registerShell("stats/chart/activity") { _ in page(.typical, box: .activity) }
            registry.registerShell("stats/chart/cards-over-time") { _ in page(.typical, box: .cardsOverTime) }
            registry.registerShell("stats/chart/card-counts") { _ in page(.typical, box: .cardCounts) }
            registry.registerShell("stats/chart/added-and-started") { _ in page(.typical, box: .addedAndStarted) }
            registry.registerShell("stats/chart/reviews-due") { _ in page(.typical, box: .reviewsDue) }
            registry.registerShell("stats/chart/answers") { _ in page(.typical, box: .answers) }
            registry.registerShell("stats/chart/time-of-day") { _ in page(.typical, box: .timeOfDay) }
            registry.registerShell("stats/chart/retrievability") { _ in page(.typical, box: .retrievability) }
            registry.registerShell("stats/chart/stability") { _ in page(.typical, box: .stability) }
            registry.registerShell("stats/chart/difficulty") { _ in page(.typical, box: .difficulty) }
            registry.registerShell("stats/chart/by-deck") { _ in page(.typical, box: .byDeck) }
            registry.register("stats/charts/bars") { _ in .screen { gallery(.bars, option: "") } }
            registry.register("stats/charts/bars/1m") { _ in .screen { gallery(.bars, option: "1m") } }
            registry.register("stats/charts/bars/3m") { _ in .screen { gallery(.bars, option: "3m") } }
            registry.register("stats/charts/bars/1y") { _ in .screen { gallery(.bars, option: "1y") } }
            registry.register("stats/charts/bars/contrast") { _ in .screen { gallery(.bars, option: "contrast") } }
            registry.register("stats/charts/bars/ax5") { _ in .screen { gallery(.bars, option: "ax5") } }
            registry.register("stats/charts/lines") { _ in .screen { gallery(.lines, option: "") } }
            registry.register("stats/charts/lines/1y") { _ in .screen { gallery(.lines, option: "1y") } }
            registry.register("stats/charts/lines/contrast") { _ in .screen { gallery(.lines, option: "contrast") } }
            registry.register("stats/charts/lines/ax5") { _ in .screen { gallery(.lines, option: "ax5") } }
            registry.register("stats/charts/areas") { _ in .screen { gallery(.areas, option: "") } }
            registry.register("stats/charts/areas/1m") { _ in .screen { gallery(.areas, option: "1m") } }
            registry.register("stats/charts/areas/contrast") { _ in .screen { gallery(.areas, option: "contrast") } }
            registry.register("stats/charts/areas/ax5") { _ in .screen { gallery(.areas, option: "ax5") } }
            registry.register("stats/charts/states") { _ in .screen { gallery(.states, option: "") } }
            registry.register("stats/charts/states/contrast") { _ in .screen { gallery(.states, option: "contrast") } }
            registry.register("stats/charts/states/ax5") { _ in .screen { gallery(.states, option: "ax5") } }
            registry.register("stats/charts/edges") { _ in .screen { gallery(.edges, option: "") } }
            registry.register("stats/charts/edges/contrast") { _ in .screen { gallery(.edges, option: "contrast") } }
            registry.register("stats/charts/selection") { _ in .screen { gallery(.selection, option: "") } }
            registry.register("stats/charts/selection/1m") { _ in .screen { gallery(.selection, option: "1m") } }
            registry.register("stats/charts/selection/contrast") { _ in .screen { gallery(.selection, option: "contrast") } }
            registry.register("stats/charts/selection/ax5") { _ in .screen { gallery(.selection, option: "ax5") } }
            registerReviews(in: &registry)
            registerSections(in: &registry)
        }

        enum Sample {
            case typical
            case shape(StatsSampleService.Shape)
            case ending(StatsSampleService.Ending)
            case updating
            /// Two seconds between tiers, so a loading phase lasts long enough for a UI test to see.
            case slow
            /// No collection open: the page never hears of one.
            case noCollection
        }

        enum Option { case none, contrast, ax5 }

        /// The shell with Statistics selected over the sample collection.
        @MainActor static func page(_ sample: Sample, period: StatsPagePeriod = .all, deck: Int64? = nil,
                                    box: StatsBoxID? = nil, option: Option = .none, pins: Bool = false) -> ShellLaunch {
            StatsRollout.enabled = true
            let service: StatsSampleService = switch sample {
            case .typical, .noCollection: StatsSampleService()
            case let .shape(shape): StatsSampleService(shape: shape)
            case let .ending(ending): StatsSampleService(ending: ending)
            case .updating: StatsSampleService(ending: .laterStall)
            case .slow: StatsSampleService(tierDelay: .seconds(2))
            }
            let model = StatsPageModel(service: service, kept: StatsKeptResults(directory: nil),
                                       store: StatsSelectionStore(defaults: nil),
                                       selection: StatsPageSelection(deck: deck, period: period))
            if case .noCollection = sample {} else {
                model.collectionOpened(identity: "sample")
                model.decksChanged(StatsSampleService.decks)
            }
            let statsPage = StatsPage(model: model)
            StatsHooks.shared.attach(statsPage, sample: true)
            var features = shell(statsPage, option: option)
            features.sectionShown = { _ in true }
            return ShellLaunch(features: features, probe: false) { navigation in
                navigation.select(.section(.stats))
                Task { @MainActor in
                    if let box {
                        // Once the boxes are drawn, the box's section at the top.
                        try? await Task.sleep(for: .milliseconds(600))
                        statsPage.go(to: StatsSectionID.allCases.first { $0.boxes.contains(box) } ?? .knowledge)
                    }
                    if pins {
                        // Once every box is filled, a pinned bar in each Knowledge box: Retention's
                        // on its last open point, the others a few days back.
                        for _ in 0..<100 where !statsPage.filled { try? await Task.sleep(for: .milliseconds(100)) }
                        for id in [StatsBoxID.knowledgeOverTime, .retention, .forgotten] {
                            let data = model.box(id).data
                            let open = data.series.first?.openPoints.last
                            let bin = (id == .retention ? open : nil) ?? max(0, data.bins.count - (id == .forgotten ? 4 : 8))
                            model.select(StatsChartSelection(bin: bin, pinned: true), in: id)
                        }
                    }
                    if case .updating = sample {
                        // Once the first refresh is complete and kept, a change makes the page read
                        // again over it.
                        for _ in 0..<100 where !statsPage.filled { try? await Task.sleep(for: .milliseconds(100)) }
                        model.receive(.graphsStale)
                    }
                }
            }
        }

        /// No collection on this device: only Today (and, on iPad, Settings) is offered.
        @MainActor static func firstRun() -> ShellLaunch {
            StatsRollout.enabled = true
            let model = StatsPageModel(service: StatsSampleService(), kept: StatsKeptResults(directory: nil),
                                       store: StatsSelectionStore(defaults: nil))
            var features = shell(StatsPage(model: model), option: .none)
            features.sectionShown = { ShellFeatures.sectionShown($0, hasCollection: false) }
            let placeholder = features.sectionRoot
            let welcome = HomeDeckListPreview.previewFixtures.first { $0.state == .empty }
            features.sectionRoot = { section, navigation in
                guard section == .home, let welcome else { return placeholder(section, navigation) }
                return AnyView(welcome.makeView().navigationTitle(Text(verbatim: "Today")))
            }
            features.sidebarDecks = { _ in
                AnyView(Text(verbatim: "No decks").foregroundStyle(.secondary).accessibilityIdentifier("home.sidebar.guidance"))
            }
            return ShellLaunch(features: features, probe: false) { navigation in
                navigation.select(.section(.stats))
            }
        }

        /// The shell's features with the page in the Statistics section, second under Today
        /// (N25.1), and the sample decks in the sidebar.
        @MainActor static func shell(_ statsPage: StatsPage, option: Option) -> ShellFeatures {
            var features = ShellSample.features(.typical)
            features.todayRoot = true
            #if os(macOS)
            features.nativeMacWorkspace = true
            features.availableSections = [.home, .stats, .browse, .noteTypes]
            #else
            features.availableSections = [.home, .stats, .noteTypes, .settings]
            #endif
            features.statsPage = statsPage
            features.sidebarDecks = { _ in AnyView(HomeSidebarPreview.decks(StatsSampleService.decks, selection: nil)) }
            let placeholder = features.sectionRoot
            features.sectionRoot = { section, navigation in
                guard section == .stats else { return placeholder(section, navigation) }
                let view = StatisticsView(page: statsPage, navigation: navigation)
                return switch option {
                case .none: AnyView(view)
                case .contrast: AnyView(view.environment(\._colorSchemeContrast, .increased))
                case .ax5: AnyView(view.dynamicTypeSize(.accessibility5))
                }
            }
            return features
        }

        @MainActor @ViewBuilder
        static func gallery(_ page: StatsChartGallery.Page, option: String) -> some View {
            let period = StatsSamplePeriod(rawValue: option) ?? .all
            let gallery = NavigationStack { StatsChartGallery(page: page, period: period) }
            switch option {
            case "contrast": gallery.environment(\._colorSchemeContrast, .increased)
            case "ax5": gallery.dynamicTypeSize(.accessibility5)
            default: gallery
            }
        }
    }

    /// The main thread's frame pacing while the hooks scroll the page (R2078): the display link's
    /// callbacks, each late by however much its interval exceeds the display's frame. XCTest's
    /// hitch metric reports nothing in the test VM (0 on its Mac, no value on its simulators), so
    /// the scroll tests read this instead: `ratio` is the late time per second of the window, in
    /// ms/s, as Apple's hitch time ratio counts it.
    @MainActor final class StatsFramePacing: NSObject {
        struct Result: CustomStringConvertible {
            var ratio: Double
            var frames: Int
            var late: Int
            var worst: Double
            var seconds: Double
            var description: String {
                String(format: "ratio=%.2f;frames=%d;late=%d;worstMs=%.1f;seconds=%.2f", ratio, frames, late, worst, seconds)
            }
        }

        private var link: AnyObject?
        private var previous: CFTimeInterval?
        private var started: CFTimeInterval = 0
        private var lateTime: CFTimeInterval = 0
        private var frames = 0
        private var late = 0
        private var worst: CFTimeInterval = 0

        func start() {
            started = CACurrentMediaTime()
            #if os(iOS)
            let link = CADisplayLink(target: self, selector: #selector(tick(_:)))
            link.add(to: .main, forMode: .common)
            self.link = link
            #else
            guard let view = NSApplication.shared.windows.first(where: { $0.isVisible })?.contentView else { return }
            let link = view.displayLink(target: self, selector: #selector(tick(_:)))
            link.add(to: .main, forMode: .common)
            self.link = link
            #endif
        }

        @objc private func tick(_ link: CADisplayLink) {
            let now = link.timestamp
            let frame = link.targetTimestamp - link.timestamp
            if previous == nil, frame > 0 {
                // Main-thread work between start() and the first callback delays it: beyond one
                // frame and the same half-frame tolerance, that wait is late time too.
                let over = CACurrentMediaTime() - started - frame
                if over > frame / 2 {
                    lateTime += over
                    late += 1
                }
            }
            if let previous, frame > 0 {
                let interval = now - previous
                let over = interval - frame
                // Half a frame of jitter is not a late frame.
                if over > frame / 2 {
                    lateTime += over
                    late += 1
                }
                worst = max(worst, interval)
            }
            previous = now
            frames += 1
        }

        func stop() -> Result {
            (link as? CADisplayLink)?.invalidate()
            link = nil
            let seconds = max(CACurrentMediaTime() - started, 0.001)
            return Result(ratio: lateTime * 1_000 / seconds, frames: frames, late: late, worst: worst * 1_000, seconds: seconds)
        }
    }

    /// The page's platform scroll view (the tallest in the key window), scrolled down and back up
    /// in animated steps of about two thirds of its height, about three seconds in all.
    @MainActor struct StatsNativeScroll {
        #if os(macOS)
        private let view: NSScrollView
        #else
        private let view: UIScrollView
        #endif

        init?() {
            #if os(macOS)
            func all(_ view: NSView) -> [NSScrollView] {
                ((view as? NSScrollView).map { [$0] } ?? []) + view.subviews.flatMap(all)
            }
            guard let root = NSApplication.shared.windows.first(where: { $0.isVisible && $0.canBecomeMain })?.contentView,
                  let tallest = all(root).max(by: { ($0.documentView?.frame.height ?? 0) < ($1.documentView?.frame.height ?? 0) })
            else { return nil }
            #else
            func all(_ view: UIView) -> [UIScrollView] {
                ((view as? UIScrollView).map { [$0] } ?? []) + view.subviews.flatMap(all)
            }
            let windows = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.flatMap(\.windows)
            guard let window = windows.first(where: \.isKeyWindow) ?? windows.first,
                  let tallest = all(window).max(by: { $0.contentSize.height < $1.contentSize.height })
            else { return nil }
            #endif
            view = tallest
        }

        func run() async {
            #if os(macOS)
            let viewport = view.contentView.bounds.height
            let total = (view.documentView?.frame.height ?? 0) - viewport
            #else
            let viewport = view.bounds.height
            let total = view.contentSize.height + view.adjustedContentInset.bottom - viewport
            #endif
            guard total > 0 else { return }
            let step = viewport * 2 / 3
            let downs = Array(stride(from: step, through: total, by: step)) + [total]
            for offset in downs + downs.reversed().dropFirst() + [0] {
                move(to: offset)
                try? await Task.sleep(for: .milliseconds(220))
            }
        }

        private func move(to offset: CGFloat) {
            #if os(macOS)
            NSAnimationContext.runAnimationGroup { context in
                context.duration = 0.2
                view.contentView.animator().setBoundsOrigin(NSPoint(x: 0, y: offset))
            }
            view.reflectScrolledClipView(view.contentView)
            #else
            view.setContentOffset(CGPoint(x: 0, y: offset - view.adjustedContentInset.top), animated: true)
            #endif
        }
    }

    /// What the hooks' commands do to the Statistics page (`ShellCommands`: `statsReload`,
    /// `statsPeriodAll`, `statsPeriodYear`, `statsPeriodHalfYear`, `statsScroll`), for the
    /// performance tests (`StatsPerfTests`) that drive it with no synthesized input; it answers
    /// `<bundle id>.testhooks.statsFilled` each time every box shows its value, and
    /// `.statsScrolled` after a scroll.
    @MainActor final class StatsHooks {
        static let shared = StatsHooks()
        private(set) weak var page: StatsPage?
        /// The page is a route's, over the sample collection.
        private var sample = false
        #if os(iOS)
        /// Holds `statsScrolled`'s state (the last scroll's hitch time ratio) for the test to read.
        private var stateToken: Int32?
        #endif

        /// The page of a route, or the app's (from `MeguriApp`, behind the flag).
        func attach(_ page: StatsPage?, sample: Bool) {
            self.page = page
            self.sample = sample
            page?.filledChanged = { filled in
                if filled { Self.answer("statsFilled") }
            }
        }

        /// A refresh over what is shown; on a route's sample, the kept results go first, as on
        /// `CollectionReplaced`.
        func reload() {
            guard let page else { return }
            if sample { page.model.receive(.collectionReplaced) } else { page.model.refresh() }
        }

        /// Answers `statsPinned` when a box holds a pinned selection (the pin's own callout is
        /// hidden from accessibility).
        func queryPin() {
            if page?.model.selections.values.contains(where: \.pinned) == true { Self.answer("statsPinned") }
        }

        /// Answers every query with `statsSelected`: the selections as its object
        /// (`<box>:<bin>:<pinned>`, sorted), "none" when no box holds one, "no page" without one.
        func querySelection() {
            guard let selections = page?.model.selections else { return Self.answer("statsSelected", object: "no page") }
            let text = selections.filter { $0.value.bin != nil }
                .map { "\($0.key.rawValue):\($0.value.bin.map(String.init) ?? "-"):\($0.value.pinned)" }.sorted()
            Self.answer("statsSelected", object: text.isEmpty ? "none" : text.joined(separator: ","))
        }

        func select(_ period: StatsPagePeriod) {
            page?.selectPeriod(period)
        }

        /// Scrolls the page section by section for about three seconds, animated: while it reads
        /// (R2078's window, `stats.scrollDuringRead`), or with `reading` false over the filled page
        /// with nothing reading (`stats.scrollSettled`, `statsScrollSettled`).
        func scroll(reading: Bool = true) {
            guard let page else { return }
            Task { @MainActor in
                // The display's own pacing with nothing moving, as a baseline for the run's machine.
                let idle = StatsFramePacing()
                idle.start()
                try? await Task.sleep(for: .seconds(1))
                let baseline = idle.stop()
                let token = Signposts.begin(reading ? IntervalNames.statsScrollDuringRead : IntervalNames.statsScrollSettled)
                let frames = StatsFramePacing()
                frames.start()
                if reading { page.model.refresh() }
                if let native = StatsNativeScroll() {
                    // The platform scroll view itself, a step at a time, as a trackpad or a finger
                    // moves it: no SwiftUI state changes, as in a person's scroll.
                    await native.run()
                } else {
                    let order = StatsSectionID.allCases + StatsSectionID.allCases.reversed()
                    for section in order {
                        page.go(to: section)
                        try? await Task.sleep(for: .milliseconds(220))
                    }
                }
                Signposts.end(token)
                let pacing = frames.stop()
                #if os(iOS)
                // Darwin notifications carry no object: the frames counted (high 32 bits) and the
                // ratio in thousandths of a ms/s (low 32) are the notification's state, read by the
                // test. notifyd keeps a name's state only while a client holds it, so the token
                // stays registered.
                let name = "\(Bundle.main.bundleIdentifier ?? "").testhooks.statsScrolled"
                if stateToken == nil {
                    var token: Int32 = 0
                    if notify_register_check(name, &token) == NOTIFY_STATUS_OK { stateToken = token }
                }
                if let stateToken {
                    let ratio = UInt64(min(max(0, pacing.ratio * 1_000).rounded(), Double(UInt32.max)))
                    notify_set_state(stateToken, UInt64(UInt32(clamping: pacing.frames)) << 32 | ratio)
                }
                #endif
                Self.answer("statsScrolled", object: "\(pacing.description);idleRatio=\(String(format: "%.2f", baseline.ratio))")
            }
        }

        private static func answer(_ name: String, object: String? = nil) {
            let full = "\(Bundle.main.bundleIdentifier ?? "").testhooks.\(name)"
            #if os(macOS)
            DistributedNotificationCenter.default().postNotificationName(Notification.Name(full), object: object, userInfo: nil,
                                                                         deliverImmediately: true)
            #else
            notify_post(full)
            #endif
        }
    }
}
#endif
