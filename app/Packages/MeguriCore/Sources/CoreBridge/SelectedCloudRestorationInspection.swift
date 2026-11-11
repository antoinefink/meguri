#if DEBUG
import Foundation
import Instrumentation
internal import MeguriCoreBindings

// Historical journal correlation is an input to inspection, never a verdict or
// authority to execute a command. All native identities stay behind Core.
public struct OriginalCloudControlWitness: Sendable {
    public let generation: UInt64
    public let activeSHA256: Data
    public let epochSHA256: Data
    public let correlation: Data
    public init(generation: UInt64, activeSHA256: Data, epochSHA256: Data, correlation: Data) {
        self.generation = generation; self.activeSHA256 = activeSHA256
        self.epochSHA256 = epochSHA256; self.correlation = correlation
    }
    var raw: MeguriCoreBindings.CloudOriginalControlWitness {
        .init(generation: generation, activeSha256: activeSHA256, epochSha256: epochSHA256, correlation: correlation)
    }
}
// Deliberately separate from SelectedCloudControlObservation: a matched
// original witness is not a new process-local Complete reply.
public struct RestoredCloudControlObservation: Sendable {
    public let generation: UInt64
    public let requiredScanComplete: Bool
}
public struct OriginalCloudExchangeCapture: Sendable {
    public let boundarySeq: UInt64
    public let selectedNames: [String]
}
// Original completion metadata. SDK acknowledgement and recovery proof stay separate.
public struct OriginalCloudExchangeWork: Sendable {
    public let heldPendingNames: [String]
    public let heldAcknowledgedNames: [String]
    public let pendingUnheldNames: [String]
    public let media: CloudPolicyMediaFacts
    public let acknowledgedNames: [String]
    public let cleanNames: [String]
    public let droppedNames: [String]
    public let absentNames: [String]
    public let blockedNames: [String]
    public let outstandingNames: [String]
    public let incoming: [OriginalCloudExchangeIncoming]
    init(heldPendingNames: [String], heldAcknowledgedNames: [String], pendingUnheldNames: [String],
         media: CloudPolicyMediaFacts, acknowledgedNames: [String] = [], cleanNames: [String] = [],
         droppedNames: [String] = [], absentNames: [String] = [], blockedNames: [String] = [],
         outstandingNames: [String] = [], incoming: [OriginalCloudExchangeIncoming] = []) {
        self.heldPendingNames = heldPendingNames; self.heldAcknowledgedNames = heldAcknowledgedNames
        self.pendingUnheldNames = pendingUnheldNames; self.media = media
        self.acknowledgedNames = acknowledgedNames; self.cleanNames = cleanNames; self.droppedNames = droppedNames
        self.absentNames = absentNames; self.blockedNames = blockedNames; self.outstandingNames = outstandingNames
        self.incoming = incoming
    }
}
public struct OriginalCloudExchangeIncoming: Sendable {
    public let recordName: String
    public let reason: OriginalCloudIncomingReason
}
public enum OriginalCloudIncomingReason: String, Sendable {
    case ready, generation, reference, structure, format, asset
    case applyFailed = "apply-failed"
    init(_ reason: MeguriCoreBindings.CloudOriginalIncomingReason) {
        self = switch reason {
        case .ready: .ready
        case .generation: .generation
        case .reference: .reference
        case .structure: .structure
        case .format: .format
        case .asset: .asset
        case .applyFailed: .applyFailed
        }
    }
}
public actor SelectedCloudRestorationInspection {
    public init() {}
    public func originalControlWitness() async throws -> OriginalCloudControlWitness {
        try await QueryLane.shared.call(.testCloudOriginalControlWitness) {
            let r = try MeguriCoreBindings.testCloudOriginalControlWitness()
            return .init(generation: r.generation, activeSHA256: r.activeSha256,
                         epochSHA256: r.epochSha256, correlation: r.correlation)
        }
    }
    public func control(original: OriginalCloudControlWitness) async throws -> RestoredCloudControlObservation {
        // Ordered current-provider verification in Core precedes owned reads.
        try await QueryLane.shared.call(.testCloudRestoredControl) {
            let r = try MeguriCoreBindings.testCloudRestoredControl(original: original.raw)
            return .init(generation: r.generation, requiredScanComplete: r.requiredScanComplete)
        }
    }
    public func originalExchangeWitness(command: CloudPolicyCommand) async throws -> Data {
        try await QueryLane.shared.call(.testCloudOriginalExchangeWitness) {
            try MeguriCoreBindings.testCloudOriginalExchangeWitness(command: command.raw)
        }
    }
    public func originalExchangeCapture(command: CloudPolicyCommand, runtimeExchange: UInt64) async throws -> OriginalCloudExchangeCapture {
        try await QueryLane.shared.call(.testCloudOriginalExchangeCapture) {
            let captured = try MeguriCoreBindings.testCloudOriginalExchangeCapture(command: command.raw,
                                                                                  runtimeExchange: runtimeExchange)
            return .init(boundarySeq: captured.boundarySeq, selectedNames: captured.selectedNames)
        }
    }
    public func originalExchangeWork(command: CloudPolicyCommand) async throws -> OriginalCloudExchangeWork {
        try await QueryLane.shared.call(.testCloudOriginalExchangeWork) {
            let work = try MeguriCoreBindings.testCloudOriginalExchangeWork(command: command.raw)
            return .init(heldPendingNames: work.heldPendingNames, heldAcknowledgedNames: work.heldAckedNames,
                         pendingUnheldNames: work.pendingUnheldNames, media: .init(work.media),
                         acknowledgedNames: work.ackedNames, cleanNames: work.cleanNames, droppedNames: work.droppedNames,
                         absentNames: work.absentNames, blockedNames: work.blockedNames, outstandingNames: work.outstandingNames,
                         incoming: work.incoming.map { .init(recordName: $0.recordName, reason: .init($0.reason)) })
        }
    }
    public func readOriginalExchange(exchange: UInt64, witness: Data) async throws -> CloudPolicyExchangeRecord {
        try await QueryLane.shared.call(.testCloudReadOriginalExchange) {
            try CloudPolicyExchangeRecord(MeguriCoreBindings.testCloudReadOriginalExchange(exchange: exchange, witness: witness))
        }
    }
    public func crosscheckOriginalAcknowledgements(exchange: UInt64, witness: Data, envelopes: [[Data]]) async throws -> UInt64 {
        try await QueryLane.shared.call(.testCloudCrosscheckOriginalAckSet) {
            try MeguriCoreBindings.testCloudCrosscheckOriginalAckSet(exchange: exchange, witness: witness, envelopes: envelopes)
        }
    }
}
#endif
