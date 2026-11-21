internal import MeguriCoreBindings
import Instrumentation

/// The Question cell (`browser/columns` C10), of an excerpt or of a Browse
/// row.
public enum ExcerptQuestion: Sendable, Hashable {
    case text(String)
    case missingCloze(kind: CardBlankKind, text: String)
    case templateError(CardTemplateError, side: CardRenderSide)

    init(_ question: MeguriCoreBindings.ExcerptQuestion) {
        self = switch question {
        case .text(let text): .text(text)
        case .missingCloze(let kind, let text): .missingCloze(kind: CardBlankKind(kind), text: text)
        case .templateError(let kind, let side): .templateError(RenderedCard.error(kind), side: CardRenderSide(side))
        }
    }
}

extension CardBlankKind {
    init(_ kind: MeguriCoreBindings.RenderBlank) {
        self = kind == .missingCloze ? .missingCloze : .emptyFront
    }
}

extension CardRenderSide {
    init(_ side: MeguriCoreBindings.RenderSide) {
        self = side == .question ? .question : .answer
    }
}
public enum CardExcerpt: Sendable, Hashable {
    case found(cardID: Int64, question: ExcerptQuestion, deck: String)
    case missing(cardID: Int64)
    case damaged(cardID: Int64)

    init(_ value: MeguriCoreBindings.CardExcerpt) {
        switch value {
        case .missing(let id): self = .missing(cardID: id)
        case .damaged(let id): self = .damaged(cardID: id)
        case .found(let id, let question, let deck):
            self = .found(cardID: id, question: ExcerptQuestion(question), deck: deck)
        }
    }
}

extension Core {
    /// A single query-lane call for the entire ordered list, including an empty list.
    public static func cardExcerpts(_ ids: [Int64]) async throws(BridgeError) -> [CardExcerpt] {
        try await cardExcerpts(ids, using: { try MeguriCoreBindings.cardExcerpts(cardIds: $0).map(CardExcerpt.init) })
    }

    // The same lane boundary used by the product, injectable for call-count and queue witnesses.
    static func cardExcerpts(
        _ ids: [Int64], using provider: @Sendable ([Int64]) throws -> [CardExcerpt]
    ) async throws(BridgeError) -> [CardExcerpt] {
        try await QueryLane.shared.call(.cardExcerpts) {
            guard !Task.isCancelled else { throw BridgeError.cancelled }
            let result = try provider(ids)
            guard !Task.isCancelled else { throw BridgeError.cancelled }
            return result
        }
    }
}
