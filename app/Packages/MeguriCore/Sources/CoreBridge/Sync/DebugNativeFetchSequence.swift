#if DEBUG
import Synchronization

extension DebugCloudDiagnostics {
    static let nativeFetchSequence = NativeFetchSequence()
    public static var nativeFetchSequenceValue: String { nativeFetchSequence.formatted }

    struct FetchInclusion: Equatable, Sendable {
        var root = false
        var collection = false
        fileprivate var tag: String { "\(root ? 1 : 0)\(collection ? 1 : 0)" }
    }
    enum FetchReason: String, CaseIterable, Sendable { case manual = "m", scheduled = "s" }
    enum NativeFetchEvent: Equatable, Sendable {
        case configured(automatic: Bool, restored: Bool)
        case prepared(scope: FetchInclusion, priority: FetchInclusion)
        case invoke, returned, cancel, release
        case will(reason: FetchReason, sameGroup: Bool)
        case did(reason: FetchReason, sameGroup: Bool)
        case database
        case willZone(FetchZone)
        // Original batch's zone classes, including deletions; all false is empty.
        case fetchedZones(root: Bool, collection: Bool, other: Bool)
        case options(reason: FetchReason, requested: FetchInclusion, offered: FetchInclusion,
                     priority: FetchInclusion, sameGroup: Bool)

        fileprivate var tag: String {
            switch self {
            case .configured(let automatic, let restored): "C\(automatic ? 1 : 0)\(restored ? 1 : 0)"
            case .prepared(let scope, let priority): "P\(scope.tag)\(priority.tag)"
            case .invoke: "I"
            case .returned: "R"
            case .cancel: "X"
            case .release: "L"
            case .will(let reason, let same): "W\(reason.rawValue)\(same ? 1 : 0)"
            case .did(let reason, let same): "D\(reason.rawValue)\(same ? 1 : 0)"
            case .database: "B"
            case .willZone(let zone):
                switch zone { case .root: "Zr"; case .collection: "Zc"; case .other: "Zo" }
            case .fetchedZones(let root, let collection, let other): "F\(root ? 1 : 0)\(collection ? 1 : 0)\(other ? 1 : 0)"
            case .options(let reason, let requested, let offered, let priority, let same):
                "N\(reason.rawValue)\(requested.tag)\(offered.tag)\(priority.tag)\(same ? 1 : 0)"
            }
        }
    }

    /// Process-local lock acquisition order, with cross-instance interleaving.
    /// Ordinals identify diagnostic registrations only, never SDK identities.
    /// After 65535, registration returns reserved 0 forever with sticky overflow;
    /// those entries cannot distinguish instances. There is no global reset.
    /// This is neither current-owner nor causal completion, convergence,
    /// selected-leg or signed proof. Group sameness observes object identity only;
    /// it says nothing about SDK group copying. No caller strings or CK objects.
    /// Format: nf{l=latest a=automatic r=restored o=overflow|ordinal:tag,...}.
    /// C=configure (automatic/restored), P=prepared (scope/priority), I=invoke,
    /// R=terminal return (including throws), X=cancel entry, L=release entry,
    /// W/D=will/did (m=manual,s=scheduled; group object same), B=database,
    /// Z=will-zone (r=root,c=registered collection,o=other), F=fetched batch
    /// (root/collection/other presence), N=options callback (reason, requested
    /// scope, offered scope, offered priority, group object same at entry).
    /// Each scope/priority pair is root then collection. Failure offers 00/00.
    final class NativeFetchSequence: Sendable {
        private struct Entry: Sendable {
            let ordinal: UInt16
            let event: NativeFetchEvent
        }
        private struct State: Sendable {
            var lastAllocated: UInt16
            var latest: UInt16 = 0
            var automatic = false
            var restored = false
            var overflow = false
            var tail: [Entry] = []
            mutating func append(_ event: NativeFetchEvent, ordinal: UInt16) {
                if tail.count == 16 { tail.removeFirst() }
                tail.append(Entry(ordinal: ordinal, event: event))
            }
        }
        private let state: Mutex<State>
        // Fresh recorder and near-max scalar seed for focused tests only.
        init(lastAllocatedOrdinal: UInt16 = 0) {
            state = Mutex(State(lastAllocated: lastAllocatedOrdinal))
        }
        func register(automatic: Bool, restored: Bool) -> UInt16 {
            state.withLock { state in
                let ordinal: UInt16
                if state.lastAllocated == .max {
                    ordinal = 0
                    state.overflow = true
                } else {
                    state.lastAllocated += 1
                    ordinal = state.lastAllocated
                }
                state.latest = ordinal
                state.automatic = automatic
                state.restored = restored
                state.append(.configured(automatic: automatic, restored: restored), ordinal: ordinal)
                return ordinal
            }
        }
        func record(_ event: NativeFetchEvent, ordinal: UInt16) {
            state.withLock { $0.append(event, ordinal: ordinal) }
        }
        // Longest entry: 5 ordinal digits + ':' + 9 tag bytes = 15.
        // Header <= 28 bytes + 16 entries * 16 bytes + '}' < 512 UTF8 bytes.
        var formatted: String {
            state.withLock { state in
                let tail = state.tail.map { "\($0.ordinal):\($0.event.tag)" }.joined(separator: ",")
                return "nf{l=\(state.latest) a=\(state.automatic ? 1 : 0) r=\(state.restored ? 1 : 0) o=\(state.overflow ? 1 : 0)|\(tail)}"
            }
        }
    }
}
#endif
