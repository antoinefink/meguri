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
