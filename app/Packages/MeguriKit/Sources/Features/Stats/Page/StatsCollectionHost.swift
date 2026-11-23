import AppModel
import CoreBridge
import Foundation
import Observation

/// The Statistics page over the app's collection (plan U296): it gives the page model the
/// collection's opens and closes with a per-collection identity, the deck list's tree, G10's
/// staleness, and the memory warning, and tells the cache signals when Home is idle (G9.2).
/// Made by the shell only while Statistics can be reached (`StatsRollout`), so a launch without
/// it, and a test of `ReviewCollection`, starts no background cache build.
@MainActor
public final class StatsCollectionHost {
    public let page: StatsPage
    private let signals: StatsCacheSignals
    private let identity: StatsCollectionIdentity
    private weak var collection: ReviewCollection?
    private var ready = false
    private var appliedTree: (generation: UInt64, nodes: [CountedDeckNode])?

    /// - Parameters:
    ///   - service: the core's statistics (tests and routes pass their own).
    ///   - watchesMemory: listen to the system's memory pressure (off in tests).
    public init(collection: ReviewCollection, service: any StatsService = CoreStatsService(), watchesMemory: Bool = true) {
        let defaults = collection.configuration.deviceDefaults
        // A launch that keeps no device state (the test hooks) keeps its results in memory only.
        let kept = StatsKeptResults(directory: defaults == nil ? nil : StatsKeptResults.standardDirectory)
        let model = StatsPageModel(service: service, kept: kept, store: StatsSelectionStore(defaults: defaults))
        page = StatsPage(model: model)
        signals = StatsCacheSignals(service: service, watchesMemory: watchesMemory)
        identity = StatsCollectionIdentity(defaults: defaults)
        self.collection = collection
        signals.onMemoryWarning.append { [weak model] in model?.memoryWarning() }
        collection.statsStaleness = { [weak self] staleness in self?.received(staleness) }
        track()
    }

    /// G10's staleness from the collection's events; a replacement is another collection.
    private func received(_ staleness: StatsStaleness) {
        if staleness == .collectionReplaced { identity.renew() }
        page.model.receive(staleness)
    }

    // MARK: Observing the collection

    /// What the page reads from the collection, re-read after each change.
    private struct Observed: Equatable {
        var ready: Bool
        var opening: Bool
        var tree: UInt64?
        var homeIdle: Bool
    }

    private func observe() -> Observed? {
        guard let collection else { return nil }
        let navigation = collection.navigation
        let opening = collection.state == .loading || collection.state == .unopened
        let homeIdle = navigation.section == .home && navigation.review == nil && navigation.sheet == nil
            && collection.review == nil && collection.foreground && collection.state == .ready
        let tree = collection.deckTreeGeneration == collection.generation ? collection.deckTreeGeneration : nil
        // Read so a change of the tree's counts is seen.
        _ = collection.decks
        return Observed(ready: collection.browseReadable, opening: opening, tree: tree, homeIdle: homeIdle)
    }

    private func track() {
        let observed = withObservationTracking { observe() } onChange: { [weak self] in
            Task { @MainActor in self?.track() }
        }
        guard let observed else { return }
        apply(observed)
    }

    private func apply(_ observed: Observed) {
        guard let collection else { return }
        page.opening = observed.opening
        if observed.ready != ready {
            ready = observed.ready
            if ready {
                page.model.collectionOpened(identity: identity.current)
                signals.collectionOpened()
            } else {
                appliedTree = nil
                page.model.collectionClosed()
                signals.collectionClosed()
            }
        }
        // The tree once read for this collection: before that a deck shown is not known gone.
        if ready, let generation = observed.tree,
           appliedTree?.generation != generation || appliedTree?.nodes != collection.decks {
            appliedTree = (generation, collection.decks)
            page.model.decksChanged(collection.decks)
        }
        signals.homeIdle(observed.homeIdle)
    }
}

/// A collection's identity for the page's kept results (S10.2): a random token kept in the
/// device's defaults, renewed when the collection is replaced, so results never cross from one
/// collection to another. It names no file or path; with no defaults (test-hook launches) each
/// launch has its own.
@MainActor
final class StatsCollectionIdentity {
    static let key = "meguri.stats.collectionToken"
    private let defaults: UserDefaults?
    private(set) var current: String

    init(defaults: UserDefaults?) {
        self.defaults = defaults
        if let stored = defaults?.string(forKey: Self.key), !stored.isEmpty {
            current = stored
        } else {
            current = UUID().uuidString
            defaults?.set(current, forKey: Self.key)
        }
    }

    /// Another collection replaced the open one: its results start afresh.
    func renew() {
        current = UUID().uuidString
        defaults?.set(current, forKey: Self.key)
    }
}
