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
    public let typeIn: TypeAnswerDescriptor?

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
        typeIn = value.typeIn.map(TypeAnswerDescriptor.init)
    }
    static func error(_ kind: MeguriCoreBindings.RenderErrorKind) -> CardTemplateError {
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


public enum TypeAnswerMode: Sendable, Hashable { case plain, nc }
/// Raw expected text stays separate from the displayed answer. Comparison
/// returns C for insertion in every answer placeholder at reveal.
public struct TypeAnswerDescriptor: Sendable, Hashable {
    public static let placeholderHTML = "<span data-meguri-type-answer></span>"
    public let expected: String
    public let mode: TypeAnswerMode
    public let caseInsensitive: Bool
    public let font: String
    public let size: UInt32

    public init(expected: String, mode: TypeAnswerMode = .plain, caseInsensitive: Bool = false, font: String = "Arial", size: UInt32 = 20) {
        self.expected = expected
        self.mode = mode
        self.caseInsensitive = caseInsensitive
        self.font = font
        self.size = size
    }
    init(_ value: MeguriCoreBindings.TypeAnswerDescriptor) {
        self.init(expected: value.expected, mode: value.mode == .plain ? .plain : .nc,
                  caseInsensitive: value.caseInsensitive, font: value.font, size: value.size)
    }
    var raw: MeguriCoreBindings.TypeAnswerDescriptor {
        .init(expected: expected, mode: mode == .plain ? .plain : .nc,
              caseInsensitive: caseInsensitive, font: font, size: size)
    }
}

extension Core {
    public static func compareTypedAnswer(_ descriptor: TypeAnswerDescriptor, typed: String) async throws(BridgeError) -> String {
        try await InteractiveLane.shared.call(.compareTypedAnswer) {
            try MeguriCoreBindings.compareTypedAnswer(descriptor: descriptor.raw, typed: typed)
        }
    }
}
