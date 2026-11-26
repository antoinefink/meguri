import CoreBridge

/// A read and the engine generation that supplied it, independent of UI epochs.
public nonisolated struct CollectionScopedRead<Value: Sendable>: Sendable {
    public let value: Value
    public let generation: UInt64

    public init(value: Value, generation: UInt64) {
        self.value = value
        self.generation = generation
    }
}

@MainActor enum CollectionGenerationRead {
    /// Generations belong to the process engine and never repeat across host
    /// replacement. Both probes must be Ready: a replacement during any of the
    /// intervening reads discards the entire result, even if names/IDs match.
    static func read<Value: Sendable>(
        _ read: @MainActor () async throws -> Value
    ) async throws -> CollectionScopedRead<Value> {
        let lifecycle: @MainActor () async throws -> CollectionLifecycle = {
            try await CoreCollectionService().lifecycle()
        }
        return try await self.read(lifecycle: lifecycle, read)
    }

    static func read<Value: Sendable>(
        lifecycle: @MainActor () async throws -> CollectionLifecycle,
        _ read: @MainActor () async throws -> Value
    ) async throws -> CollectionScopedRead<Value> {
        guard case .ready(let before) = try await lifecycle(), before != 0 else {
            throw BridgeError.unavailable(reason: "Collection scope is unavailable")
        }
        let value = try await read()
        guard case .ready(let after) = try await lifecycle() else {
            throw BridgeError.unavailable(reason: "Collection scope is unavailable")
        }
        guard before == after else { throw BridgeError.staleGeneration(planned: before, current: after) }
        return .init(value: value, generation: before)
    }
}
