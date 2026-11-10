#if DEBUG && MEGURI_TEST_HOOKS
import CoreBridge
import Foundation

// J3.2 private role preparation. This capability survives adoption/edit, while
// the original descriptor chain stays authoritative for every inspection.
@MainActor final class CloudKitSelectedOriginalRole: CustomStringConvertible, CustomDebugStringConvertible {
    private let source: AgentGeneratedBase.VerifiedOriginalSnapshot
    private let reader: OriginalCloudRoleInspection
    private init(source: AgentGeneratedBase.VerifiedOriginalSnapshot, reader: OriginalCloudRoleInspection) {
        self.source = source; self.reader = reader
    }
    nonisolated var description: String { "CloudKitSelectedOriginalRole" }
    nonisolated var debugDescription: String { description }
    static func prepare(base: AgentGeneratedBase) async throws -> CloudKitSelectedOriginalRole {
        let source = try base.verifiedOriginalSnapshot()
        let reader = try await source.inspect { main, wal in
            try await OriginalCloudRoleInspection.inspect(main: main, wal: wal)
        }
        return CloudKitSelectedOriginalRole(source: source, reader: reader)
    }
    func revalidate() throws { try source.revalidate() }
    func observe(generation: UInt64) async throws -> OriginalCloudObservation {
        try source.revalidate()
        do {
            let result = try await reader.observe()
            try source.revalidate()
            guard result.generation == generation else { throw AgentFailure.execution }
            return result
        } catch {
            try source.revalidate()
            throw error
        }
    }
    func fixedEdit(generation: UInt64) async throws -> OriginalCloudEditFacts {
        try source.revalidate()
        do {
            let result = try await reader.fixedEdit()
            try source.revalidate()
            guard result.generation == generation else { throw AgentFailure.execution }
            return result
        } catch {
            try source.revalidate()
            throw error
        }
    }
    func validateOwned(generation: UInt64) async throws -> OriginalCloudOwnedFacts {
        try source.revalidate()
        do {
            let result = try await reader.validateOwned()
            try source.revalidate()
            guard result.generation == generation else { throw AgentFailure.execution }
            return result
        } catch {
            try source.revalidate()
            throw error
        }
    }
}
#endif
