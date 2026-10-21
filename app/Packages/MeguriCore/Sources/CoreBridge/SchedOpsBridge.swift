import Foundation
import Instrumentation
internal import MeguriCoreBindings

public enum InvalidDaysKind: Sendable, Hashable {
    case empty, malformed, tooLarge
    init(_ raw: MeguriCoreBindings.InvalidDaysKind) {
        switch raw { case .empty: self = .empty; case .malformed: self = .malformed; case .tooLarge: self = .tooLarge }
    }
}
public enum SchedOpRefusal: Sendable, Hashable {
    case invalidDays(InvalidDaysKind)
    case homeDeckFiltered(deck: Int64)
    case inFilteredDeck(card: Int64)
    case notFiltered(deck: Int64)
    case presetUnsupported(presetIDs: [Int64])
    init(_ raw: MeguriCoreBindings.SchedOpRefusal) {
        switch raw {
        case .invalidDays(let kind): self = .invalidDays(InvalidDaysKind(kind))
        case .homeDeckFiltered(let deck): self = .homeDeckFiltered(deck: deck)
        case .inFilteredDeck(let card): self = .inFilteredDeck(card: card)
        case .notFiltered(let deck): self = .notFiltered(deck: deck)
        case .presetUnsupported(let ids): self = .presetUnsupported(presetIDs: ids)
        }
    }
}
public struct DueDays: Sendable, Hashable {
    public let minimum: UInt32
    public let maximum: UInt32
    public let resetInterval: Bool
    public init(minimum: UInt32, maximum: UInt32, resetInterval: Bool) {
        self.minimum = minimum; self.maximum = maximum; self.resetInterval = resetInterval
    }
    init(_ raw: MeguriCoreBindings.DueDays) { self.init(minimum: raw.minimum, maximum: raw.maximum, resetInterval: raw.resetInterval) }
}
public enum SchedTargets: Sendable, Hashable {
    case cards([Int64]), notes([Int64])
    var raw: MeguriCoreBindings.SchedTargets {
        switch self { case .cards(let ids): return .cards(ids: ids); case .notes(let ids): return .notes(ids: ids) }
    }
}
public enum SchedContext: Sendable, Hashable {
    case reviewer, browser
    var raw: MeguriCoreBindings.SchedContext { switch self { case .reviewer: return .reviewer; case .browser: return .browser } }
}
public enum DeckUnburyMode: Sendable, Hashable {
    case all, manual, siblings
    var raw: MeguriCoreBindings.DeckUnburyMode { switch self { case .all: return .all; case .manual: return .manual; case .siblings: return .siblings } }
}
/// Unbury of chosen cards uses unsuspend; both restore all negative queues.
public enum SchedOperation: Sendable, Hashable {
    case bury(SchedTargets), suspend(SchedTargets), unsuspend(cards: [Int64])
    case unburyDeck(deck: Int64, mode: DeckUnburyMode)
    case setDue(cards: [Int64], days: String, context: SchedContext?)
    case reset(cards: [Int64], restorePosition: Bool, resetCounts: Bool, context: SchedContext?)
    case gradeNow(cards: [Int64], rating: ReviewRating)
    case emptyFiltered(deck: Int64)
    var raw: MeguriCoreBindings.SchedOperation {
        switch self {
        case .bury(let targets): return .bury(targets: targets.raw)
        case .suspend(let targets): return .suspend(targets: targets.raw)
        case .unsuspend(let cards): return .unsuspend(cards: cards)
        case .unburyDeck(let deck, let mode): return .unburyDeck(deck: deck, mode: mode.raw)
        case .setDue(let cards, let days, let context): return .setDue(cards: cards, days: days, context: context?.raw)
        case .reset(let cards, let restore, let counts, let context): return .reset(cards: cards, restorePosition: restore, resetCounts: counts, context: context?.raw)
        case .gradeNow(let cards, let rating): return .gradeNow(cards: cards, rating: rating.raw)
        case .emptyFiltered(let deck): return .emptyFiltered(deck: deck)
        }
    }
}
public struct SchedMutation: Sendable, Hashable {
    public let summary: MutationSummary
    public let count: UInt64
    public let headDiff: ReviewHeadDiff?
    public init(summary: MutationSummary, count: UInt64, headDiff: ReviewHeadDiff?) {
        self.summary = summary; self.count = count; self.headDiff = headDiff
    }
    init(_ raw: MeguriCoreBindings.SchedMutation) { self.init(summary: MutationSummary(raw.summary), count: raw.count, headDiff: raw.headDiff.map(ReviewHeadDiff.init)) }
}
public struct BuriedKinds: Sendable, Hashable {
    public let manual: Bool
    public let siblings: Bool
    public init(manual: Bool, siblings: Bool) { self.manual = manual; self.siblings = siblings }
    init(_ raw: MeguriCoreBindings.BuriedKinds) { self.init(manual: raw.manual, siblings: raw.siblings) }
}
public final class CoreSchedOpJob: Sendable {
    public let job: CoreJob
    private let raw: MeguriCoreBindings.SchedOpJob
    init(job: CoreJob, raw: MeguriCoreBindings.SchedOpJob) { self.job = job; self.raw = raw }
    public func value() async throws(BridgeError) -> SchedMutation {
        try await job.value()
        let raw = raw
        return try await MaintenanceLane.shared.call(.schedOpJobResult) {
            guard let result = try raw.result() else { throw BridgeError.internal("Successful job has no result") }
            return SchedMutation(result)
        }
    }
}
public protocol SchedOpsService: Sendable {
    func parseDueDays(_ text: String) async throws(BridgeError) -> DueDays
    func start(_ operation: SchedOperation, sessionID: UInt64?) async throws(BridgeError) -> CoreSchedOpJob
    func buriedKinds(deck: Int64) async throws(BridgeError) -> BuriedKinds
}
public actor CoreSchedOpsService: SchedOpsService {
    public init() {}
    public func parseDueDays(_ text: String) async throws(BridgeError) -> DueDays {
        try await InteractiveLane.shared.call(.parseDueDays) { DueDays(try MeguriCoreBindings.parseDueDays(text: text)) }
    }
    public func start(_ operation: SchedOperation, sessionID: UInt64? = nil) async throws(BridgeError) -> CoreSchedOpJob {
        try await CoreEventStream.shared.start()
        return try await MaintenanceLane.shared.call(.startSchedOp) {
            let raw = try MeguriCoreBindings.startSchedOp(operation: operation.raw, sessionId: sessionID)
            return CoreSchedOpJob(job: try Jobs.track(raw.handle), raw: raw.result)
        }
    }
    public func buriedKinds(deck: Int64) async throws(BridgeError) -> BuriedKinds {
        try await CoreEventStream.shared.start()
        let (job, result) = try await MaintenanceLane.shared.call(.startBuriedKinds) {
            let raw = try MeguriCoreBindings.startBuriedKinds(deck: deck)
            return (try Jobs.track(raw.handle), raw.result)
        }
        try await job.value()
        return try await MaintenanceLane.shared.call(.buriedKindsJobResult) {
            guard let value = try result.result() else { throw BridgeError.internal("Successful job has no result") }
            return BuriedKinds(value)
        }
    }
}
