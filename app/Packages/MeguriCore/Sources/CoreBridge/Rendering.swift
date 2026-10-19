internal import MeguriCoreBindings
import Instrumentation

public enum CardRenderMode: Sendable, Hashable { case review, preview, browser }
public enum CardRenderPlatform: Sendable, Hashable { case mac, ios }
public enum CardAudio: Sendable, Hashable {
    case sound(name: String)
    case tts(language: String, content: String)
}
public enum CardBlankKind: Sendable, Hashable { case emptyFront, missingCloze }
public enum CardTemplateError: Sendable, Hashable {
    case unclosedConditional, unopenedConditional, mismatchedConditional, unknownField, unknownConditional
}
public enum CardRenderSide: Sendable, Hashable { case question, answer }
public enum CardRenderOutcome: Sendable, Hashable {
    case rendered
    case blank(CardBlankKind)
    case templateError(CardTemplateError, side: CardRenderSide)
}
public struct RenderedCard: Sendable, Hashable {
    public let cardID: Int64
    public let ordinal: UInt32
    public let mode: CardRenderMode
    public let question: String?
    public let answer: String?
    public let questionAudio: [CardAudio]
    public let answerAudio: [CardAudio]
    public let outcome: CardRenderOutcome
    public let css: String
    public let hidePlayButtons: Bool
    public let reviewExtra: String
    public let flag: UInt8
    public let marked: Bool
    /// Filled by U64's type-in substitution.
    public let typeIn: String?

    init(_ value: MeguriCoreBindings.RenderedCard) {
        cardID = value.cardId
        ordinal = value.ordinal
        mode = switch value.mode { case .review: .review; case .preview: .preview; case .browser: .browser }
        question = value.question
        answer = value.answer
        func audio(_ tag: MeguriCoreBindings.RenderAv) -> CardAudio {
            switch tag {
            case .sound(let name): .sound(name: name)
            case .tts(let language, let content): .tts(language: language, content: content)
            }
        }
        questionAudio = value.questionAv.map(audio)
        answerAudio = value.answerAv.map(audio)
        outcome = switch value.outcome {
        case .rendered: .rendered
        case .blank(let kind): .blank(kind == .emptyFront ? .emptyFront : .missingCloze)
        case .templateError(let kind, let side): .templateError(Self.error(kind), side: side == .question ? .question : .answer)
        }
        css = value.css
        hidePlayButtons = value.hidePlayButtons
        reviewExtra = value.reviewExtra
        flag = value.flag
        marked = value.marked
        typeIn = value.typeIn
    }
    private static func error(_ kind: MeguriCoreBindings.RenderErrorKind) -> CardTemplateError {
        switch kind {
        case .unclosedConditional: .unclosedConditional
        case .unopenedConditional: .unopenedConditional
        case .mismatchedConditional: .mismatchedConditional
        case .unknownField: .unknownField
        case .unknownConditional: .unknownConditional
        }
    }
}

extension Core {
    /// Renders stored data on the interactive lane. Browser returns raw column
    /// text; review and preview return the host's display HTML.
    public static func renderCard(
        id: Int64, mode: CardRenderMode = .review, platform: CardRenderPlatform
    ) async throws(BridgeError) -> RenderedCard {
        let rawMode: MeguriCoreBindings.RenderMode = switch mode {
        case .review: .review; case .preview: .preview; case .browser: .browser
        }
        let rawPlatform: MeguriCoreBindings.RenderPlatform = platform == .mac ? .mac : .ios
        return try await InteractiveLane.shared.call(.renderCard) {
            RenderedCard(try MeguriCoreBindings.renderCard(cardId: id, mode: rawMode, platform: rawPlatform))
        }
    }
}
