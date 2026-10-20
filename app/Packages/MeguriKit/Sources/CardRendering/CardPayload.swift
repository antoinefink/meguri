import CoreBridge
import Foundation

/// Native identity, restricted to integers JavaScript can carry exactly.
public nonisolated struct CardUpdate: Sendable, Hashable {
    public let card: Int64
    public let generation: UInt64
    public let side: CardRenderSide

    public init(card: Int64, generation: UInt64, side: CardRenderSide) {
        precondition(card >= -9_007_199_254_740_991 && card <= 9_007_199_254_740_991 && generation <= 9_007_199_254_740_991)
        self.card = card
        self.generation = generation
        self.side = side
    }

    public var sideName: String { side == .question ? "question" : "answer" }
}

/// Render output plus session policy; no second renderer or AV extraction.
public nonisolated struct CardPayload: Sendable {
    public let cardID: Int64
    public let ordinal: Int
    /// Native template label for the future Template Errors view.
    public let templateDescriptor: String?
    public let question: String
    public let answer: String
    public let questionAudio: [CardAudio]
    public let answerAudio: [CardAudio]
    public let typeIn: TypeAnswerDescriptor?
    public var flag: UInt8
    public var marked: Bool
    public var audioPolicy: CardAudioPolicy

    public init(_ rendered: RenderedCard, audioPolicy: CardAudioPolicy, templateDescriptor: String? = nil) {
        self.init(cardID: rendered.cardID, ordinal: Int(rendered.ordinal), templateDescriptor: templateDescriptor,
                  question: rendered.question ?? "", answer: rendered.answer ?? "",
                  questionAudio: rendered.questionAudio, answerAudio: rendered.answerAudio,
                  typeIn: rendered.typeIn, flag: rendered.flag, marked: rendered.marked, audioPolicy: audioPolicy)
    }

    /// Also permits synthetic render output and previews without opening a core.
    public init(cardID: Int64, ordinal: Int = 0, templateDescriptor: String? = nil, question: String, answer: String,
                questionAudio: [CardAudio] = [], answerAudio: [CardAudio] = [],
                typeIn: TypeAnswerDescriptor? = nil, flag: UInt8 = 0, marked: Bool = false,
                audioPolicy: CardAudioPolicy = .init()) {
        precondition(cardID >= -9_007_199_254_740_991 && cardID <= 9_007_199_254_740_991 && ordinal >= 0 && ordinal < Int.max)
        self.cardID = cardID
        self.ordinal = ordinal
        self.templateDescriptor = templateDescriptor.map { String($0.prefix(240)) }
        self.question = question
        self.answer = answer
        self.questionAudio = questionAudio
        self.answerAudio = answerAudio
        self.typeIn = typeIn
        self.flag = flag
        self.marked = marked
        self.audioPolicy = audioPolicy
    }

    public func audio(on side: CardRenderSide) -> [CardAudio] { side == .question ? questionAudio : answerAudio }
}
