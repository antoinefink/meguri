#if MEGURI_TEST_HOOKS
@_spi(MeguriTestHooks) import AppModel
import CoreBridge
import Foundation
import Observation
import Stats
import SwiftUI

extension TestHooks {
    /// Production shells over synthetic reads, with no collection opened. Mac tests
    /// release gates explicitly; touch captures start with a read tree and hold selection.
    enum NavigationFeedbackRoutes {
        static func register(in registry: inout RouteRegistry) {
            registry.register("navigation/feedback") { context in
                #if os(macOS)
                .screen { NavigationFeedbackWindow(configuration: context.configuration) }
                #else
                .screen { NavigationFeedbackWindow(configuration: context.configuration, preparedHome: true) }
                #endif
            }
        }
    }

    @MainActor @Observable final class NavigationReadGate {
        private var held = true
        private var waiters: [CheckedContinuation<Void, Never>] = []
        private(set) var calls = 0
        var fails = false

        func wait() async {
            calls += 1
            guard held else { return }
            await withCheckedContinuation { waiters.append($0) }
        }
        func hold() { held = true }
        func release() {
            held = false
            let pending = waiters
            waiters = []
            pending.forEach { $0.resume() }
        }
    }

    nonisolated private struct NavigationStatsService: StatsService {
        let gate: NavigationReadGate
        private let sample = StatsSampleService(tierDelay: .zero)
        init(gate: NavigationReadGate) { self.gate = gate }
        func page(_ request: StatsPageRequest) async throws(BridgeError) -> StatsPageStream {
            await gate.wait()
            guard !Task.isCancelled else { throw .cancelled }
            return try await sample.page(request)
        }
        func dayCards(scope: StatsPageScope, context: StatsContext, day: Int, kind: StatsDayKind,
                      cancel: CancelToken?) async throws(BridgeError) -> StatsDayCards {
            try await sample.dayCards(scope: scope, context: context, day: day, kind: kind, cancel: cancel)
        }
        func releaseCaches() async throws(BridgeError) {}
        func homeIdle() async throws(BridgeError) {}
    }

    @MainActor private final class NavigationFeedbackFixture {
        let configuration: LaunchConfiguration
        let navigation = AppNavigation()
        let collection: ReviewCollection
        let features: ShellFeatures
        let home = NavigationReadGate()
        let deck = NavigationReadGate()
        let browse = NavigationReadGate()
        let list = NavigationReadGate()
        let record = NavigationReadGate()
        let stats = NavigationReadGate()
        #if os(macOS)
        private var observer: NSObjectProtocol?
        #endif

        init(configuration input: LaunchConfiguration, preparedHome: Bool) {
            var configuration = input
            configuration.initialPresentation = nil
            configuration.deviceDefaults = nil
            configuration.homeSnapshotFile = nil
            configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
            // An isolated nonexistent directory; never the real app's collection.
            configuration.collectionLocation = .init(directory: FileManager.default.temporaryDirectory
                .appending(path: "MeguriNavigation-\(UUID().uuidString)", directoryHint: .isDirectory))
            self.configuration = configuration
            let collection = ReviewCollection(configuration: configuration, navigation: navigation)
            self.collection = collection
            let browseBackend = SampleBrowseBackend()
            let browseGate = browse
            browseBackend.beforeSearchSession = { await browseGate.wait() }
            let noteBackend = SampleNoteTypesBackend()
            let listGate = list, recordGate = record
            noteBackend.beforeList = { await listGate.wait() }
            noteBackend.beforeRecord = { await recordGate.wait() }
            let homeGate = home, deckGate = deck
            if preparedHome { homeGate.release() }
            collection.configureNavigationFixture(generation: 1, tree: {
                await homeGate.wait()
                return CountedDeckTree(generation: 1, nextChangeAt: Int64.max, nodes: StatsSampleService.decks)
            }, select: { _ in
                await deckGate.wait()
                if deckGate.fails { throw BridgeError.invalidArgument("Synthetic deck selection failure") }
                return 1
            }, deckEmpty: { id in
                // Every modeled deck contains cards. No secondary page read may
                // reach the deliberately nonexistent collection directory.
                func contains(_ nodes: [CountedDeckNode]) -> Bool {
                    nodes.contains { $0.id == id || contains($0.children) }
                }
                guard contains(StatsSampleService.decks) else {
                    throw BridgeError.invalidArgument("Unknown synthetic deck")
                }
                return false
            }, browse: BrowseModel(backend: browseBackend, store: BrowseDeviceStore(defaults: nil)),
               noteTypes: NoteTypesModel(backend: noteBackend))
            StatsRollout.enabled = true
            features = .standard(collection: collection, statsService: NavigationStatsService(gate: stats))
        }

        func listen() {
            #if os(macOS)
            guard observer == nil, let bundle = Bundle.main.bundleIdentifier else { return }
            observer = DistributedNotificationCenter.default().addObserver(
                forName: Notification.Name("\(bundle).testhooks.navigation.command"), object: nil, queue: .main
            ) { [weak self] notification in
                guard let command = notification.object as? String else { return }
                Task { @MainActor [weak self] in
                    guard let self else { return }
                    let gates = ["home": home, "deck": deck, "browse": browse, "list": list, "record": record, "stats": stats]
                    let pieces = command.split(separator: ".").map(String.init)
                    if pieces.count == 2, let gate = gates[pieces[1]] {
                        if pieces[0] == "release" { gate.release() }
                        if pieces[0] == "hold" { gate.hold() }
                        if pieces[0] == "fail" { gate.fails = true }
                        if pieces[0] == "succeed" { gate.fails = false }
                    }
                    if command == "settle.deck" { await collection.deckPageOpened() }
                    if command == "refresh.home" { collection.refreshCounts() }
                    if command == "refresh.list" { collection.noteTypes.reloadList() }
                    let state = ["home=\(home.calls)", "deck=\(deck.calls)", "browse=\(browse.calls)",
                                 "list=\(list.calls)", "record=\(record.calls)", "stats=\(stats.calls)",
                                 "filled=\(features.statsPage?.filled == true)", "section=\(navigation.section.rawValue)",
                                 "pending=\(navigation.pendingDeckPage?.rawValue ?? 0)",
                                 "state=\(collection.state)", "homePhase=\(collection.homePhase)",
                                 "generation=\(collection.generation)", "treeGeneration=\(collection.deckTreeGeneration ?? 0)",
                                 "commandFailed=\(collection.error != nil)"]
                        .joined(separator: ",")
                    DistributedNotificationCenter.default().postNotificationName(
                        Notification.Name("\(bundle).testhooks.navigation.ack"), object: "\(command)|\(state)",
                        userInfo: nil, deliverImmediately: true)
                }
            }
            #endif
        }

        func stop() {
            #if os(macOS)
            if let observer { DistributedNotificationCenter.default().removeObserver(observer) }
            observer = nil
            #endif
            for gate in [home, deck, browse, list, record, stats] { gate.release() }
        }
    }

    struct NavigationFeedbackWindow: View {
        @State private var fixture: NavigationFeedbackFixture
        init(configuration: LaunchConfiguration, preparedHome: Bool = false) {
            _fixture = State(initialValue: NavigationFeedbackFixture(configuration: configuration, preparedHome: preparedHome))
        }
        var body: some View {
            #if os(macOS)
            SidebarRootView(navigation: fixture.navigation, features: fixture.features, configuration: fixture.configuration)
                .focusedSceneValue(\.nativeCommandContext, NativeCommandContext(collection: fixture.collection,
                    host: .main, layout: .sidebar, statsPage: fixture.features.statsPage))
                .environment(\.shellLayout, .sidebar)
                .modifier(LaunchEnvironment(configuration: fixture.configuration))
                .onAppear { fixture.listen() }
                .onDisappear { fixture.stop() }
            #else
            CompactRootView(navigation: fixture.navigation, features: fixture.features,
                configuration: fixture.configuration, hasDecks: !fixture.collection.homeDecks.isEmpty, besideDetail: false)
                .environment(\.shellLayout, .compact)
                .modifier(LaunchEnvironment(configuration: fixture.configuration))
                .onDisappear { fixture.stop() }
            #endif
        }
    }
}
#endif
