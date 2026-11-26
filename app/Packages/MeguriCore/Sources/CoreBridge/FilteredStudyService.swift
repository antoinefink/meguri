internal import MeguriCoreBindings
import Instrumentation

public struct FilteredStudyTerm: Sendable, Equatable {
    public var search: String
    public var limit: UInt32
    public var order: UInt32
    public init(search: String, limit: UInt32, order: UInt32) {
        self.search = search; self.limit = limit; self.order = order
    }
    init(_ raw: MeguriCoreBindings.FilteredStudyTerm) {
        self.init(search: raw.search, limit: raw.limit, order: raw.order)
    }
    var raw: MeguriCoreBindings.FilteredStudyTerm { .init(search: search, limit: limit, order: order) }
}
public struct FilteredStudyDefinition: Sendable, Equatable {
    public var name: String
    public var terms: [FilteredStudyTerm]
    public var reschedule: Bool
    public var againSeconds: UInt32
    public var hardSeconds: UInt32
    public var goodSeconds: UInt32
    public init(name: String, terms: [FilteredStudyTerm], reschedule: Bool,
                againSeconds: UInt32, hardSeconds: UInt32, goodSeconds: UInt32) {
        self.name = name; self.terms = terms; self.reschedule = reschedule
        self.againSeconds = againSeconds; self.hardSeconds = hardSeconds; self.goodSeconds = goodSeconds
    }
    init(_ raw: MeguriCoreBindings.FilteredStudyDefinition) {
        self.init(name: raw.name, terms: raw.terms.map(FilteredStudyTerm.init), reschedule: raw.reschedule,
                  againSeconds: raw.againSeconds, hardSeconds: raw.hardSeconds, goodSeconds: raw.goodSeconds)
    }
    var raw: MeguriCoreBindings.FilteredStudyDefinition {
        .init(name: name, terms: terms.map(\.raw), reschedule: reschedule,
              againSeconds: againSeconds, hardSeconds: hardSeconds, goodSeconds: goodSeconds)
    }
}
public enum CustomStudyMode: Sendable, Equatable {
    case increaseNew, increaseReview, forgotten, ahead, previewNew, byState
    var raw: MeguriCoreBindings.CustomStudyMode {
        switch self {
        case .increaseNew: .increaseNew
        case .increaseReview: .increaseReview
        case .forgotten: .forgotten
        case .ahead: .ahead
        case .previewNew: .previewNew
        case .byState: .byState
        }
    }
}
public enum CustomStudyState: Sendable, Equatable {
    case new, due, review, all
    var raw: MeguriCoreBindings.CustomStudyState {
        switch self {
        case .new: .new
        case .due: .due
        case .review: .review
        case .all: .all
        }
    }
}
public struct CustomStudyRequest: Sendable, Equatable {
    public var mode: CustomStudyMode
    public var amount: Int32
    public var state: CustomStudyState
    public var includeTags: [String]
    public var excludeTags: [String]
    public init(mode: CustomStudyMode, amount: Int32, state: CustomStudyState, includeTags: [String], excludeTags: [String]) {
        self.mode = mode; self.amount = amount; self.state = state
        self.includeTags = includeTags; self.excludeTags = excludeTags
    }
    var raw: MeguriCoreBindings.CustomStudyRequest {
        .init(mode: mode.raw, amount: amount, state: state.raw, includeTags: includeTags, excludeTags: excludeTags)
    }
}
public struct CustomStudyDefaults: Sendable, Equatable {
    public let availableNew: UInt32
    public let availableReview: UInt32
    public let extendNew: Int32
    public let extendReview: Int32
    public let includeTags: [String]
    public let excludeTags: [String]
    public init(availableNew: UInt32, availableReview: UInt32, extendNew: Int32, extendReview: Int32,
                includeTags: [String], excludeTags: [String]) {
        self.availableNew = availableNew; self.availableReview = availableReview
        self.extendNew = extendNew; self.extendReview = extendReview
        self.includeTags = includeTags; self.excludeTags = excludeTags
    }
    init(_ raw: MeguriCoreBindings.CustomStudyDefaults) {
        self.init(availableNew: raw.availableNew, availableReview: raw.availableReview,
                  extendNew: raw.extendNew, extendReview: raw.extendReview,
                  includeTags: raw.includeTags, excludeTags: raw.excludeTags)
    }
}
public protocol FilteredStudyService: Sendable {
    func definition(deck: Int64, generation: UInt64) async throws(BridgeError) -> FilteredStudyDefinition
    func save(deck: Int64?, definition: FilteredStudyDefinition, allowEmpty: Bool, generation: UInt64) async throws(BridgeError) -> Int64
    func deckScope(deck: Int64, generation: UInt64) async throws(BridgeError) -> String
    func rebuild(deck: Int64, generation: UInt64) async throws(BridgeError)
    func empty(deck: Int64, generation: UInt64) async throws(BridgeError)
    func custom(deck: Int64, request: CustomStudyRequest, generation: UInt64) async throws(BridgeError) -> Int64
    func customDefaults(deck: Int64, generation: UInt64) async throws(BridgeError) -> CustomStudyDefaults
}

/// Every command carries the initiating route's generation, including its first call.
public actor CoreFilteredStudyService: FilteredStudyService {
    public private(set) var lastMutation: MutationSummary?
    public init() {}
    public func deckScope(deck: Int64, generation: UInt64) async throws(BridgeError) -> String {
        try await QueryLane.shared.call(.filteredStudyDeckScope) {
            try MeguriCoreBindings.filteredStudyDeckScope(deck: deck, generation: generation)
        }
    }
    public func definition(deck: Int64, generation: UInt64) async throws(BridgeError) -> FilteredStudyDefinition {
        return try await QueryLane.shared.call(.filteredStudyDefinition) {
            FilteredStudyDefinition(try MeguriCoreBindings.filteredStudyDefinition(deck: deck, generation: generation))
        }
    }
    public func customDefaults(deck: Int64, generation: UInt64) async throws(BridgeError) -> CustomStudyDefaults {
        return try await QueryLane.shared.call(.customStudyDefaults) {
            CustomStudyDefaults(try MeguriCoreBindings.customStudyDefaults(deck: deck, generation: generation))
        }
    }
    public func save(deck: Int64?, definition: FilteredStudyDefinition, allowEmpty: Bool, generation: UInt64) async throws(BridgeError) -> Int64 {
        try await perform(.save(deck: deck, definition: definition.raw, allowEmpty: allowEmpty), generation: generation)
    }
    public func rebuild(deck: Int64, generation: UInt64) async throws(BridgeError) {
        _ = try await perform(.rebuild(deck: deck), generation: generation)
    }
    public func empty(deck: Int64, generation: UInt64) async throws(BridgeError) {
        _ = try await perform(.empty(deck: deck), generation: generation)
    }
    public func custom(deck: Int64, request: CustomStudyRequest, generation: UInt64) async throws(BridgeError) -> Int64 {
        try await perform(.custom(deck: deck, request: request.raw), generation: generation)
    }
    private func perform(_ command: MeguriCoreBindings.FilteredStudyCommand, generation: UInt64) async throws(BridgeError) -> Int64 {
        try await CoreEventStream.shared.start()
        let started = try await MaintenanceLane.shared.call(.startFilteredStudy) {
            try MeguriCoreBindings.startFilteredStudy(command: command, generation: generation)
        }
        let job = await Jobs.trackMutationDraining(started.handle)
        try await job.value()
        let raw = started.result
        let result = try await MaintenanceLane.shared.call(.filteredStudyJobResult) {
            guard let result = try raw.result() else { throw BridgeError.internal("Successful job has no result") }
            return result
        }
        lastMutation = MutationSummary(result.summary)
        return result.deck
    }
}
