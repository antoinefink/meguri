import Instrumentation
internal import MeguriCoreBindings

public struct AddDefaults: Sendable, Hashable {
    public let deckId: Int64
    public let notetypeId: Int64

    public init(deckId: Int64, notetypeId: Int64) {
        self.deckId = deckId
        self.notetypeId = notetypeId
    }

    init(_ value: MeguriCoreBindings.AddDefaults) {
        self.init(deckId: value.deckId, notetypeId: value.notetypeId)
    }
}

public struct NotetypeLastDeck: Sendable, Hashable {
    public let deckId: Int64?

    public init(deckId: Int64?) {
        self.deckId = deckId
    }

    init(_ value: MeguriCoreBindings.NotetypeLastDeck) {
        self.init(deckId: value.deckId)
    }
}

public protocol EditorServices: Sendable {
    func addDefaults(reviewCard: Int64?, presetDeck: Int64?) async throws(BridgeError) -> AddDefaults
    func notetypeLastDeck(notetypeId: Int64) async throws(BridgeError) -> NotetypeLastDeck
    func tagComplete(input: String, limit: UInt64, cancel: CancelToken) async throws(BridgeError) -> [String]
}

public actor CoreEditorServices: EditorServices {
    public init() {}

    /// D1: pass the currently displayed review card, and a deck entry point's
    /// preset when present. Both returned identities are persisted identities.
    public func addDefaults(reviewCard: Int64? = nil, presetDeck: Int64? = nil) async throws(BridgeError) -> AddDefaults {
        try await QueryLane.shared.call(.addDefaults) {
            AddDefaults(try MeguriCoreBindings.addDefaults(reviewCard: reviewCard, presetDeck: presetDeck))
        }
    }

    /// D2: a missing result keeps the editor's selection. The caller applies
    /// this only when its change-deck-by-note-type preference is enabled.
    public func notetypeLastDeck(notetypeId: Int64) async throws(BridgeError) -> NotetypeLastDeck {
        try await QueryLane.shared.call(.notetypeLastDeck) {
            NotetypeLastDeck(try MeguriCoreBindings.notetypeLastDeck(notetypeId: notetypeId))
        }
    }

    /// C1–C4: pass the query text unchanged. Cancel a superseded request
    /// through its explicit token; cancellation never returns a partial list.
    public func tagComplete(input: String, limit: UInt64 = 500, cancel: CancelToken) async throws(BridgeError) -> [String] {
        try await QueryLane.shared.call(.tagComplete) {
            try MeguriCoreBindings.tagComplete(input: input, limit: limit, cancel: cancel.raw)
        }
    }
}
