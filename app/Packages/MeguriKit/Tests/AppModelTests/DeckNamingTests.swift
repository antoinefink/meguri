import CoreBridge
import Testing
@testable import AppModel

@MainActor
@Suite struct DeckNamingTests {
    private static let counts = ReviewCounts(new: 0, learning: 0, review: 0)
    private static func node(_ id: Int64, _ name: String, filtered: Bool = false, _ children: [CountedDeckNode] = []) -> CountedDeckNode {
        CountedDeckNode(id: id, name: name, filtered: filtered, studyCollapsed: true, browserCollapsed: true,
            counts: counts, children: children)
    }
    /// Languages › 日本語 › {文法, 読解}, Languages › Español, Filtered, Science.
    private static let tree = [
        node(1, "Languages", [node(2, "日本語", [node(3, "文法"), node(4, "読解")]), node(5, "Español")]),
        node(6, "Filtered", filtered: true),
        node(7, "Science"),
    ]

    @Test func choicesFollowTheTreeWithDepthAndLeaveOutFilteredDecks() throws {
        let choices = DeckNaming.parentChoices(in: Self.tree)
        #expect(choices.map(\.id) == [1, 2, 3, 4, 5, 7])
        #expect(choices.map(\.depth) == [0, 1, 2, 2, 1, 0])
        let grammar = try #require(choices.first { $0.id == 3 })
        #expect(grammar.name == "文法")
        #expect(grammar.fullName == "Languages::日本語::文法")
        #expect(grammar.spokenPath == "Languages › 日本語 › 文法")
    }

    @Test func renameLeavesOutTheDeckAndItsSubdecks() {
        #expect(DeckNaming.parentChoices(in: Self.tree, excluding: 2).map(\.id) == [1, 5, 7])
        #expect(DeckNaming.parentChoices(in: Self.tree, excluding: 1).map(\.id) == [7])
        #expect(DeckNaming.parentChoices(in: Self.tree, excluding: 3).map(\.id) == [1, 2, 4, 5, 7])
    }

    @Test func fullNameJoinsTheParentAndKeepsTypedNesting() throws {
        let parent = try #require(DeckNaming.parentChoices(in: Self.tree).first { $0.id == 2 })
        #expect(DeckNaming.fullName(name: "読解", parent: parent) == "Languages::日本語::読解")
        #expect(DeckNaming.fullName(name: "読解", parent: nil) == "読解")
        // "::" typed in the name still nests, under the parent or at the top level.
        #expect(DeckNaming.fullName(name: "A::B", parent: parent) == "Languages::日本語::A::B")
        #expect(DeckNaming.fullName(name: "A::B", parent: nil) == "A::B")
    }

    @Test func parentAndDescendantsFromTheTree() {
        #expect(DeckNaming.parent(of: 3, in: Self.tree)?.id == 2)
        #expect(DeckNaming.parent(of: 2, in: Self.tree)?.id == 1)
        #expect(DeckNaming.parent(of: 1, in: Self.tree) == nil)
        #expect(DeckNaming.parent(of: 99, in: Self.tree) == nil)
        #expect(DeckNaming.descendantCount(of: 1, in: Self.tree) == 4)
        #expect(DeckNaming.descendantCount(of: 2, in: Self.tree) == 2)
        #expect(DeckNaming.descendantCount(of: 3, in: Self.tree) == 0)
        #expect(DeckNaming.descendantCount(of: 99, in: Self.tree) == 0)
    }

    @Test func footerSaysWhatMovesOrWhereTheDeckGoes() throws {
        let parent = try #require(DeckNaming.parentChoices(in: Self.tree).first { $0.id == 2 })
        #expect(DeckNaming.footer(renaming: true, descendants: 3, parent: parent) == .subdecksMove(3))
        #expect(DeckNaming.footer(renaming: true, descendants: 1, parent: nil) == .subdecksMove(1))
        #expect(DeckNaming.footer(renaming: true, descendants: 0, parent: parent) == .inside("Languages › 日本語"))
        #expect(DeckNaming.footer(renaming: false, descendants: 0, parent: parent) == .inside("Languages › 日本語"))
        #expect(DeckNaming.footer(renaming: false, descendants: 0, parent: nil) == nil)
        #expect(DeckNaming.footer(renaming: true, descendants: 0, parent: nil) == nil)
    }

    @Test func formDefaultsComposeAndRefuseAnUnchangedRename() async {
        let owner = await makeOwner(), model = owner.deckActions
        model.present(.create)
        #expect(model.name.isEmpty && model.parentID == nil && model.footer == nil && !model.canSubmit)
        model.name = "Korean"
        #expect(model.composedName == "Korean" && model.canSubmit)
        model.cancel()

        model.present(.create, name: "日本")
        #expect(model.name == "日本" && model.parentID == nil)
        model.cancel()

        model.present(.subdeck(2))
        #expect(model.name.isEmpty && model.parentID == 2)
        #expect(model.footer == .inside("Languages › 日本語"))
        model.name = "聴解"
        #expect(model.composedName == "Languages::日本語::聴解" && model.canSubmit)
        model.parentID = nil
        #expect(model.composedName == "聴解" && model.footer == nil)
        model.cancel()

        model.present(.rename(2))
        #expect(model.name == "日本語" && model.parentID == 1)
        #expect(model.parentChoices.map(\.id) == [1, 5, 7])
        #expect(model.footer == .subdecksMove(2))
        #expect(!model.canSubmit, "Nothing changed")
        model.parentID = 7
        #expect(model.composedName == "Science::日本語" && model.canSubmit)
        model.parentID = 1; model.name = "QZ70"
        #expect(model.composedName == "Languages::QZ70" && model.canSubmit)
        model.name = "  "
        #expect(!model.canSubmit)
        model.cancel()

        model.present(.rename(5))
        #expect(model.footer == .inside("Languages"))
        model.cancel()
        #expect(!model.canPresent(.subdeck(6)))
    }

    private func makeOwner() async -> ReviewCollection {
        let owner = ReviewCollection(configuration: LaunchConfiguration(), navigation: AppNavigation())
        owner.loadDeckTree = { CountedDeckTree(generation: 11, nextChangeAt: Int64.max, nodes: Self.tree) }
        owner.receiveLifecycle(.ready(generation: 11)); await owner.loadCounts()
        return owner
    }
}
