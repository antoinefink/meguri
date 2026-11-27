import CoreBridge
import Darwin
import Foundation
import Testing
@testable import AppModel

extension ReviewCollectionStartupTests {
    /// Exercises the native page's production adapter, not a DTO-only fixture.
    @Test func deckOptionsProductionDraftAddsSavesReopensAndDiscards() async throws {
        let temp = try #require(realpath(FileManager.default.temporaryDirectory.path, nil))
        let root = URL(filePath: String(cString: temp)).appending(path: UUID().uuidString)
        free(temp)
        let directory = root.appending(path: "collection"), backups = root.appending(path: "backups")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        var configuration = LaunchConfiguration()
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
        configuration.collectionLocation = AppModel.CollectionLocation(directory: directory, backupsDirectory: backups)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            sync: SyncCoordinator(directory: directory, defaults: nil),
            providerDirectory: root.appending(path: "transfers"))
        do {
            await owner.start(); await owner.startEmpty()
            try #require(owner.state == .ready)
            if owner.policy != .ok { await owner.enableFsrs() }
            let target = try await CoreDeckService().addDeck(name: "Options Target")
            await owner.loadCounts()
            let model = owner.deckOptions
            model.open(deckID: target.id); await model.waitForCompletion()
            try #require(model.phase == .editing)
            let originalID = model.selectedPreset
            // Every choice traverses the production adapter, including the nonsequential values.
            let expected: [DeckOptionsDisplayOrder: [Int32]] = [
                .newGatherPriority: [0, 5, 1, 2, 3, 4], .newSortOrder: [0, 1, 2, 3, 4],
                .newMix: [0, 1, 2], .interdayLearningMix: [0, 1, 2],
                .reviewOrder: [0, 1, 2, 3, 4, 6, 5, 7, 11, 12, 8, 9, 10],
            ]
            model.setValue(.newPerDay, 37)
            for row in DeckOptionsDisplayOrder.allCases {
                #expect(model.choices(row) == expected[row])
                for value in model.choices(row) { model.setDisplayOrder(row, value) }
            }
            await model.waitForCompletion()
            model.beginNewPreset(); model.presetName = "  Daily New  "; model.createPreset()
            await model.waitForCompletion()
            try #require(model.selectedPreset < 0)
            let staged = model.selectedPreset
            #expect(model.selected?.name == "Daily New" && model.selectedUseCount == 0)
            #expect(model.value(.newPerDay) == 20)
            model.setDisplayOrder(.newGatherPriority, 5)
            model.setDisplayOrder(.reviewOrder, 11)
            model.selectPreset(originalID)
            #expect(model.value(.newPerDay) == 37 && model.selected?.displayOrder[.reviewOrder] == 10)
            model.selectPreset(staged)
            model.save(); await model.waitForCompletion()
            #expect(model.outcome == .saved)
            await owner.loadCounts()
            model.open(deckID: target.id); await model.waitForCompletion()
            try #require(model.phase == .editing)
            #expect(model.selectedPreset > 0 && model.selected?.name == "Daily New" && model.selectedUseCount == 1)
            #expect(model.selected?.displayOrder[.newGatherPriority] == 5 && model.selected?.displayOrder[.reviewOrder] == 11)
            #expect(model.presets.first { $0.id == originalID }?.newPerDay == 37)
            model.beginNewPreset(); model.presetName = "Discarded"; model.createPreset()
            await model.waitForCompletion()
            model.requestLeave(); model.discardChanges()
            model.open(deckID: target.id); await model.waitForCompletion()
            #expect(!model.presets.contains { $0.name == "Discarded" })
            #expect(model.selected?.name == "Daily New" && !model.hasChanges)
            model.requestLeave()
            _ = await owner.close()
        } catch {
            _ = await owner.close()
            throw error
        }
    }
}
