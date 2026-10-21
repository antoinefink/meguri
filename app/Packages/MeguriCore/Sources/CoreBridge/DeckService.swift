internal import MeguriCoreBindings
import Instrumentation

public struct CountedDeckNode: Sendable, Hashable {
    public let id: Int64
    public let name: String
    public let filtered: Bool
    public let studyCollapsed: Bool
    public let browserCollapsed: Bool
    public let counts: ReviewCounts
    public let children: [CountedDeckNode]
    public init(id: Int64, name: String, filtered: Bool, studyCollapsed: Bool, browserCollapsed: Bool,
                counts: ReviewCounts, children: [CountedDeckNode]) {
        self.id = id; self.name = name; self.filtered = filtered
        self.studyCollapsed = studyCollapsed; self.browserCollapsed = browserCollapsed
        self.counts = counts; self.children = children
    }
    init(_ raw: MeguriCoreBindings.CountedDeckNode) {
        self.init(id: raw.id, name: raw.name, filtered: raw.filtered, studyCollapsed: raw.studyCollapsed,
                  browserCollapsed: raw.browserCollapsed, counts: ReviewCounts(raw.counts), children: raw.children.map(Self.init))
    }
}
public struct CountedDeckTree: Sendable, Hashable {
    public let generation: UInt64
    public let nextChangeAt: Int64
    public let nodes: [CountedDeckNode]
    public init(generation: UInt64, nextChangeAt: Int64, nodes: [CountedDeckNode]) {
        self.generation = generation; self.nextChangeAt = nextChangeAt; self.nodes = nodes
    }
    init(_ raw: MeguriCoreBindings.CountedDeckTree) {
        self.init(generation: raw.generation, nextChangeAt: raw.nextChangeAt, nodes: raw.nodes.map(CountedDeckNode.init))
    }
}
public protocol DeckService: Sendable {
    func addDeck(name: String) async throws(BridgeError) -> DeckMutation
    func renameDeck(id: Int64, name: String) async throws(BridgeError) -> DeckMutation
    func moveDecks(ids: [Int64], parent: Int64?) async throws(BridgeError) -> CountMutation
    func collapseDecks(ids: [Int64], scope: DeckCollapseScope, collapsed: Bool) async throws(BridgeError) -> MutationSummary
    func selectDeck(id: Int64) async throws(BridgeError) -> MutationSummary
    func deckDeletionSummary(ids: [Int64]) async throws(BridgeError) -> DeckDeletionSummary
    func startDeleteDecks(ids: [Int64]) async throws(BridgeError) -> CoreDeckDeletionJob
    func treeWithCounts() async throws(BridgeError) -> CountedDeckTree
    func studyState(deckID: Int64) async throws(BridgeError) -> ReviewDeckStudyState
    func descriptionHTML(deckID: Int64) async throws(BridgeError) -> String
}
public actor CoreDeckService: DeckService {
    public init() {}
    public func treeWithCounts() async throws(BridgeError) -> CountedDeckTree {
        try await QueryLane.shared.call(.deckTreeWithCounts) { CountedDeckTree(try deckTreeWithCounts()) }
    }
    public func studyState(deckID: Int64) async throws(BridgeError) -> ReviewDeckStudyState {
        try await QueryLane.shared.call(.deckStudyState) { ReviewDeckStudyState(try deckStudyState(deckId: deckID)) }
    }
    public func descriptionHTML(deckID: Int64) async throws(BridgeError) -> String {
        try await QueryLane.shared.call(.deckDescriptionHtml) { try deckDescriptionHtml(deckId: deckID) }
    }
}

public enum DeckCollapseScope: Sendable, Hashable {
    case study
    case browser
    init(_ value: MeguriCoreBindings.DeckCollapseScope) {
        switch value {
        case .study: self = .study
        case .browser: self = .browser
        }
    }
    var raw: MeguriCoreBindings.DeckCollapseScope {
        switch self {
        case .study: .study
        case .browser: .browser
        }
    }
}

public struct DeckMutation: Sendable, Hashable {
    public let summary: MutationSummary
    public let id: Int64
    public let name: String
    public init(summary: MutationSummary, id: Int64, name: String) {
        self.summary = summary
        self.id = id
        self.name = name
    }
    init(_ value: MeguriCoreBindings.DeckMutation) {
        summary = MutationSummary(value.summary)
        id = value.id
        name = value.name
    }
}

public struct DeckDeletionSummary: Sendable, Hashable {
    public let decks: UInt64
    public let cards: UInt64
    public init(decks: UInt64, cards: UInt64) {
        self.decks = decks
        self.cards = cards
    }
    init(_ value: MeguriCoreBindings.DeckDeletionSummary) {
        decks = value.decks
        cards = value.cards
    }
}

public struct DeckDeletion: Sendable, Hashable {
    public let summary: MutationSummary
    public let deleted: DeckDeletionSummary
    public init(summary: MutationSummary, deleted: DeckDeletionSummary) {
        self.summary = summary
        self.deleted = deleted
    }
    init(_ value: MeguriCoreBindings.DeckDeletion) {
        summary = MutationSummary(value.summary)
        deleted = DeckDeletionSummary(value.deleted)
    }
}

/// Cancellation is observed before engine orchestration starts. The current
/// U25/deletion APIs do not accept a token after entry; that contract remains
/// pending and callers must await completion before acting on cancellation.
public final class CoreDeckDeletionJob: Sendable {
    public let job: CoreJob
    private let raw: MeguriCoreBindings.DeckDeletionJob
    init(job: CoreJob, raw: MeguriCoreBindings.DeckDeletionJob) { self.job = job; self.raw = raw }
    public func value() async throws(BridgeError) -> DeckDeletion {
        try await job.value()
        let raw = raw
        return try await MaintenanceLane.shared.call(.deckDeletionJobResult) {
            guard let result = try raw.result() else { throw BridgeError.internal("Successful job has no result") }
            return DeckDeletion(result)
        }
    }
}

extension CoreDeckService {
    public func addDeck(name: String) async throws(BridgeError) -> DeckMutation {
        try await InteractiveLane.shared.call(.addDeck) { DeckMutation(try MeguriCoreBindings.addDeck(name: name)) }
    }
    public func renameDeck(id: Int64, name: String) async throws(BridgeError) -> DeckMutation {
        try await InteractiveLane.shared.call(.renameDeck) { DeckMutation(try MeguriCoreBindings.renameDeck(id: id, name: name)) }
    }
    public func moveDecks(ids: [Int64], parent: Int64?) async throws(BridgeError) -> CountMutation {
        try await InteractiveLane.shared.call(.moveDecks) { CountMutation(try MeguriCoreBindings.moveDecks(ids: ids, parent: parent)) }
    }
    public func collapseDecks(ids: [Int64], scope: DeckCollapseScope, collapsed: Bool) async throws(BridgeError) -> MutationSummary {
        try await InteractiveLane.shared.call(.collapseDecks) { MutationSummary(try MeguriCoreBindings.collapseDecks(ids: ids, scope: scope.raw, collapsed: collapsed)) }
    }
    public func selectDeck(id: Int64) async throws(BridgeError) -> MutationSummary {
        try await InteractiveLane.shared.call(.selectDeck) { MutationSummary(try MeguriCoreBindings.selectDeck(id: id)) }
    }
    public func deckDeletionSummary(ids: [Int64]) async throws(BridgeError) -> DeckDeletionSummary {
        try await QueryLane.shared.call(.deckDeletionSummary) { DeckDeletionSummary(try MeguriCoreBindings.deckDeletionSummary(ids: ids)) }
    }
    public func startDeleteDecks(ids: [Int64]) async throws(BridgeError) -> CoreDeckDeletionJob {
        try await CoreEventStream.shared.start()
        return try await MaintenanceLane.shared.call(.startDeleteDecks) {
            let raw = try MeguriCoreBindings.startDeleteDecks(ids: ids)
            return CoreDeckDeletionJob(job: try Jobs.track(raw.handle), raw: raw.result)
        }
    }
}
