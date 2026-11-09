#if DEBUG
import Foundation
import Instrumentation
internal import MeguriCoreBindings

public struct OriginalCloudRoleFacts: Sendable {
    public let noteDigest: Data
    public let baselineDigest: Data
    public let fieldCount: UInt32
    public let templateCount: UInt32
    public let cardCount: UInt32
    public let deckCount: UInt32
    public let qOrdinal: UInt32
    // Source hashes are private observations, never expected verdict inputs.
    internal let mainSHA256: Data
    internal let walSHA256: Data?
    internal init(_ r: MeguriCoreBindings.CloudOriginalRoleFacts) {
        noteDigest = r.noteDigest; baselineDigest = r.baselineDigest
        fieldCount = r.fieldCount; templateCount = r.templateCount
        cardCount = r.cardCount; deckCount = r.deckCount; qOrdinal = r.qOrdinal
        mainSHA256 = r.sourceMainSha256; walSHA256 = r.sourceWalSha256
    }
}
public struct OriginalCloudOwnedFacts: Sendable {
    public let generation: UInt64
    public let seq: UInt64
    public let noteDigest: Data
    public let qChanged: Bool
}

// The caller retains verified-source authority through each await. Rust retains
// the full canonical baseline and all native identities behind this handle.
public actor OriginalCloudRoleInspection: CustomStringConvertible, CustomDebugStringConvertible {
    private let raw: MeguriCoreBindings.CloudOriginalRole
    public nonisolated let facts: OriginalCloudRoleFacts
    private init(raw: MeguriCoreBindings.CloudOriginalRole, facts: OriginalCloudRoleFacts) {
        self.raw = raw; self.facts = facts
    }
    public nonisolated var description: String { "OriginalCloudRoleInspection" }
    public nonisolated var debugDescription: String { description }
    public static func inspect(main: Data, wal: Data?) async throws -> OriginalCloudRoleInspection {
        let raw = try await QueryLane.shared.call(.testCloudOriginalRole) {
            try MeguriCoreBindings.testCloudOriginalRole(main: main, wal: wal)
        }
        let facts = try await QueryLane.shared.call(.cloudOriginalRoleFacts) {
            try OriginalCloudRoleFacts(raw.facts())
        }
        return OriginalCloudRoleInspection(raw: raw, facts: facts)
    }
    public func validateOwned() async throws -> OriginalCloudOwnedFacts {
        let raw = raw
        return try await QueryLane.shared.call(.cloudOriginalRoleValidateOwned) {
            let r = try raw.validateOwned()
            return OriginalCloudOwnedFacts(generation: r.generation, seq: r.seq, noteDigest: r.noteDigest, qChanged: r.qChanged)
        }
    }
}
#endif
