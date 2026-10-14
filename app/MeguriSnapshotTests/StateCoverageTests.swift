#if MEGURI_TEST_HOOKS
import DesignSystem
import Foundation
import Testing

@testable import Meguri

/// What `StateCoverageTests` checks (R1970): every registered screen has a
/// fixture for each state its `screen-inventory.yaml` entry lists. A sample
/// screen (`sample.*`, test hooks only, never in the inventory) has all five.
/// Inventory screens nobody has registered yet are U252's
/// `InventoryCoverageTests`' business.
enum StateCoverage {
    struct InventoryScreen: Decodable {
        let id: String
        let states: [String]
    }

    struct Inventory: Decodable {
        let screens: [InventoryScreen]
    }

    /// The inventory as JSON (`tools/mg snapshots` converts the YAML).
    static func load(_ url: URL) throws -> [String: [String]] {
        let inventory = try JSONDecoder().decode(Inventory.self, from: Data(contentsOf: url))
        return Dictionary(inventory.screens.map { ($0.id, $0.states) }, uniquingKeysWith: { first, _ in first })
    }

    /// One line per problem, naming the screen.
    static func problems(registered: [(id: String, states: [PreviewState])], inventory: [String: [String]]) -> [String] {
        var problems: [String] = []
        for screen in registered.sorted(by: { $0.id < $1.id }) {
            let required: [String]
            if screen.id.hasPrefix("sample.") {
                required = PreviewState.allCases.map(\.rawValue)
            } else if let listed = inventory[screen.id] {
                required = listed
            } else {
                problems.append("\(screen.id) is registered for snapshots but has no screen-inventory.yaml entry")
                continue
            }
            let provided = Set(screen.states.map(\.rawValue))
            for state in required where !provided.contains(state) {
                let source = screen.id.hasPrefix("sample.") ? "a sample screen needs every state" : "its screen-inventory.yaml entry lists it"
                problems.append("\(screen.id) has no \(state) state fixture (\(source))")
            }
        }
        return problems
    }
}

@MainActor
@Suite struct StateCoverageTests {
    @Test(.enabled(if: SnapshotRun.current.inventory != nil, "needs MEGURI_INVENTORY_JSON: run through tools/mg snapshots"))
    func everyRegisteredScreenHasEachStateItsInventoryEntryLists() throws {
        let run = SnapshotRun.current
        let inventory = try StateCoverage.load(try #require(run.inventory))
        let registered = MatrixRegistry.screens.map { (id: $0.id, states: $0.type.previewStates) }
        let problems = StateCoverage.problems(registered: registered, inventory: inventory)
        #expect(problems.isEmpty, "\(problems.joined(separator: "; "))")
        // tools/mg snapshots requires this file: a skipped check is no check.
        // It also lists where each screen is registered and the cells it is
        // rendered in on each platform (registry.json), whatever the run's
        // filter: tools/mg critic grades a screen only on all of them.
        let report = try JSONSerialization.data(withJSONObject: [
            "screens": MatrixRegistry.screens.map {
                [
                    "id": $0.id, "states": $0.type.previewStates.map(\.rawValue), "platforms": MatrixRegistry.platforms(of: $0).map(\.rawValue),
                    "cells": MatrixRegistry.renderedCells(of: $0),
                ] as [String: Any]
            },
            "problems": problems,
        ], options: [.sortedKeys])
        _ = SnapshotLog.write(report, as: "state-coverage.json", run: run)
    }

    @Test func aScreenWithoutAStateItsInventoryEntryListsIsNamed() {
        let problems = StateCoverage.problems(
            registered: [(id: "home.deckList", states: [.empty, .typical, .overflow, .loading])],
            inventory: ["home.deckList": ["empty", "typical", "overflow", "error", "loading"]]
        )
        #expect(problems == ["home.deckList has no error state fixture (its screen-inventory.yaml entry lists it)"])
    }

    @Test func statesTheInventoryDoesNotListAreNotRequired() {
        let problems = StateCoverage.problems(
            registered: [(id: "shell.root", states: [.typical, .loading, .error])],
            inventory: ["shell.root": ["typical", "loading", "error"]]
        )
        #expect(problems.isEmpty)
    }

    @Test func aRegisteredScreenOutsideTheInventoryIsNamed() {
        let problems = StateCoverage.problems(registered: [(id: "home.unknown", states: [.typical])], inventory: [:])
        #expect(problems == ["home.unknown is registered for snapshots but has no screen-inventory.yaml entry"])
    }

    @Test func theSampleScreenIsRegisteredOnEveryPlatform() throws {
        let sample = try #require(MatrixRegistry.screens.first { $0.id == "sample.deckOverview" })
        #expect(MatrixRegistry.platforms(of: sample) == [.iphone, .ipad, .mac])
    }

    /// The registry lists every cell a simulator or the Mac renders, in all
    /// states (deferred cells are the device session's), so the critic can
    /// tell a filtered run from a full one.
    @Test func theRegistryListsEveryRenderedCellOfAScreen() throws {
        let sample = try #require(MatrixRegistry.screens.first { $0.id == "sample.deckOverview" })
        let cells = MatrixRegistry.renderedCells(of: sample)
        #expect(Set(cells.keys) == ["iphone", "ipad", "mac"])
        #expect(Set(cells["mac"] ?? []) == Set(PreviewState.allCases.flatMap { state in
            ["light", "dark"].map { "\(state.rawValue)/\($0)-standard-macDefault" }
        }))
        let iphone = try #require(cells["iphone"])
        #expect(iphone.contains("typical/light-ax5-iphoneLarge") && iphone.contains { $0.hasPrefix("overflow/") && $0.hasSuffix("-iphoneSmall") })
        #expect(!iphone.contains { $0.contains("foldable") }, "foldable cells are deferred")
    }

    @Test func aSampleScreenNeedsAllFiveStates() {
        let problems = StateCoverage.problems(registered: [(id: "sample.x", states: [.typical, .empty, .overflow, .loading])], inventory: [:])
        #expect(problems == ["sample.x has no error state fixture (a sample screen needs every state)"])
        #expect(TestHooks.SampleDeckOverview.previewStates == PreviewState.allCases)
    }
}
#endif
