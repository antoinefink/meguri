import CoreBridge
import Observation

/// A native menu retains its names and collection epoch, rather than resolving
/// the current selection when the person chooses Delete Tag (T9, S9.3).
public nonisolated struct BrowseTagDeletionTarget: Sendable, Hashable {
    public let names: [String]
    fileprivate let epoch: UInt64
    fileprivate let generation: UInt64
    fileprivate let expectedGeneration: UInt64?
}

@Observable @MainActor
public final class BrowseTagDeletion {
    public private(set) var busy = false
    public private(set) var failed: BrowseTagDeletionTarget?
    public private(set) var announcement: BrowseAnnouncement?
    @ObservationIgnored private let backend: any BrowseBackend
    @ObservationIgnored private var epoch: UInt64 = 0
    @ObservationIgnored private var serial: UInt64 = 0
    @ObservationIgnored private var task: Task<Void, Never>?
    @ObservationIgnored var generation: @MainActor () -> UInt64 = { 0 }
    @ObservationIgnored var expectedGeneration: @MainActor () -> UInt64? = { nil }
    @ObservationIgnored var stepRecorded: @MainActor (UInt64, String) -> Void = { _, _ in }
    @ObservationIgnored var refreshUndo: @MainActor () async -> Void = {}
    @ObservationIgnored var available: @MainActor () -> Bool = { false }
    @ObservationIgnored var contains: @MainActor (String) -> Bool = { _ in false }
    @ObservationIgnored var saveFirst: @MainActor () async -> Bool = { true }
    @ObservationIgnored var finished: @MainActor (CountMutation, String) async -> Void = { _, _ in }

    init(backend: any BrowseBackend) { self.backend = backend }

    public func target(_ names: [String]) -> BrowseTagDeletionTarget {
        BrowseTagDeletionTarget(names: names, epoch: epoch, generation: generation(), expectedGeneration: expectedGeneration())
    }

    public func canDelete(_ target: BrowseTagDeletionTarget) -> Bool {
        matchesTree(target) && !target.names.isEmpty && target.names.allSatisfy(contains) && available() && !busy
    }

    private func isCurrent(_ target: BrowseTagDeletionTarget) -> Bool {
        target.epoch == epoch && target.generation == generation()
    }

    private func matchesTree(_ target: BrowseTagDeletionTarget) -> Bool {
        isCurrent(target) && target.expectedGeneration != nil && target.expectedGeneration == expectedGeneration()
    }

    public func delete(_ target: BrowseTagDeletionTarget) {
        guard canDelete(target) else { return }
        busy = true
        failed = nil
        announcement = nil
        task = Task {
            defer { if target.epoch == epoch { busy = false; task = nil } }
            guard matchesTree(target), available(), !Task.isCancelled else { return }
            // A global tag delete rewrites notes too: save the inspector's draft
            // first, just as the table's commands do, then recheck its collection.
            guard await saveFirst() else {
                if isCurrent(target) { failed = target }
                return
            }
            guard matchesTree(target), !Task.isCancelled else { return }
            guard available(), target.names.allSatisfy(contains) else { failed = target; return }
            do {
                guard let expectedGeneration = target.expectedGeneration else { return }
                let result = try await backend.deleteTags(text: target.names.joined(separator: " "), expectedGeneration: expectedGeneration)
                // The write's change event can invalidate the tree before this
                // result arrives. Verify its retained scope against the receipt.
                guard isCurrent(target),
                      result.summary.generation == expectedGeneration, available() else { return }
                // A zero-note result can still remove registry rows and record Undo.
                let title = target.names.count == 1 ? "Delete Tag" : "Delete Tags"
                if result.summary.undoName != nil { stepRecorded(result.summary.seq, title) }
                await finished(result, title)
                await refreshUndo()
                guard isCurrent(target), available() else { return }
                serial &+= 1
                announcement = BrowseAnnouncement(kind: .tagsChanged, count: result.count, serial: serial)
            } catch {
                guard isCurrent(target) else { return }
                failed = target
            }
        }
    }

    public func dismissFailure() { failed = nil }
    public func dismissAnnouncement() { announcement = nil }

    func reset() {
        epoch &+= 1
        task?.cancel(); task = nil
        busy = false; failed = nil; announcement = nil
    }

    public func waitForCompletion() async { await task?.value }
}

extension BrowseModel {
    /// Exact tree identity only. Matching tags for deletion belongs to the core,
    /// including simple folding and the `::` boundary (T5).
    func hasTag(_ name: String) -> Bool {
        tagsLoaded && tagIdentities.contains(name)
    }

    static func tagNames(_ nodes: [TagNode]) -> [String] {
        nodes.flatMap { [$0.fullName] + tagNames($0.children) }
    }
}
