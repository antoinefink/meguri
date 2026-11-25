import Foundation
import Instrumentation
internal import MeguriCoreBindings

public nonisolated struct MiningAttachment: Sendable {
    public let name: String
    public let bytes: Data
    public init(name: String, bytes: Data) { self.name = name; self.bytes = bytes }
}

public actor CoreMiningService {
    public init() {}
    public func add(_ request: AddScreenNoteRequest, retryKey: String, payloadDigest: String,
                    attachments: [MiningAttachment]) async throws(BridgeError) -> NoteMutation {
        try await InteractiveLane.shared.call(.addMinedNote) {
            NoteMutation(try MeguriCoreBindings.addMinedNote(request: request.raw, retryKey: retryKey,
                payloadDigest: payloadDigest,
                attachments: attachments.map { .init(name: $0.name, bytes: $0.bytes) }))
        }
    }
}
