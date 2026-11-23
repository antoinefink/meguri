import CoreBridge
import Foundation
import Observation

/// The page's kept results (`stats/statistics-page` S10.2): the last sets the page showed for
/// each (collection, page scope, history), in memory for every window and on the device in the
/// app's caches directory (never synced, never in the collection; deleting them loses nothing).
///
/// A set is kept under its own history: a `last12Months` set serves every period from 1M to
/// 1Y (S3.2 re-bins it with no core call), and on All's axis it is partial (S10.3). Sets are
/// kept only for the collection they were read from: another collection, or
/// `CollectionReplaced`, drops them all.
@Observable @MainActor
public final class StatsKeptResults {
    nonisolated struct Key: Hashable, Sendable, Codable {
        let scope: StatsPageScope
        let history: StatsHistory
    }

    /// Sets per key, newest first; at most `perKey`, none older than the newest complete one.
    private var entries: [Key: [StatsPartSet]] = [:]
    @ObservationIgnored private var recent: [Key] = []
    @ObservationIgnored private var loading: [Key: Task<Void, Never>] = [:]
    /// The collection the sets are of; nil keeps nothing.
    public private(set) var collection: String?
    @ObservationIgnored private let disk: StatsKeptDisk?
    /// Moved by every drop: a write or a read started before it never lands after it.
    @ObservationIgnored private var epoch: UInt64 = 0

    static let perKey = 3
    static let keysInMemory = 8

    /// - Parameter directory: where the sets are kept on the device (`<Caches>/Statistics`);
    ///   nil keeps them in memory only (tests, test-hook launches).
    public init(directory: URL?) {
        disk = directory.map(StatsKeptDisk.init)
    }

    /// The app's caches directory.
    public static var standardDirectory: URL {
        URL.cachesDirectory.appending(path: "Statistics", directoryHint: .isDirectory)
    }

    /// Selects the collection the page reads. Another one than before drops every kept set.
    public func open(collection: String) {
        guard collection != self.collection else { return }
        let had = self.collection != nil
        self.collection = collection
        drop(disk: had)
    }

    /// No collection is open: nothing is shown or kept from now on until `open`.
    public func close() {
        collection = nil
        drop(disk: false)
    }

    /// `CollectionReplaced` (S10.2, S11): every kept set goes, on the device too.
    public func collectionReplaced() {
        drop(disk: true)
    }

    /// A memory warning: keeps only the sets of `scope`, which a page shows.
    public func trim(keeping scope: StatsPageScope?) {
        for key in Array(entries.keys) where key.scope != scope { entries[key] = nil }
        recent.removeAll { $0.scope != scope }
    }

    private func drop(disk: Bool) {
        for task in loading.values { task.cancel() }
        loading = [:]
        entries = [:]
        recent = []
        epoch &+= 1
        if disk, let store = self.disk {
            let epoch = epoch
            Task { await store.removeAll(epoch: epoch) }
        }
    }

    /// The kept sets of `scope`, both histories, newest first.
    func sets(for scope: StatsPageScope) -> [StatsPartSet] {
        let all = (entries[Key(scope: scope, history: .allHistory)] ?? []) + (entries[Key(scope: scope, history: .last12Months)] ?? [])
        return all.sorted { $0.isNewer(than: $1) }
    }

    /// Whether a complete set for `scope` and `history` is kept.
    func hasComplete(_ scope: StatsPageScope, _ history: StatsHistory) -> Bool {
        entries[Key(scope: scope, history: history)]?.contains(where: \.complete) == true
    }

    /// Keeps `set` (a running refresh's set is kept as it fills): it replaces the set of the same
    /// refresh, and a complete set makes the older ones go.
    func keep(_ set: StatsPartSet) {
        guard collection != nil, set.header != nil else { return }
        let key = Key(scope: set.scope, history: set.history)
        var sets = entries[key] ?? []
        sets.removeAll { $0.id == set.id }
        sets.append(set)
        sets.sort { $0.isNewer(than: $1) }
        if let complete = sets.firstIndex(where: \.complete) { sets = Array(sets.prefix(complete + 1)) }
        entries[key] = Array(sets.prefix(Self.perKey))
        touch(key)
    }

    /// Writes the kept sets of `scope` to the device, off the main actor.
    func persist(_ scope: StatsPageScope) {
        guard let disk, let collection else { return }
        for history in [StatsHistory.last12Months, .allHistory] {
            let key = Key(scope: scope, history: history)
            guard let sets = entries[key], !sets.isEmpty else { continue }
            let epoch = epoch
            Task { await disk.save(sets, key: key, collection: collection, epoch: epoch) }
        }
    }

    /// Reads the kept sets of `scope` from the device into memory, off the main actor, unless
    /// they are already there; `loaded` runs on the main actor when sets arrived.
    func prepare(_ scope: StatsPageScope, loaded: @escaping @MainActor () -> Void) {
        guard let disk, let collection else { return }
        for history in [StatsHistory.last12Months, .allHistory] {
            let key = Key(scope: scope, history: history)
            guard entries[key] == nil, loading[key] == nil else { continue }
            let epoch = epoch
            loading[key] = Task { [weak self] in
                let sets = await disk.load(key: key, collection: collection, epoch: epoch)
                guard let self, !Task.isCancelled, self.epoch == epoch, self.collection == collection else { return }
                self.loading[key] = nil
                guard let sets, !sets.isEmpty else { return }
                // A set kept meanwhile (this launch's refresh) is newer; the read ones join it.
                for set in sets.reversed() where set.scope == scope && set.history == history { self.keep(set) }
                loaded()
            }
        }
    }

    /// Waits for the reads `prepare` started (tests).
    func waitForLoads() async {
        for task in Array(loading.values) { await task.value }
    }

    private func touch(_ key: Key) {
        recent.removeAll { $0 == key }
        recent.append(key)
        while recent.count > Self.keysInMemory {
            let old = recent.removeFirst()
            entries[old] = nil
        }
    }
}

/// The kept sets' files: one per (scope, history), a binary property list (it keeps the NaN a
/// memory-state value can be), read back only for the same collection and format.
actor StatsKeptDisk {
    nonisolated private struct File: Codable {
        static let format = 1
        let format: Int
        let collection: String
        let sets: [StatsPartSet]
    }

    private let directory: URL
    /// The epoch of the last removal: a write or read from before it does nothing.
    private var removed: UInt64 = 0

    init(directory: URL) {
        self.directory = directory
    }

    private func url(_ key: StatsKeptResults.Key) -> URL {
        let scope: String = switch key.scope {
        case .allDecks: "all"
        case let .deck(id): "deck-\(id)"
        case .search: "search"
        }
        let history = key.history == .allHistory ? "allHistory" : "last12Months"
        return directory.appending(path: "\(scope)-\(history).plist", directoryHint: .notDirectory)
    }

    func save(_ sets: [StatsPartSet], key: StatsKeptResults.Key, collection: String, epoch: UInt64) {
        guard epoch >= removed else { return }
        if case .search = key.scope { return }
        let encoder = PropertyListEncoder()
        encoder.outputFormat = .binary
        guard let data = try? encoder.encode(File(format: File.format, collection: collection, sets: sets)) else { return }
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try? data.write(to: url(key), options: [.atomic])
    }

    func load(key: StatsKeptResults.Key, collection: String, epoch: UInt64) -> [StatsPartSet]? {
        guard epoch >= removed else { return nil }
        let url = url(key)
        guard let data = try? Data(contentsOf: url) else { return nil }
        guard let file = try? PropertyListDecoder().decode(File.self, from: data),
              file.format == File.format, file.collection == collection else {
            // Another collection's, another format's or a damaged file: never shown.
            try? FileManager.default.removeItem(at: url)
            return nil
        }
        return file.sets.filter { $0.scope == key.scope && $0.history == key.history && $0.header != nil }
    }

    func removeAll(epoch: UInt64) {
        removed = max(removed, epoch)
        try? FileManager.default.removeItem(at: directory)
    }
}
