internal import MeguriCoreBindings
import Instrumentation
import Foundation

// Epoch millisecond answer inputs and epoch second head/waiting values remain
// distinct fields. No generated binding type enters the public API.

/// Scheduling identity; the core validates exactly 32 bytes on answer input.
public struct ReviewStateToken: Sendable, Hashable {
    public let bytes: [UInt8]

    public init(bytes: [UInt8]) {
        self.bytes = bytes
    }
    init(_ value: MeguriCoreBindings.ReviewStateToken) {
        bytes = Array(value.bytes)
    }
    var raw: MeguriCoreBindings.ReviewStateToken { .init(bytes: Data(bytes)) }
}

/// Render identity, distinct from a scheduling token and a storage row digest.
public struct ReviewRenderKey: Sendable, Hashable {
    public let bytes: [UInt8]

    public init(bytes: [UInt8]) {
        self.bytes = bytes
    }
    init(_ value: MeguriCoreBindings.ReviewRenderKey) {
        bytes = Array(value.bytes)
    }
    var raw: MeguriCoreBindings.ReviewRenderKey { .init(bytes: Data(bytes)) }
}

public struct ReviewMemoryState: Sendable, Hashable {
    public let stability: Float
    public let difficulty: Float

    public init(stability: Float, difficulty: Float) {
        self.stability = stability
        self.difficulty = difficulty
    }
    init(_ value: MeguriCoreBindings.ReviewMemoryState) {
        stability = value.stability
        difficulty = value.difficulty
    }
}

public struct ReviewLearningState: Sendable, Hashable {
    public let remainingSteps: UInt32
    public let scheduledSecs: UInt32
    public let elapsedSecs: UInt32
    public let memoryState: ReviewMemoryState?

    public init(remainingSteps: UInt32, scheduledSecs: UInt32, elapsedSecs: UInt32, memoryState: ReviewMemoryState?) {
        self.remainingSteps = remainingSteps
        self.scheduledSecs = scheduledSecs
        self.elapsedSecs = elapsedSecs
        self.memoryState = memoryState
    }
    init(_ value: MeguriCoreBindings.ReviewLearningState) {
        remainingSteps = value.remainingSteps
        scheduledSecs = value.scheduledSecs
        elapsedSecs = value.elapsedSecs
        memoryState = value.memoryState.map(ReviewMemoryState.init)
    }
}

public struct ReviewReviewState: Sendable, Hashable {
    public let scheduledDays: UInt32
    public let elapsedDays: UInt32
    public let easeFactor: Float
    public let lapses: UInt32
    public let leeched: Bool
    public let memoryState: ReviewMemoryState?

    public init(scheduledDays: UInt32, elapsedDays: UInt32, easeFactor: Float, lapses: UInt32, leeched: Bool, memoryState: ReviewMemoryState?) {
        self.scheduledDays = scheduledDays
        self.elapsedDays = elapsedDays
        self.easeFactor = easeFactor
        self.lapses = lapses
        self.leeched = leeched
        self.memoryState = memoryState
    }
    init(_ value: MeguriCoreBindings.ReviewReviewState) {
        scheduledDays = value.scheduledDays
        elapsedDays = value.elapsedDays
        easeFactor = value.easeFactor
        lapses = value.lapses
        leeched = value.leeched
        memoryState = value.memoryState.map(ReviewMemoryState.init)
    }
}

public struct ReviewRevlogPreview: Sendable, Hashable {
    public let interval: Int32
    public let lastInterval: Int32
    public let kind: UInt8
    public let factor: UInt32

    public init(interval: Int32, lastInterval: Int32, kind: UInt8, factor: UInt32) {
        self.interval = interval
        self.lastInterval = lastInterval
        self.kind = kind
        self.factor = factor
    }
    init(_ value: MeguriCoreBindings.ReviewRevlogPreview) {
        interval = value.interval
        lastInterval = value.lastInterval
        kind = value.kind
        factor = value.factor
    }
}

public struct ReviewAnswerLabel: Sendable, Hashable {
    public let lessThan: Bool
    public let amount: String
    public let unit: ReviewTimeUnit

    public init(lessThan: Bool, amount: String, unit: ReviewTimeUnit) {
        self.lessThan = lessThan
        self.amount = amount
        self.unit = unit
    }
    init(_ value: MeguriCoreBindings.ReviewAnswerLabel) {
        lessThan = value.lessThan
        amount = value.amount
        unit = ReviewTimeUnit(value.unit)
    }
}

public struct ReviewAnswerState: Sendable, Hashable {
    public let state: ReviewSchedulingState
    public let delay: ReviewDelay
    public let left: UInt32
    public let factor: UInt16
    public let revlog: ReviewRevlogPreview
    public let label: ReviewAnswerLabel

    public init(state: ReviewSchedulingState, delay: ReviewDelay, left: UInt32, factor: UInt16, revlog: ReviewRevlogPreview, label: ReviewAnswerLabel) {
        self.state = state
        self.delay = delay
        self.left = left
        self.factor = factor
        self.revlog = revlog
        self.label = label
    }
    init(_ value: MeguriCoreBindings.ReviewAnswerState) {
        state = ReviewSchedulingState(value.state)
        delay = ReviewDelay(value.delay)
        left = value.left
        factor = value.factor
        revlog = ReviewRevlogPreview(value.revlog)
        label = ReviewAnswerLabel(value.label)
    }
}

public struct ReviewSchedulingStates: Sendable, Hashable {
    public let current: ReviewSchedulingState
    public let again: ReviewAnswerState
    public let hard: ReviewAnswerState
    public let good: ReviewAnswerState
    public let easy: ReviewAnswerState

    public init(current: ReviewSchedulingState, again: ReviewAnswerState, hard: ReviewAnswerState, good: ReviewAnswerState, easy: ReviewAnswerState) {
        self.current = current
        self.again = again
        self.hard = hard
        self.good = good
        self.easy = easy
    }
    init(_ value: MeguriCoreBindings.ReviewSchedulingStates) {
        current = ReviewSchedulingState(value.current)
        again = ReviewAnswerState(value.again)
        hard = ReviewAnswerState(value.hard)
        good = ReviewAnswerState(value.good)
        easy = ReviewAnswerState(value.easy)
    }
}

public struct ReviewPredictions: Sendable, Hashable {
    public let again: Int64?
    public let hard: Int64?
    public let good: Int64?
    public let easy: Int64?

    public init(again: Int64?, hard: Int64?, good: Int64?, easy: Int64?) {
        self.again = again
        self.hard = hard
        self.good = good
        self.easy = easy
    }
    init(_ value: MeguriCoreBindings.ReviewPredictions) {
        again = value.again
        hard = value.hard
        good = value.good
        easy = value.easy
    }
}

public struct ReviewHomePreset: Sendable, Hashable {
    public let id: Int64
    public let autoplay: Bool
    public let skipQuestionWhenReplayingAnswer: Bool
    public let showTimer: Bool
    public let stopTimerOnAnswer: Bool
    public let maximumAnswerSeconds: UInt32
    public let questionSeconds: Float
    public let answerSeconds: Float
    public let questionAction: Int32
    public let answerAction: Int32
    public let waitForAudio: Bool
    public let leechAction: Int32

    public init(id: Int64, autoplay: Bool, skipQuestionWhenReplayingAnswer: Bool, showTimer: Bool, stopTimerOnAnswer: Bool, maximumAnswerSeconds: UInt32, questionSeconds: Float, answerSeconds: Float, questionAction: Int32, answerAction: Int32, waitForAudio: Bool, leechAction: Int32) {
        self.id = id
        self.autoplay = autoplay
        self.skipQuestionWhenReplayingAnswer = skipQuestionWhenReplayingAnswer
        self.showTimer = showTimer
        self.stopTimerOnAnswer = stopTimerOnAnswer
        self.maximumAnswerSeconds = maximumAnswerSeconds
        self.questionSeconds = questionSeconds
        self.answerSeconds = answerSeconds
        self.questionAction = questionAction
        self.answerAction = answerAction
        self.waitForAudio = waitForAudio
        self.leechAction = leechAction
    }
    init(_ value: MeguriCoreBindings.ReviewHomePreset) {
        id = value.id
        autoplay = value.autoplay
        skipQuestionWhenReplayingAnswer = value.skipQuestionWhenReplayingAnswer
        showTimer = value.showTimer
        stopTimerOnAnswer = value.stopTimerOnAnswer
        maximumAnswerSeconds = value.maximumAnswerSeconds
        questionSeconds = value.questionSeconds
        answerSeconds = value.answerSeconds
        questionAction = value.questionAction
        answerAction = value.answerAction
        waitForAudio = value.waitForAudio
        leechAction = value.leechAction
    }
}

public struct ReviewHeadItem: Sendable, Hashable {
    public let cardId: Int64
    public let noteId: Int64
    public let kind: ReviewQueueKind
    public let stateToken: ReviewStateToken
    public let renderKey: ReviewRenderKey
    public let nextStates: ReviewSchedulingStates
    public let predictedNext: ReviewPredictions
    public let flag: UInt8
    public let marked: Bool
    public let homePreset: ReviewHomePreset

    public init(cardId: Int64, noteId: Int64, kind: ReviewQueueKind, stateToken: ReviewStateToken, renderKey: ReviewRenderKey, nextStates: ReviewSchedulingStates, predictedNext: ReviewPredictions, flag: UInt8, marked: Bool, homePreset: ReviewHomePreset) {
        self.cardId = cardId
        self.noteId = noteId
        self.kind = kind
        self.stateToken = stateToken
        self.renderKey = renderKey
        self.nextStates = nextStates
        self.predictedNext = predictedNext
        self.flag = flag
        self.marked = marked
        self.homePreset = homePreset
    }
    init(_ value: MeguriCoreBindings.ReviewHeadItem) {
        cardId = value.cardId
        noteId = value.noteId
        kind = ReviewQueueKind(value.kind)
        stateToken = ReviewStateToken(value.stateToken)
        renderKey = ReviewRenderKey(value.renderKey)
        nextStates = ReviewSchedulingStates(value.nextStates)
        predictedNext = ReviewPredictions(value.predictedNext)
        flag = value.flag
        marked = value.marked
        homePreset = ReviewHomePreset(value.homePreset)
    }
}

public struct ReviewCounts: Sendable, Hashable {
    public let new: UInt32
    public let learning: UInt32
    public let review: UInt32

    public init(new: UInt32, learning: UInt32, review: UInt32) {
        self.new = new
        self.learning = learning
        self.review = review
    }
    init(_ value: MeguriCoreBindings.ReviewCounts) {
        new = value.new
        learning = value.learning
        review = value.review
    }
}

public enum ReviewTerminalState: Sendable, Hashable {
    case active
    case waitingUntil(instant: Int64)
    case finished

    init(_ value: MeguriCoreBindings.ReviewTerminalState) {
        switch value {
        case .active: self = .active
        case .waitingUntil(let instant): self = .waitingUntil(instant: instant)
        case .finished: self = .finished
        }
    }
}

public struct ReviewHead: Sendable, Hashable {
    public let items: [ReviewHeadItem]
    public let counts: ReviewCounts
    public let validUntil: Int64
    public let terminalState: ReviewTerminalState

    public init(items: [ReviewHeadItem], counts: ReviewCounts, validUntil: Int64, terminalState: ReviewTerminalState) {
        self.items = items
        self.counts = counts
        self.validUntil = validUntil
        self.terminalState = terminalState
    }
    init(_ value: MeguriCoreBindings.ReviewHead) {
        items = value.items.map(ReviewHeadItem.init)
        counts = ReviewCounts(value.counts)
        validUntil = value.validUntil
        terminalState = ReviewTerminalState(value.terminalState)
    }
}

public struct ReviewMarks: Sendable, Hashable {
    public let added: Bool
    public let restate: Bool
    public let rerender: Bool
    public let relabel: Bool

    public init(added: Bool, restate: Bool, rerender: Bool, relabel: Bool) {
        self.added = added
        self.restate = restate
        self.rerender = rerender
        self.relabel = relabel
    }
    init(_ value: MeguriCoreBindings.ReviewMarks) {
        added = value.added
        restate = value.restate
        rerender = value.rerender
        relabel = value.relabel
    }
}

public struct ReviewHeadDiff: Sendable, Hashable {
    public let head: ReviewHead
    public let marks: [ReviewMarks]
    public let removed: [Int64]

    public init(head: ReviewHead, marks: [ReviewMarks], removed: [Int64]) {
        self.head = head
        self.marks = marks
        self.removed = removed
    }
    init(_ value: MeguriCoreBindings.ReviewHeadDiff) {
        head = ReviewHead(value.head)
        marks = value.marks.map(ReviewMarks.init)
        removed = value.removed
    }
}

public struct ReviewDifference: Sendable, Hashable {
    public let new: Int64
    public let learning: Int64
    public let review: Int64

    public init(new: Int64, learning: Int64, review: Int64) {
        self.new = new
        self.learning = learning
        self.review = review
    }
    init(_ value: MeguriCoreBindings.ReviewDifference) {
        new = value.new
        learning = value.learning
        review = value.review
    }
}

public struct ReviewDeckStudyState: Sendable, Hashable {
    public let deck: Int64
    public let queueCounts: ReviewCounts
    public let listCounts: ReviewCounts
    public let difference: ReviewDifference
    public let finished: Bool
    public let learningRemainingToday: UInt32
    public let learningLaterToday: UInt32
    public let nextLearningDue: Int64?
    public let nextLearningReadyAt: Int64?
    public let reviewLimitReached: Bool
    public let newLimitReached: Bool
    public let learningRemaining: UInt32
    public let secsUntilNextLearn: Int64
    public let reviewRemaining: Bool
    public let newRemaining: Bool
    public let siblingBuried: UInt32
    public let userBuried: UInt32
    public let tomorrow: ReviewCounts
    public let empty: Bool

    public init(deck: Int64, queueCounts: ReviewCounts, listCounts: ReviewCounts, difference: ReviewDifference, finished: Bool, learningRemainingToday: UInt32, learningLaterToday: UInt32, nextLearningDue: Int64?, nextLearningReadyAt: Int64?, reviewLimitReached: Bool, newLimitReached: Bool, learningRemaining: UInt32, secsUntilNextLearn: Int64, reviewRemaining: Bool, newRemaining: Bool, siblingBuried: UInt32, userBuried: UInt32, tomorrow: ReviewCounts, empty: Bool) {
        self.deck = deck
        self.queueCounts = queueCounts
        self.listCounts = listCounts
        self.difference = difference
        self.finished = finished
        self.learningRemainingToday = learningRemainingToday
        self.learningLaterToday = learningLaterToday
        self.nextLearningDue = nextLearningDue
        self.nextLearningReadyAt = nextLearningReadyAt
        self.reviewLimitReached = reviewLimitReached
        self.newLimitReached = newLimitReached
        self.learningRemaining = learningRemaining
        self.secsUntilNextLearn = secsUntilNextLearn
        self.reviewRemaining = reviewRemaining
        self.newRemaining = newRemaining
        self.siblingBuried = siblingBuried
        self.userBuried = userBuried
        self.tomorrow = tomorrow
        self.empty = empty
    }
    init(_ value: MeguriCoreBindings.ReviewDeckStudyState) {
        deck = value.deck
        queueCounts = ReviewCounts(value.queueCounts)
        listCounts = ReviewCounts(value.listCounts)
        difference = ReviewDifference(value.difference)
        finished = value.finished
        learningRemainingToday = value.learningRemainingToday
        learningLaterToday = value.learningLaterToday
        nextLearningDue = value.nextLearningDue
        nextLearningReadyAt = value.nextLearningReadyAt
        reviewLimitReached = value.reviewLimitReached
        newLimitReached = value.newLimitReached
        learningRemaining = value.learningRemaining
        secsUntilNextLearn = value.secsUntilNextLearn
        reviewRemaining = value.reviewRemaining
        newRemaining = value.newRemaining
        siblingBuried = value.siblingBuried
        userBuried = value.userBuried
        tomorrow = ReviewCounts(value.tomorrow)
        empty = value.empty
    }
}

public struct ReviewDuration: Sendable, Hashable {
    public let seconds: UInt64
    public let nanoseconds: UInt32

    public init(seconds: UInt64, nanoseconds: UInt32) {
        self.seconds = seconds
        self.nanoseconds = nanoseconds
    }
    init(_ value: MeguriCoreBindings.ReviewDuration) {
        seconds = value.seconds
        nanoseconds = value.nanoseconds
    }
}

public struct ReviewStepInfo: Sendable, Hashable {
    public let id: UInt64
    public let name: String
    public let kind: ReviewStepKind
    public let session: UInt64?
    public let longWrite: ReviewDuration?

    public init(id: UInt64, name: String, kind: ReviewStepKind, session: UInt64?, longWrite: ReviewDuration?) {
        self.id = id
        self.name = name
        self.kind = kind
        self.session = session
        self.longWrite = longWrite
    }
    init(_ value: MeguriCoreBindings.ReviewStepInfo) {
        id = value.id
        name = value.name
        kind = ReviewStepKind(value.kind)
        session = value.session
        longWrite = value.longWrite.map(ReviewDuration.init)
    }
}

public enum ReviewSchedulingState: Sendable, Hashable {
    case new(position: UInt32)
    case learning(state: ReviewLearningState)
    case review(state: ReviewReviewState)
    case relearning(learning: ReviewLearningState, review: ReviewReviewState)

    init(_ value: MeguriCoreBindings.ReviewSchedulingState) {
        switch value {
        case .new(let position):
            self = .new(position: position)
        case .learning(let state):
            self = .learning(state: ReviewLearningState(state))
        case .review(let state):
            self = .review(state: ReviewReviewState(state))
        case .relearning(let learning, let review):
            self = .relearning(learning: ReviewLearningState(learning), review: ReviewReviewState(review))
        }
    }
}

public enum ReviewDelay: Sendable, Hashable {
    case seconds(amount: UInt32)
    case days(amount: UInt32)

    init(_ value: MeguriCoreBindings.ReviewDelay) {
        switch value {
        case .seconds(let amount):
            self = .seconds(amount: amount)
        case .days(let amount):
            self = .days(amount: amount)
        }
    }
}

public enum ReviewTimeUnit: Sendable, Hashable {
    case seconds
    case minutes
    case hours
    case days
    case months
    case years
    case end

    init(_ value: MeguriCoreBindings.ReviewTimeUnit) {
        switch value {
        case .seconds:
            self = .seconds
        case .minutes:
            self = .minutes
        case .hours:
            self = .hours
        case .days:
            self = .days
        case .months:
            self = .months
        case .years:
            self = .years
        case .end:
            self = .end
        }
    }
}

public enum ReviewQueueKind: Sendable, Hashable {
    case new
    case review
    case interdayLearning
    case learning

    init(_ value: MeguriCoreBindings.ReviewQueueKind) {
        switch value {
        case .new:
            self = .new
        case .review:
            self = .review
        case .interdayLearning:
            self = .interdayLearning
        case .learning:
            self = .learning
        }
    }
}

public enum ReviewRating: Sendable, Hashable {
    case again
    case hard
    case good
    case easy

    init(_ value: MeguriCoreBindings.ReviewRating) {
        switch value {
        case .again:
            self = .again
        case .hard:
            self = .hard
        case .good:
            self = .good
        case .easy:
            self = .easy
        }
    }
    var raw: MeguriCoreBindings.ReviewRating {
        switch self {
        case .again: .again
        case .hard: .hard
        case .good: .good
        case .easy: .easy
        }
    }
}

public enum ReviewRefusal: Sendable, Hashable {
    case fsrsPolicyPending
    case presetUnsupported(presetIds: [Int64])

    init(_ value: MeguriCoreBindings.ReviewRefusal) {
        switch value {
        case .fsrsPolicyPending:
            self = .fsrsPolicyPending
        case .presetUnsupported(let presetIds):
            self = .presetUnsupported(presetIds: presetIds)
        }
    }
}

public enum ReviewStartResult: Sendable, Hashable {
    case started(sessionId: UInt64, generation: UInt64, head: ReviewHead, timeLim: UInt32)
    case refused(reason: ReviewRefusal)

    init(_ value: MeguriCoreBindings.ReviewStartResult) {
        switch value {
        case .started(let sessionId, let generation, let head, let timeLim):
            self = .started(sessionId: sessionId, generation: generation, head: ReviewHead(head), timeLim: timeLim)
        case .refused(let reason):
            self = .refused(reason: ReviewRefusal(reason))
        }
    }
}

public enum ReviewPeekResult: Sendable, Hashable {
    case card(item: ReviewHeadItem)
    case empty
    case refused(reason: ReviewRefusal)

    init(_ value: MeguriCoreBindings.ReviewPeekResult) {
        switch value {
        case .card(let item):
            self = .card(item: ReviewHeadItem(item))
        case .empty:
            self = .empty
        case .refused(let reason):
            self = .refused(reason: ReviewRefusal(reason))
        }
    }
}

public enum ReviewAnswerOutcome: Sendable, Hashable {
    case accepted(headDiff: ReviewHeadDiff, leech: Bool, suspended: Bool, rolledOver: Bool)
    case stale(freshHead: ReviewHead, notice: Bool)
    case notAtHead(freshHead: ReviewHead)

    init(_ value: MeguriCoreBindings.ReviewAnswerOutcome) {
        switch value {
        case .accepted(let headDiff, let leech, let suspended, let rolledOver):
            self = .accepted(headDiff: ReviewHeadDiff(headDiff), leech: leech, suspended: suspended, rolledOver: rolledOver)
        case .stale(let freshHead, let notice):
            self = .stale(freshHead: ReviewHead(freshHead), notice: notice)
        case .notAtHead(let freshHead):
            self = .notAtHead(freshHead: ReviewHead(freshHead))
        }
    }
}

public enum ReviewCurrentChange: Sendable, Hashable {
    case eligible
    case removed
    case memoryOnly

    init(_ value: MeguriCoreBindings.ReviewCurrentChange) {
        switch value {
        case .eligible:
            self = .eligible
        case .removed:
            self = .removed
        case .memoryOnly:
            self = .memoryOnly
        }
    }
}

public enum ReviewUndoScope: Sendable, Hashable {
    case global
    case session(sessionId: UInt64)

    init(_ value: MeguriCoreBindings.ReviewUndoScope) {
        switch value {
        case .global:
            self = .global
        case .session(let sessionId):
            self = .session(sessionId: sessionId)
        }
    }
    var raw: MeguriCoreBindings.ReviewUndoScope {
        switch self {
        case .global: .global
        case .session(let sessionId): .session(sessionId: sessionId)
        }
    }
}

public enum ReviewDisabledReason: Sendable, Hashable {
    case none
    case clearedByRollover
    case clearedBySync
    case clearedByOneWay
    case clearedByNonUndoableOp
    case changedExternally
    case alreadyUploaded
    case uploadPending

    init(_ value: MeguriCoreBindings.ReviewDisabledReason) {
        switch value {
        case .none:
            self = .none
        case .clearedByRollover:
            self = .clearedByRollover
        case .clearedBySync:
            self = .clearedBySync
        case .clearedByOneWay:
            self = .clearedByOneWay
        case .clearedByNonUndoableOp:
            self = .clearedByNonUndoableOp
        case .changedExternally:
            self = .changedExternally
        case .alreadyUploaded:
            self = .alreadyUploaded
        case .uploadPending:
            self = .uploadPending
        }
    }
}

public enum ReviewStepKind: Sendable, Hashable {
    case answer
    case other

    init(_ value: MeguriCoreBindings.ReviewStepKind) {
        switch value {
        case .answer:
            self = .answer
        case .other:
            self = .other
        }
    }
}

public enum ReviewUndoStatus: Sendable, Hashable {
    case available(step: ReviewStepInfo)
    case unavailable(reason: ReviewDisabledReason)

    init(_ value: MeguriCoreBindings.ReviewUndoStatus) {
        switch value {
        case .available(let step):
            self = .available(step: ReviewStepInfo(step))
        case .unavailable(let reason):
            self = .unavailable(reason: ReviewDisabledReason(reason))
        }
    }
}

/// Lifecycle jobs cancel while queued for writer admission. Once committing,
/// they finish the drain/release/start and report its actual outcome.
public final class CoreReviewSessionJob: Sendable {
    public let job: CoreJob
    private let raw: MeguriCoreBindings.ReviewSessionJob
    init(job: CoreJob, raw: MeguriCoreBindings.ReviewSessionJob) { self.job = job; self.raw = raw }
    public func value() async throws(BridgeError) -> ReviewStartResult {
        try await job.value()
        let raw = raw
        return try await MaintenanceLane.shared.call(.reviewSessionJobResult) {
            guard let result = try raw.result() else { throw BridgeError.internal("Successful job has no result") }
            return ReviewStartResult(result)
        }
    }
}

extension Core {
    public static func startSession(deckID: Int64, resumeCardID: Int64? = nil) async throws(BridgeError) -> ReviewStartResult {
        let job = try await startSessionJob(deckID: deckID, resumeCardID: resumeCardID)
        return try await job.value()
    }
    public static func startSessionJob(deckID: Int64, resumeCardID: Int64? = nil) async throws(BridgeError) -> CoreReviewSessionJob {
        try await CoreEventStream.shared.start()
        return try await MaintenanceLane.shared.call(.startSession) {
            let raw = try MeguriCoreBindings.startSession(deckId: deckID, resumeCardId: resumeCardID)
            return CoreReviewSessionJob(job: try Jobs.track(raw.handle), raw: raw.result)
        }
    }
    public static func peekFirstCard(deckID: Int64) async throws(BridgeError) -> ReviewPeekResult {
        try await InteractiveLane.shared.call(.peekFirstCard) {
            ReviewPeekResult(try MeguriCoreBindings.peekFirstCard(deckId: deckID))
        }
    }
    public static func sessionHead(sessionID: UInt64, count: UInt32) async throws(BridgeError) -> ReviewHead {
        try await InteractiveLane.shared.call(.sessionHead) {
            ReviewHead(try MeguriCoreBindings.sessionHead(sessionId: sessionID, count: count))
        }
    }
    public static func refreshSessionCard(sessionID: UInt64, cardID: Int64) async throws(BridgeError) -> ReviewHead {
        try await InteractiveLane.shared.call(.refreshSessionCard) {
            ReviewHead(try MeguriCoreBindings.refreshSessionCard(sessionId: sessionID, cardId: cardID))
        }
    }
    public static func renderSessionHead(sessionID: UInt64, keys: [ReviewRenderKey], platform: CardRenderPlatform) async throws(BridgeError) -> [RenderedCard] {
        try await InteractiveLane.shared.call(.renderSessionHead) {
            try MeguriCoreBindings.renderSessionHead(sessionId: sessionID, keys: keys.map(\.raw), platform: platform == .mac ? .mac : .ios).map(RenderedCard.init)
        }
    }
    public static func answerSession(sessionID: UInt64, cardID: Int64, rating: ReviewRating, stateToken: ReviewStateToken, answeredAtMs: Int64, elapsedMs: UInt64) async throws(BridgeError) -> ReviewAnswerOutcome {
        try await InteractiveLane.shared.call(.answerSession) {
            ReviewAnswerOutcome(try MeguriCoreBindings.answerSession(sessionId: sessionID, cardId: cardID, rating: rating.raw, stateToken: stateToken.raw, answeredAtMs: answeredAtMs, elapsedMs: elapsedMs))
        }
    }
    public static func endSession(sessionID: UInt64) async throws(BridgeError) {
        let job = try await endSessionJob(sessionID: sessionID)
        try await job.value()
    }
    public static func endSessionJob(sessionID: UInt64) async throws(BridgeError) -> CoreJob {
        try await Jobs.start(.endSession) { try MeguriCoreBindings.endSession(sessionId: sessionID) }
    }
    public static func peekReviewUndo(scope: ReviewUndoScope) async throws(BridgeError) -> ReviewUndoStatus {
        try await InteractiveLane.shared.call(.peekReviewUndo) {
            ReviewUndoStatus(try MeguriCoreBindings.peekReviewUndo(scope: scope.raw))
        }
    }
    public static func flushReviewForBackground() async throws(BridgeError) {
        try await ControlLane.shared.call(.flushReviewForBackground) { try MeguriCoreBindings.flushReviewForBackground() }
    }
}
