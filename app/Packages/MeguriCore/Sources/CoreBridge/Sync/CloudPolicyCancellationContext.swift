import Foundation
import Synchronization

/// A request is shared before task cancellation; each native participant captures
/// its own immutable observation when its cancellation handler actually runs.
public final class CloudPolicyCancellationContext: Sendable {
    @TaskLocal public static var current: CloudPolicyCancellationContext?
    public enum Trigger: Sendable { case expiry, replacement, shutdown, other }
    struct RequestStamp: Sendable, Equatable {
        enum Source: Sendable, Equatable { case expiry, replacement, shutdown, other, deadline }
        let sequence: UUID
        let source: Source
        var allowsDependentCompletion: Bool {
            switch source {
            case .expiry, .replacement, .deadline: true
            case .shutdown, .other: false
            }
        }
    }
    struct Snapshot: Sendable {
        let ownerID: UUID
        let request: RequestStamp?
        func sharesEligibleRequest(with predecessor: Snapshot) -> Bool {
            guard let request, request.allowsDependentCompletion else { return false }
            return request == predecessor.request
        }
    }
    private struct Storage {
        var pending: RequestStamp?
        var captured: Snapshot?
    }
    private let storage = Mutex(Storage())
    let id = UUID()
    public init() {}
    public func record(_ trigger: Trigger, sequence: UUID) {
        let source: RequestStamp.Source
        switch trigger {
        case .expiry: source = .expiry
        case .replacement: source = .replacement
        case .shutdown: source = .shutdown
        case .other: source = .other
        }
        storage.withLock { value in
            guard value.pending == nil else { return }
            value.pending = RequestStamp(sequence: sequence, source: source)
        }
    }
    // Only the retained bridge may forward an already recorded original request.
    // A previous capture without a request stays unowned.
    func captureRequestedCancellation() -> Snapshot? {
        storage.withLock { value in
            if let captured = value.captured {
                guard let request = captured.request, request.source != .other else { return nil }
                return captured
            }
            guard let request = value.pending, request.source != .other else { return nil }
            let captured = Snapshot(ownerID: id, request: request)
            value.captured = captured
            return captured
        }
    }
    // Account output cannot claim a wait already cancelled by its original owner.
    func beforeCancellation(_ operation: () -> Void) {
        storage.withLock { value in
            guard value.pending == nil, value.captured == nil else { return }
            operation()
        }
    }
    /// Capture before any actor hop or revocation wait. A previous observation,
    /// including one without request metadata, is never revised by later work.
    func capture(predecessor: Snapshot? = nil, deadlineReached: Bool = false) -> Snapshot {
        storage.withLock { value in
            if let captured = value.captured { return captured }
            let selected: RequestStamp?
            switch value.pending?.source {
            case .some(.shutdown), .some(.other): selected = value.pending
            default:
                if let predecessor { selected = predecessor.request }
                else if let pending = value.pending { selected = pending }
                else if deadlineReached { selected = RequestStamp(sequence: UUID(), source: .deadline) }
                else { selected = nil }
            }
            let captured = Snapshot(ownerID: id, request: selected)
            value.captured = captured
            return captured
        }
    }
}
