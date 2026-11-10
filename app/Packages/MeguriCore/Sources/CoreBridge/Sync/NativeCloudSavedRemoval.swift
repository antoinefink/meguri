import Foundation

/// Native state exposes IDs, not pending-entry revisions. Keep the application
/// update version observed at issue; any intervening update invalidates an
/// unresolved proof. Read absence before applying a Core removal or re-add.
struct NativeCloudSavedRemoval {
    struct Issue: Sendable {
        let version: UInt64
        let generation: UInt64
        let pending: [String: CloudPendingChange]
    }
    private struct Awaiting {
        let issue: Issue
        let names: Set<String>
    }
    private var version: UInt64 = 0
    private var issued: [UInt64: Issue] = [:]
    private var awaiting: [UInt64: Awaiting] = [:]
    private var ready: [(generation: UInt64, receipt: CloudSavedPendingRemoval)] = []

    func capture(pending: [String: CloudPendingChange], generation: UInt64) -> Issue {
        Issue(version: version, generation: generation, pending: pending)
    }
    mutating func handed(_ request: UInt64, names: Set<String>, issue: Issue) {
        // Retain only the original batch membership, never a whole SDK snapshot.
        issued[request] = Issue(version: issue.version, generation: issue.generation,
                                pending: issue.pending.filter { names.contains($0.key) })
    }
    mutating func sent(_ request: UInt64, names: Set<String>) {
        guard let issue = issued.removeValue(forKey: request), !names.isEmpty,
              names.isSubset(of: Set(issue.pending.keys)) else { return }
        awaiting[request] = Awaiting(issue: issue, names: names)
    }
    mutating func observe(instance: String, pending: [String: CloudPendingChange], generation: UInt64) {
        for (request, candidate) in awaiting {
            guard candidate.issue.version == version, candidate.issue.generation == generation else {
                awaiting.removeValue(forKey: request)
                continue
            }
            if candidate.names.allSatisfy({ pending[$0] == nil }) {
                ready.append((generation, .init(instanceID: instance, requestID: request, names: candidate.names)))
                awaiting.removeValue(forKey: request)
            }
        }
    }
    mutating func applicationWillChangePending() {
        version &+= 1
        // No later name-only absence can establish these original removals.
        awaiting.removeAll()
        issued.removeAll()
    }
    mutating func take(generation: UInt64) -> [CloudSavedPendingRemoval] {
        let result = ready.filter { $0.generation == generation }.map(\.receipt)
        ready.removeAll()
        return result
    }
}
