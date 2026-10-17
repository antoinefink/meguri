#if MEGURI_TEST_HOOKS
import AppModel
import DesignSystem
import SwiftUI
#if os(macOS)
import AppKit
#else
import UIKit
import notify
#endif

extension TestHooks {
    /// A shell route's launch: the features the shell hosts and the state its
    /// navigation starts in.
    struct ShellLaunch {
        var features: ShellFeatures
        /// Whether the navigation state probe shows: in the routes tests
        /// start from, never in a screen state's (audits and snapshots see
        /// the product's screen only).
        var probe = true
        var prepare: (AppNavigation) -> Void = { _ in }
    }

    /// The app shell's routes (U150): the shell with sample content in each
    /// of its states (`shell/root/<state>`, the states of `shell.root` in
    /// the screen inventory), and the states its navigation tests start from.
    /// The sample content stands in for features other units build: decks
    /// in the sidebar, scrolling section roots, a deck page with a Study
    /// button, and a review that keeps its card and side.
    enum ShellRoutes {
        static func register(in registry: inout RouteRegistry) {
            registry.registerShell("shell/root/typical") { _ in ShellLaunch(features: ShellSample.features(.typical), probe: false) }
            registry.registerShell("shell/root/loading") { _ in ShellLaunch(features: ShellSample.features(.loading), probe: false) }
            registry.registerShell("shell/root/error") { _ in ShellLaunch(features: ShellSample.features(.error), probe: false) }
            registry.registerShell("shell/root/deck-page") { _ in
                ShellLaunch(features: ShellSample.features(.scrolling)) { navigation in
                    navigation.openDeckPage(ShellSample.spanishMining)
                }
            }
            // A selected deck, then Browse with Note Types pushed and its root
            // scrolled to row 20 (the layout switch scenario, R5).
            registry.registerShell("shell/root/browse-pushed") { _ in
                ShellLaunch(features: ShellSample.features(.scrolling)) { navigation in
                    navigation.openDeckPage(ShellSample.spanishMining)
                    navigation.select(.section(.browse))
                    navigation.push(.noteTypes)
                    navigation.setScrollPosition(ScrollPosition(id: SampleSectionContent.rowID(20), anchor: .top), for: .section(.browse))
                }
            }
            // A review of Spanish::Mining showing card 1001's answer.
            registry.registerShell("shell/root/review-answer") { _ in
                ShellLaunch(features: ShellSample.features(.scrolling)) { navigation in
                    navigation.openDeckPage(ShellSample.spanishMining)
                    let session = SampleReviewSession(cardID: 1001)
                    session.revealed = true
                    navigation.startReview(ReviewPresentation(deck: ShellSample.spanishMining, session: session))
                }
            }
        }
    }

    // MARK: Sample content

    /// Sample decks and the features built from them.
    enum ShellSample {
        static let defaultDeck = DeckID(1)
        static let japanese = DeckID(1_700_000_000_002)
        static let spanish = DeckID(1_700_000_000_010)
        static let spanishMining = DeckID(1_700_000_000_001)

        /// The sample tree, in tree order: id, full name.
        static let decks: [(id: DeckID, name: String)] = [
            (defaultDeck, "Default"), (japanese, "Japanese"), (spanish, "Spanish"), (spanishMining, "Spanish::Mining"),
        ]

        static func name(_ deck: DeckID) -> String {
            decks.first { $0.id == deck }?.name ?? "Deck \(deck.rawValue)"
        }

        enum Content {
            /// The shell's typical state: sample decks around the app's
            /// own section placeholders.
            case typical
            case loading, error
            /// Roots and a deck page that scroll, for the navigation tests.
            case scrolling
        }

        static func features(_ content: Content) -> ShellFeatures {
            var features = ShellFeatures.standard
            features.sectionRoot = { section, navigation in
                AnyView(
                    Group {
                        switch content {
                        case .typical:
                            SectionPlaceholder(section: section, navigation: navigation)
                        case .scrolling:
                            SampleSectionContent(section: section, navigation: navigation)
                        case .loading:
                            LoadingStateView(Text("Loading…"))
                        case .error:
                            ErrorStateView(Text("Couldn’t Open the Collection"), message: Text("The collection is busy. Try again in a moment."))
                        }
                    }
                    .navigationTitle(Text(section.title))
                    .accessibilityElement(children: .contain)
                    .accessibilityIdentifier("testhooks.shell.root.\(section.rawValue)")
                )
            }
            if content == .scrolling {
                features.deckPage = { deck, navigation in AnyView(SampleDeckPage(deck: deck, navigation: navigation)) }
            }
            features.sidebarDecks = { _ in AnyView(SampleSidebarDecks()) }
            features.review = { presentation, navigation in
                AnyView(SampleReview(presentation: presentation, navigation: navigation))
            }
            let known = Set(decks.map(\.id))
            features.deckExists = { known.contains($0) }
            return features
        }
    }

    /// The sidebar's sample deck rows, indented by depth.
    struct SampleSidebarDecks: View {
        var body: some View {
            ForEach(ShellSample.decks, id: \.id) { deck in
                let parts = deck.name.components(separatedBy: "::")
                Label {
                    Text(verbatim: parts.last ?? deck.name)
                        .accessibilityIdentifier("testhooks.shell.deck.\(deck.id.rawValue)")
                } icon: {
                    Image(systemName: "rectangle.stack")
                        .accessibilityHidden(true)
                }
                .padding(.leading, Spacing.medium * CGFloat(parts.count - 1))
                .tag(SidebarItem.deck(deck.id))
            }
        }
    }

    /// A section root that scrolls: 40 rows, its scroll position kept by the
    /// navigation (R5, R19).
    struct SampleSectionContent: View {
        let section: AppSection
        let navigation: AppNavigation

        static func rowID(_ row: Int) -> String { "row-\(row)" }

        var body: some View {
            SampleRows(key: .section(section), prefix: section.rawValue, navigation: navigation) {
                if section == .browse, !navigation.browseSearch.isEmpty {
                    Text("Search: \(navigation.browseSearch)")
                        .font(.headline)
                        .accessibilityIdentifier("testhooks.shell.browseSearch")
                }
            }
        }
    }

    /// 40 rows under a header, scrolled where the navigation says.
    struct SampleRows<Header: View>: View {
        let key: ScrollAnchorKey
        let prefix: String
        let navigation: AppNavigation
        @ViewBuilder let header: () -> Header

        var body: some View {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: Spacing.small) {
                    header()
                    ForEach(1...40, id: \.self) { row in
                        Text("Row \(row)")
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(Spacing.small)
                            .background(.token(.backgroundSecondary), in: .rect(cornerRadius: Radius.small))
                            .id(SampleSectionContent.rowID(row))
                            .accessibilityIdentifier("testhooks.shell.\(prefix)Row.\(row)")
                    }
                }
                .scrollTargetLayout()
                .padding(Spacing.medium)
                .frame(maxWidth: ContentWidth.readable)
                .frame(maxWidth: .infinity)
            }
            .navigationScrollPosition(navigation, key, idType: String.self)
            .background(.token(.backgroundPrimary))
        }
    }

    /// A deck page that scrolls, with a Study button that opens the sample
    /// review.
    struct SampleDeckPage: View {
        let deck: DeckID
        let navigation: AppNavigation

        var body: some View {
            SampleRows(key: .deckPage, prefix: "deckPage", navigation: navigation) {
                Text(verbatim: ShellSample.name(deck))
                    .font(.title2.bold())
                    .accessibilityIdentifier("testhooks.shell.deckPage.title")
                Button("Study") {
                    navigation.startReview(ReviewPresentation(deck: deck, session: SampleReviewSession(cardID: 1001)))
                }
                .buttonStyle(.borderedProminent)
                .accessibilityIdentifier("testhooks.shell.deckPage.study")
            }
            .navigationTitle(Text(verbatim: ShellSample.name(deck)))
        }
    }

    /// The review session's stand-in: the card on screen, its side and the
    /// answers given. The shell holds it by reference; whatever happens to
    /// the views, a card and its side stay as they were (R5).
    @Observable
    final class SampleReviewSession {
        var cardID: Int
        var revealed = false
        var answers = 0

        init(cardID: Int) {
            self.cardID = cardID
        }
    }

    struct SampleReview: View {
        let presentation: ReviewPresentation
        let navigation: AppNavigation

        var body: some View {
            if let session = presentation.session as? SampleReviewSession {
                VStack(spacing: Spacing.medium) {
                    Text(verbatim: ShellSample.name(presentation.deck))
                        .font(.headline)
                        .accessibilityIdentifier("testhooks.shell.review.deck")
                    Text(verbatim: "Card \(session.cardID)")
                        .font(.largeTitle)
                        .accessibilityIdentifier("testhooks.shell.review.card")
                    Text(verbatim: session.revealed ? "Answer" : "Question")
                        .accessibilityIdentifier("testhooks.shell.review.side")
                    Text(verbatim: "Answers: \(session.answers)")
                        .accessibilityIdentifier("testhooks.shell.review.answers")
                    if session.revealed {
                        Button("Good") {
                            session.answers += 1
                            session.cardID += 1
                            session.revealed = false
                        }
                        .accessibilityIdentifier("testhooks.shell.review.good")
                    } else {
                        Button("Show Answer") { session.revealed = true }
                            .accessibilityIdentifier("testhooks.shell.review.showAnswer")
                    }
                    Button("Close") { navigation.endReview() }
                        .accessibilityIdentifier("testhooks.shell.review.close")
                }
                .buttonStyle(.bordered)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(.token(.backgroundPrimary))
            }
        }
    }

    // MARK: Probe and commands

    /// The navigation's state in one line, for UI tests: what no screen
    /// shows at once (the selected deck while Browse is shown, every stack,
    /// the scroll positions, the columns). A strip below the shell
    /// (`TestHooks.Root`), so screenshots show it beside the screen.
    struct ShellStateProbe: View {
        let navigation: AppNavigation
        #if os(iOS)
        @Environment(\.horizontalSizeClass) private var sizeClass
        #endif

        private var layout: String {
            #if os(iOS)
            sizeClass == .compact ? "compact" : "sidebar"
            #else
            "sidebar"
            #endif
        }

        var body: some View {
            Text(verbatim: "layout=\(layout) " + Self.describe(navigation))
                .font(.caption2)
                // A few lines at any text size, never cut: the screen above
                // it is what an accessibility-size screenshot is for.
                .dynamicTypeSize(...DynamicTypeSize.large)
                .fixedSize(horizontal: false, vertical: true)
                .foregroundStyle(.token(.textSecondary))
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, Spacing.medium)
                .padding(.vertical, Spacing.xxSmall)
                .background(.token(.backgroundSecondary))
                .allowsHitTesting(false)
                .accessibilityIdentifier("testhooks.shell.state")
        }

        static func describe(_ navigation: AppNavigation) -> String {
            func path(_ section: AppSection) -> String {
                navigation.path(section).map { destination in
                    switch destination {
                    case let .deckPage(deck): "deck:\(deck.rawValue)"
                    case .noteTypes: "noteTypes"
                    }
                }.joined(separator: ",")
            }
            func scroll(_ key: ScrollAnchorKey) -> String {
                let position = navigation.scrollPosition(for: key)
                if let id = position.viewID { return "\(id)" }
                return position.edge == .top ? "top" : "none"
            }
            let columns = switch navigation.columnVisibility {
            case .all: "all"
            case .detailOnly: "detailOnly"
            case .doubleColumn: "doubleColumn"
            default: "automatic"
            }
            return [
                "section=\(navigation.section.rawValue)",
                "deck=\(navigation.selectedDeck.map { String($0.rawValue) } ?? "none")",
                "home=\(path(.home))", "browse=\(path(.browse))", "settings=\(path(.settings))",
                "scroll.browse=\(scroll(.section(.browse)))", "scroll.deckPage=\(scroll(.deckPage))",
                "search=\(navigation.browseSearch)",
                "review=\(navigation.review.map { String($0.deck.rawValue) } ?? "none")",
                "sheet=\(navigation.sheet == nil ? "none" : "add")",
                "columns=\(columns)",
            ].joined(separator: " ")
        }
    }

    #if os(iOS)
    /// Forces the window's horizontal size class (`-MeguriSizeClass`).
    struct SizeClassOverride: UIViewRepresentable {
        let sizeClass: LaunchOptions.SizeClass?

        func makeUIView(context: Context) -> OverrideView {
            OverrideView(sizeClass: sizeClass)
        }

        func updateUIView(_ view: OverrideView, context: Context) {}

        /// Applies the option once per run: SwiftUI may build this view
        /// again (a layout switch does), and a toggle must stay.
        final class OverrideView: UIView {
            private static var applied = false
            private let sizeClass: LaunchOptions.SizeClass?

            init(sizeClass: LaunchOptions.SizeClass?) {
                self.sizeClass = sizeClass
                super.init(frame: .zero)
            }

            @available(*, unavailable)
            required init?(coder: NSCoder) {
                fatalError("init(coder:) is not used")
            }

            override func didMoveToWindow() {
                super.didMoveToWindow()
                guard !Self.applied, let sizeClass, let scene = window?.windowScene else { return }
                Self.applied = true
                ShellCommands.force(sizeClass == .compact ? .compact : .regular, on: scene)
            }
        }
    }
    #endif

    /// Commands a UI test posts while the app runs (Darwin notifications on
    /// iOS, distributed ones on the Mac, named `<bundle id>.testhooks.<command>`):
    /// they need no synthesized input, which macOS drops on this toolchain
    /// (app/AGENTS.md).
    ///
    /// - `toggleSizeClass` (iOS): flips the window's horizontal size class,
    ///   switching layouts without a relaunch (R5).
    /// - `presentAdd`: opens Add over the current screen, as the toolbar "+"
    ///   will (R24, R25), with the selected deck.
    /// - `closeWindow` (Mac): closes the main window as its close button does
    ///   (R11).
    final class ShellCommands: NSObject {
        enum Command: String, CaseIterable {
            case toggleSizeClass, presentAdd, closeWindow
        }

        static let shared = ShellCommands()

        static func name(_ command: Command) -> String {
            "\(Bundle.main.bundleIdentifier ?? "").testhooks.\(command.rawValue)"
        }

        private var navigation: AppNavigation?
        private var listening = false
        #if os(iOS)
        private var tokens: [Int32] = []
        #endif

        func listen(navigation: AppNavigation, sizeClass: LaunchOptions.SizeClass?) {
            self.navigation = navigation
            guard !listening else { return }
            listening = true
            for command in Command.allCases {
                #if os(macOS)
                DistributedNotificationCenter.default().addObserver(
                    self, selector: #selector(received(_:)), name: Notification.Name(Self.name(command)), object: nil,
                    suspensionBehavior: .deliverImmediately
                )
                #else
                var token = NOTIFY_TOKEN_INVALID
                notify_register_dispatch(Self.name(command), &token, .main) { [weak self] _ in
                    MainActor.assumeIsolated { self?.perform(command) }
                }
                tokens.append(token)
                #endif
            }
        }

        #if os(macOS)
        @objc private func received(_ notification: Notification) {
            guard let command = Command.allCases.first(where: { Self.name($0) == notification.name.rawValue }) else { return }
            perform(command)
        }
        #endif

        private func perform(_ command: Command) {
            switch command {
            case .toggleSizeClass:
                #if os(iOS)
                guard let scene = UIApplication.shared.connectedScenes.compactMap({ $0 as? UIWindowScene }).first else { return }
                Self.force(Self.current(on: scene) == .compact ? .regular : .compact, on: scene)
                #endif
            case .presentAdd:
                guard let navigation else { return }
                navigation.present(.add(deck: navigation.selectedDeck))
            case .closeWindow:
                #if os(macOS)
                NSApplication.shared.windows.first { $0.isVisible && $0.canBecomeMain }?.performClose(nil)
                #endif
            }
        }

        #if os(iOS)
        /// The size class forced last; nil until a launch option or a toggle.
        private(set) static var forced: UIUserInterfaceSizeClass?

        /// Overrides the trait on the scene's windows: the scene's own
        /// override does not reach SwiftUI's size class on an iPhone.
        static func force(_ sizeClass: UIUserInterfaceSizeClass, on scene: UIWindowScene) {
            forced = sizeClass
            for window in scene.windows {
                window.traitOverrides.horizontalSizeClass = sizeClass
                // Trait overrides apply at the next layout pass.
                window.setNeedsLayout()
                window.layoutIfNeeded()
            }
        }

        static func current(on scene: UIWindowScene) -> UIUserInterfaceSizeClass {
            forced ?? scene.traitCollection.horizontalSizeClass
        }
        #endif
    }
}

// MARK: Snapshot and preview states

/// The shell with sample content in each of `shell.root`'s states: the tab
/// layout at compact width, the sidebar layout at regular width and on the
/// Mac (R2–R4).
struct ShellRootPreview: View {
    let content: TestHooks.ShellSample.Content
    @State private var navigation = AppNavigation()

    var body: some View {
        RootView(
            configuration: LaunchConfiguration(), navigation: navigation,
            features: TestHooks.ShellSample.features(content), uiStateStore: UIStateStore(defaults: nil)
        )
    }
}

extension ShellRootPreview: PreviewStates {
    static let previewID = "shell.root"

    static var previewFixtures: [PreviewFixture] {
        [
            PreviewFixture(.typical) { Self(content: .typical) },
            PreviewFixture(.loading) { Self(content: .loading) },
            PreviewFixture(.error) { Self(content: .error) },
        ]
    }
}

#Preview("Typical") { ShellRootPreview.preview(.typical) }
#Preview("Loading") { ShellRootPreview.preview(.loading) }
#Preview("Error") { ShellRootPreview.preview(.error) }
#endif
