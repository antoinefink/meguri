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
    @ObservationIgnored private var persistence: [Key: Persistence] = [:]
    @ObservationIgnored private var diskReset: Task<Void, Never>?

    /// Only scalar metadata for at most `perKey` sets; never compares the part records.
    nonisolated private struct SavedSet: Equatable {
        let version: StatsSetVersion
        let complete: Bool
    }

    private struct SaveRequest {
        let sets: [StatsPartSet]
        let versions: [SavedSet]
        let collection: String
        let epoch: UInt64
    }

    private final class Persistence {
        var versions: [SavedSet] = []
        var saved: [SavedSet]?
        var active: [SavedSet]?
        var requested: SaveRequest?
        var task: Task<Void, Never>?
    }

    // Suspension seams for deterministic production-path tests; no production work counters.
    @ObservationIgnored var beforeSave: (@MainActor () async -> Void)?
    @ObservationIgnored var afterSave: (@MainActor () async -> Void)?
    var pendingWriteCount: Int { persistence.values.filter { $0.task != nil }.count }
    var persistenceKeyCount: Int { persistence.count }
    /// Every caller waiting for a running read (each window that asked), run when it ends.
    @ObservationIgnored private var waiters: [Key: [@MainActor () -> Void]] = [:]
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

    /// The scope each open page shows (one per window), which a memory warning keeps.
    @ObservationIgnored private var shown: [ObjectIdentifier: StatsPageScope] = [:]

    func show(_ scope: StatsPageScope?, by page: ObjectIdentifier) {
        shown[page] = scope
    }

    /// A memory warning: keeps only the sets of the scopes the open pages show.
    public func trim() {
        let keep = Set(shown.values)
        for key in Array(entries.keys) where !keep.contains(key.scope) { evict(key) }
        // A read still running for a scope no page shows would bring its sets back.
        for key in Array(loading.keys) where !keep.contains(key.scope) {
            loading[key]?.cancel()
            loading[key] = nil
            waiters[key] = nil
        }
        recent.removeAll { !keep.contains($0.scope) }
    }

    private func drop(disk: Bool) {
        for task in loading.values { task.cancel() }
        // Cancel and fence the active save, discarding queued bytes. New-epoch requests
        // reuse its mailbox until it retires; repeated drops create no predecessors.
        for (key, state) in persistence {
            state.task?.cancel()
            state.versions = []
            state.saved = nil
            state.active = nil
            state.requested = nil
            if state.task == nil { persistence[key] = nil }
        }
        loading = [:]
        waiters = [:]
        entries = [:]
        recent = []
        epoch &+= 1
        if disk, let store = self.disk {
            let epoch = epoch
            let previous = diskReset
            diskReset = Task {
                await previous?.value
                await store.removeAll(epoch: epoch)
            }
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
        let retained = Array(sets.prefix(Self.perKey))
        let versions = retained.map { SavedSet(version: StatsSetVersion($0), complete: $0.complete) }
        let state = persistence[key] ?? Persistence()
        // Preserve the resident array storage for unchanged versions. Queued requests share
        // that copy-on-write storage instead of rebuilding or comparing large part records.
        if entries[key] == nil || state.versions != versions {
            entries[key] = retained
            state.versions = versions
        }
        persistence[key] = state
        touch(key)
    }

    /// Writes the kept sets of `scope` to the device, off the main actor.
    func persist(_ scope: StatsPageScope) {
        guard let disk, let collection else { return }
        if case .search = scope { return }
        for history in [StatsHistory.last12Months, .allHistory] {
            let key = Key(scope: scope, history: history)
            guard let sets = entries[key], !sets.isEmpty, let state = persistence[key] else { continue }
            if state.requested?.versions == state.versions { continue }
            if state.active == state.versions {
                // A -> B -> A while A is writing: B is no longer wanted.
                state.requested = nil
                continue
            }
            if state.task == nil, state.saved == state.versions { continue }
            // At most one replaceable request, sharing entries' array until eviction.
            state.requested = SaveRequest(sets: sets, versions: state.versions, collection: collection, epoch: epoch)
            if state.task == nil { startDrain(state, key: key, disk: disk) }
        }
    }

    private func startDrain(_ state: Persistence, key: Key, disk: StatsKeptDisk) {
        state.task = Task { [weak self] in
            await self?.drain(state, key: key, disk: disk)
        }
    }

    private func drain(_ state: Persistence, key: Key, disk: StatsKeptDisk) async {
        defer {
            state.task = nil
            if state.requested != nil { startDrain(state, key: key, disk: disk) }
            else if entries[key] == nil { persistence[key] = nil }
        }
        while !Task.isCancelled, let request = state.requested {
            state.requested = nil
            guard request.epoch == epoch, request.collection == collection,
                  state.saved != request.versions else { continue }
            state.active = request.versions
            await diskReset?.value
            await beforeSave?()
            if !Task.isCancelled, request.epoch == epoch, request.collection == collection {
                let success = await disk.save(request.sets, key: key, collection: request.collection, epoch: request.epoch)
                await afterSave?()
                if success, request.epoch == epoch, request.collection == collection {
                    state.saved = request.versions
                }
            }
            state.active = nil
            // Failure retires normally; only another explicit request continues the loop.
        }
    }

    /// Whether a read of `scope`'s kept sets from the device is still running.
    func isLoading(_ scope: StatsPageScope) -> Bool {
        loading.keys.contains { $0.scope == scope }
    }

    /// Reads the kept sets of `scope` from the device into memory, off the main actor, unless
    /// they are already there; `loaded` runs on the main actor when each read ends.
    func prepare(_ scope: StatsPageScope, loaded: @escaping @MainActor () -> Void) {
        guard let disk, let collection else { return }
        for history in [StatsHistory.last12Months, .allHistory] {
            let key = Key(scope: scope, history: history)
            guard entries[key] == nil else { continue }
            waiters[key, default: []].append(loaded)
            guard loading[key] == nil else { continue }
            let epoch = epoch
            let reset = diskReset
            loading[key] = Task { [weak self] in
                await reset?.value
                let sets = await disk.load(key: key, collection: collection, epoch: epoch)
                guard let self, !Task.isCancelled, self.epoch == epoch, self.collection == collection else { return }
                self.loading[key] = nil
                let wasEmpty = self.entries[key] == nil
                // A set kept meanwhile (this launch's refresh) is newer; the read ones join it.
                for set in (sets ?? []).reversed() where set.scope == scope && set.history == history { self.keep(set) }
                // Loaded bytes are already persisted. Absence stays uncached so another
                // window/process can produce that history before the next prepare.
                if wasEmpty, self.entries[key] != nil, self.persistence[key]?.task == nil {
                    self.persistence[key]?.saved = self.persistence[key]?.versions
                }
                for waiter in self.waiters.removeValue(forKey: key) ?? [] { waiter() }
            }
        }
    }

    /// Waits for the reads `prepare` started (tests).
    func waitForLoads() async {
        for task in Array(loading.values) { await task.value }
    }

    /// Waits for the writes `persist` started (tests).
    func waitForWrites() async {
        for state in Array(persistence.values) {
            // A cancelled epoch's drain can hand its mailbox to a fresh task on retirement.
            while let task = state.task { await task.value }
        }
        await diskReset?.value
    }

    private func touch(_ key: Key) {
        recent.removeAll { $0 == key }
        recent.append(key)
        while recent.count > Self.keysInMemory {
            let old = recent.removeFirst()
            evict(old)
        }
    }

    private func evict(_ key: Key) {
        entries[key] = nil
        // A requested snapshot must survive eviction until this key's drain handles it.
        if persistence[key]?.task == nil { persistence[key] = nil }
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

    func save(_ sets: [StatsPartSet], key: StatsKeptResults.Key, collection: String, epoch: UInt64) -> Bool {
        guard !Task.isCancelled, epoch >= removed else { return false }
        if case .search = key.scope { return false }
        let encoder = PropertyListEncoder()
        encoder.outputFormat = .binary
        do {
            let data = try encoder.encode(File(format: File.format, collection: collection, sets: sets))
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            try data.write(to: url(key), options: [.atomic])
            return true
        } catch {
            return false
        }
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
