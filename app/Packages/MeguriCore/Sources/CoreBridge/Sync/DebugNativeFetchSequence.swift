#if DEBUG
import Synchronization

extension DebugCloudDiagnostics {
    static let nativeFetchSequence = NativeFetchSequence()
    /// Independently sampled curated values; proximity proves neither causality nor error attribution.
    public static var nativeFetchSequenceValue: String {
        formatNativeFetchSequenceValue(latest: latestValue, sequence: nativeFetchSequence.formatted)
    }

    // Internal composition seam, not a sanitizer. Production uses only the two curated sources above.
    // Cap only the fault fragment: 180 + 8 wrapper bytes + <=306 sequence bytes = 494 < 512.
    static func formatNativeFetchSequenceValue(latest: String?, sequence: String) -> String {
        var fault = ""
        for scalar in (latest ?? "none").unicodeScalars {
            if fault.utf8.count + scalar.utf8.count > 180 { break }
            fault.unicodeScalars.append(scalar)
        }
        return "fault{\(fault)} \(sequence)"
    }

    struct FetchInclusion: Equatable, Sendable {
        var root = false
        var collection = false
        fileprivate var tag: String { "\(root ? 1 : 0)\(collection ? 1 : 0)" }
    }
    enum FetchReason: String, CaseIterable, Sendable { case manual = "m", scheduled = "s" }
    enum NativeFetchOutcome: Equatable, Sendable {
        case success, cancelled
        case failure(CloudEngineErrorCode)


        fileprivate var tag: String {
            switch self {
            case .success: "Osuccess"
            case .cancelled: "Ocancel"
            case .failure(let code):
                switch code {
                case .notAuthenticated: "Oauth"
                case .accountTemporarilyUnavailable: "Oaccount"
                case .networkFailure: "OnetFail"
                case .networkUnavailable: "OnetDown"
                case .requestRateLimited: "Orate"
                case .serviceUnavailable: "Oservice"
                case .zoneBusy: "OzoneBusy"
                case .limitExceeded: "Olimit"
                case .batchRequestFailed: "Obatch"
                case .unknownItem: "Ounknown"
                case .zoneNotFound: "OzoneGone"
                case .encryptedDataReset: "Oreset"
                case .userDeletedZone: "Odeleted"
                case .serverRecordChanged: "Oconflict"
                case .other: "Oother"
                }
            }
        }
    }
    enum NativeFetchEvent: Equatable, Sendable {
        case configured(automatic: Bool, restored: Bool)
        case prepared(scope: FetchInclusion, priority: FetchInclusion)
        case invoke, returned, cancel, release
        case outcome(NativeFetchOutcome)
        case will(reason: FetchReason, sameGroup: Bool)
        case did(reason: FetchReason, sameGroup: Bool)
        case database
        case unrelatedDatabaseDeletion
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
            case .outcome(let outcome): outcome.tag
            case .cancel: "X"
            case .release: "L"
            case .will(let reason, let same): "W\(reason.rawValue)\(same ? 1 : 0)"
            case .did(let reason, let same): "D\(reason.rawValue)\(same ? 1 : 0)"
            case .database: "B"
            case .unrelatedDatabaseDeletion: "U"
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
    /// n counts every recorder append, including configuration, saturating at UInt64.max;
    /// the next attempted append sets sticky e, independently of ordinal overflow o.
    /// The count is observed at render time, not on-demand freshness or causal SDK ordering.
    /// SyncNowButton samples on status-driven render; an identical count does not prove
    /// a stalled native engine if the render did not refresh. Repeated tails can alias.
    /// Format: nf{l=latest a=automatic r=restored o=overflow n=count e=countOverflow|ordinal:tag,...}.
    /// C=configure (automatic/restored), P=prepared (scope/priority), I=invoke,
    /// R=terminal return (including throws), X=cancel entry, L=release entry,
    /// O=original SDK call outcome (success, CancellationError, or fixed error-code tag),
    /// observed before R; success says only that fetchChanges returned without throwing.
    /// W/D=will/did (m=manual,s=scheduled; group object same), B=database,
    /// U=nonempty database deletions accepted as unrelated after validation,
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
            var appendCount: UInt64
            var appendOverflow = false
            var latest: UInt16 = 0
            var automatic = false
            var restored = false
            var overflow = false
            var tail: [Entry] = []
            mutating func append(_ event: NativeFetchEvent, ordinal: UInt16) {
                if appendCount == .max {
                    appendOverflow = true
                } else {
                    appendCount += 1
                }
                if tail.count == 16 { tail.removeFirst() }
                tail.append(Entry(ordinal: ordinal, event: event))
            }
        }
        private let state: Mutex<State>
        // Fresh recorder and near-max scalar seeds for focused tests only.
        init(lastAllocatedOrdinal: UInt16 = 0, appendCount: UInt64 = 0) {
            state = Mutex(State(lastAllocated: lastAllocatedOrdinal, appendCount: appendCount))
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
        // Header <= 50 bytes + 16 entries * 15 bytes + 15 commas + '}' = 306 UTF8 bytes.
        var formatted: String {
            state.withLock { state in
                let tail = state.tail.map { "\($0.ordinal):\($0.event.tag)" }.joined(separator: ",")
                return "nf{l=\(state.latest) a=\(state.automatic ? 1 : 0) r=\(state.restored ? 1 : 0) o=\(state.overflow ? 1 : 0) n=\(state.appendCount) e=\(state.appendOverflow ? 1 : 0)|\(tail)}"
            }
        }
    }
}
#endif
