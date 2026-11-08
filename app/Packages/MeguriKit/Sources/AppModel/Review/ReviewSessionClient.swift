import CoreBridge

/// A narrow failure/order seam; scheduling and restoration remain core calls.
public protocol ReviewSessionClient {
    func start(deckID: Int64) async throws -> ReviewStartResult
    #if DEBUG
    func start(deckID: Int64, observation: ReviewStartObservation) async throws -> ReviewStartResult
    #endif
    func head(sessionID: UInt64) async throws -> ReviewHead
    func answer(sessionID: UInt64, item: ReviewHeadItem, rating: ReviewRating, at: Int64, elapsed: UInt64) async throws -> ReviewAnswerOutcome
    func undoStatus(sessionID: UInt64, redo: Bool) async throws -> ReviewUndoStatus
    func restore(sessionID: UInt64, redo: Bool) async throws -> ReviewHead
    func studyState(deckID: Int64) async throws -> ReviewDeckStudyState
    func end(sessionID: UInt64) async throws
}

#if DEBUG
extension ReviewSessionClient {
    public func start(deckID: Int64, observation: ReviewStartObservation) async throws -> ReviewStartResult {
        try await start(deckID: deckID)
    }
}
#endif

public struct CoreReviewSessionClient: ReviewSessionClient {
    private let undo = CoreUndoService()
    private let decks = CoreDeckService()
    public init() {}
    public func start(deckID: Int64) async throws -> ReviewStartResult {
        try await Core.startSessionJob(deckID: deckID).value()
    }
    #if DEBUG
    public func start(deckID: Int64, observation: ReviewStartObservation) async throws -> ReviewStartResult {
        try await Core.startSessionJob(deckID: deckID, observation: observation).value()
    }
    #endif
    public func head(sessionID: UInt64) async throws -> ReviewHead { try await Core.sessionHead(sessionID: sessionID, count: 3) }
    public func answer(sessionID: UInt64, item: ReviewHeadItem, rating: ReviewRating, at: Int64, elapsed: UInt64) async throws -> ReviewAnswerOutcome {
        try await Core.answerSession(sessionID: sessionID, cardID: item.cardId, rating: rating,
                                     stateToken: item.stateToken, answeredAtMs: at, elapsedMs: elapsed)
    }
    public func undoStatus(sessionID: UInt64, redo: Bool) async throws -> ReviewUndoStatus {
        let scope = ReviewUndoScope.session(sessionId: sessionID)
        if redo { return try await undo.peekRedo(scope: scope) }
        return try await undo.peekUndo(scope: scope)
    }
    public func restore(sessionID: UInt64, redo: Bool) async throws -> ReviewHead {
        let scope = ReviewUndoScope.session(sessionId: sessionID)
        let start: CoreUndoStart
        if redo { start = try await undo.startRedo(scope: scope) }
        else { start = try await undo.startUndo(scope: scope) }
        guard case .session(let head) = try await start.value() else { throw BridgeError.internal("Session restore returned a global result") }
        return head
    }
    public func studyState(deckID: Int64) async throws -> ReviewDeckStudyState { try await decks.studyState(deckID: deckID) }
    public func end(sessionID: UInt64) async throws { try await Core.endSessionJob(sessionID: sessionID).value() }
}
