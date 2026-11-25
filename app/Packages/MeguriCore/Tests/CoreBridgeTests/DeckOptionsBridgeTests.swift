import Foundation
import Testing
import MeguriCoreBindings
@testable import CoreBridge

@Suite struct DeckOptionsBridgeTests {
    @Test func parserUsesProductValidationAndExactFormattingThroughLane() async throws {
        let service = CoreDeckOptionsService()
        let parsed = try await service.parseSteps("1s 10m 36h")
        #expect(parsed.issues.isEmpty && parsed.minutes == [1 / 60, 10, 2160])
        #expect(try await service.formatSteps(parsed.minutes) == "1s 10m 36h")
        let invalid = try await service.parseSteps("0m -5 5x")
        #expect(invalid.issues.map(\.code) == [.stepZero, .stepNegative, .stepMalformed])
        #expect(invalid.issues.map(\.params) == [["index": "0", "token": "0m"], ["index": "1", "token": "-5"], ["index": "2", "token": "5x"]])
    }
    @Test func allFieldsAndTypedIssuesRoundTripWithoutBindingsInPublicDTOs() {
        for field in CoreBridge.OptionField.allCases { #expect(CoreBridge.OptionField(field.raw) == field) }
        let issue = MeguriCoreBindings.OptionIssue(field: .desiredRetention, preset: -7, code: .outOfRange, severity: .error, params: ["min": "0.7", "max": "0.99"])
        let mapped = CoreBridge.OptionIssue(issue)
        #expect(mapped.preset == -7 && mapped.code == .outOfRange && mapped.severity == .error)
        #expect(BridgeError(MeguriCoreBindings.CoreError.DeckOptionsInvalid(issues: [issue])) == .deckOptionsInvalid([mapped]))
    }
}
private final class OptionsClock: CollectionClock {
    func nowMilliseconds() -> Int64 {1_894_701_600_000}
    func utcOffsetSeconds(atMilliseconds: Int64) -> Int32 {0}
    func zoneIdentifier() -> String {"UTC"}
}
extension EventTests {
    @Test func deckOptionsReadValidateStageAndSaveThroughRuntimeJobs() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let collection = root.appending(path: "collection"), backups = root.appending(path: "backups")
        try FileManager.default.createDirectory(at: collection, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: backups, withIntermediateDirectories: true)
        defer {try? FileManager.default.removeItem(at: root)}
        let location = CoreBridge.CollectionLocation(collectionDirectory: collection, backupsDirectory: backups, appGroupSnapshot: root.appending(path: "widget.snapshot"),
            manualTransferDirectory: root.appending(path: "ManualTransfers"))
        try await MaintenanceLane.shared.bootstrapCollection(location)
        let service = CoreCollectionService(clock: OptionsClock())
        do {
            let open = try await service.startOpen(location, randomSeed: 7)
            #expect(await open.outcome() == .succeeded)
            let options = CoreDeckOptionsService()
            let read = try await options.startRead(deck: 1)
            let editor = try await read.value()
            let snapshot = try await editor.snapshot()
            #expect(snapshot.presets.count == 1 && snapshot.presets[0].useCount == 1)
            #expect(snapshot.effectiveNewLimit == 20 && snapshot.effectiveReviewLimit == 200)
            #expect(snapshot.metadata.first { $0.field == .loadBalancerEnabled }?.global == true)
            _ = try await editor.edit(.set(preset: 1, field: .desiredRetention, value: .number(0.68)))
            #expect(try await editor.validate().contains { $0.code == .outOfRange && $0.field == .desiredRetention })
            _ = try await editor.edit(.restoreDefault(preset: 1, field: .desiredRetention))
            let staged = try await editor.edit(.clone(name: "Unsaved clone"))
            #expect(staged.selectedPreset < 0 && staged.unsavedChanges)
            let save = try await editor.startSave()
            #expect(try await save.value().summary.undoName == "Deck Options")
            // O17.3: the first version's page keeps the five deck values as stored.
            let limit = try await (try await options.startRead(deck: 1)).value()
            _ = try await limit.edit(.set(preset: nil, field: .newLimit, value: .number(50)))
            _ = try await (try await limit.startSave()).value()
            let page = try await (try await options.startRead(deck: 1)).value()
            let kept = try await page.edit(.keep(fields: DeckOptionsEdit.deckValues))
            #expect(kept.keptDeckValues == Set(DeckOptionsEdit.deckValues) && !kept.unsavedChanges)
            let selected = kept.selectedPreset
            _ = try await page.edit(.set(preset: selected, field: .newPerDay, value: .number(15)))
            #expect(try await page.validate().isEmpty)
            #expect(try await (try await page.startSave()).value().summary.changes.decks.isEmpty)
            let after = try await (try await options.startRead(deck: 1)).value().snapshot()
            #expect(after.deckValues[.newLimit] == .number(50))
            #expect(after.presets.first { $0.key == selected }?.values[.newPerDay] == .number(15))
            let prefs = try await options.startPreferencesSave(.init(learnAheadMinutes: 33, loadBalancer: false))
            #expect(try await prefs.value().undoName == "Preferences")
            let prefsRead = try await options.startPreferencesRead()
            #expect(try await prefsRead.value().learnAheadMinutes == 33)
            try await service.close()
        } catch {
            try? await service.close()
            throw error
        }
    }
}

extension EventTests {
    @Test func deckOptionsAddUsesDefaultsCanonicalNamesAndOneDurableAssignment() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let collection = root.appending(path: "collection"), backups = root.appending(path: "backups")
        try FileManager.default.createDirectory(at: collection, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: backups, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let location = CoreBridge.CollectionLocation(collectionDirectory: collection, backupsDirectory: backups,
            appGroupSnapshot: root.appending(path: "widget"), manualTransferDirectory: root.appending(path: "transfers"))
        try await MaintenanceLane.shared.bootstrapCollection(location)
        let service = CoreCollectionService(clock: OptionsClock())
        do {
            #expect(await (try await service.startOpen(location, randomSeed: 7)).outcome() == .succeeded)
            let options = CoreDeckOptionsService()
            let initial = try await (try await options.startRead(deck: 1)).value().snapshot()
            // Editing Default before Add proves that Add uses built-in defaults, not a clone.
            let edited = try await (try await options.startRead(deck: 1)).value()
            _ = try await edited.edit(.set(preset: 1, field: .newPerDay, value: .number(37)))
            _ = try await edited.edit(.set(preset: nil, field: .newLimit, value: .number(50)))
            _ = try await edited.edit(.set(preset: nil, field: .deckDesiredRetention, value: .number(0.85)))
            _ = try await (try await edited.startSave()).value()
            // Dropping an editor, including one with a staged preset, writes nothing.
            do {
                let cancelled = try await (try await options.startRead(deck: 1)).value()
                let staged = try await cancelled.edit(.add(name: "Discarded"))
                #expect(staged.selectedPreset < 0 && staged.presets.first { $0.key < 0 }?.useCount == 0)
            }
            let unchanged = try await (try await options.startRead(deck: 1)).value().snapshot()
            #expect(unchanged.presets.count == 1 && unchanged.assignedPreset == 1)
            let editor = try await (try await options.startRead(deck: 1)).value()
            _ = try await editor.edit(.keep(fields: DeckOptionsEdit.deckValues))
            let staged = try await editor.edit(.add(name: "  Default  "))
            let key = staged.selectedPreset
            let added = try #require(staged.presets.first { $0.key == key })
            #expect(key < 0 && added.name == "Default1894701600" && added.useCount == 0)
            #expect(added.values[.newPerDay] == .number(20))
            let order: [(CoreBridge.OptionField, [Int32])] = [
                (.newCardGatherPriority, [0, 5, 1, 2, 3, 4]), (.newCardSortOrder, [0, 1, 2, 3, 4]),
                (.newMix, [0, 1, 2]), (.interdayLearningMix, [0, 1, 2]),
                (.reviewOrder, [0, 1, 2, 3, 4, 6, 5, 7, 11, 12, 8, 9, 10]),
            ]
            for (field, choices) in order {
                #expect(staged.metadata.first { $0.field == field }?.choices == choices)
                for choice in choices {
                    let changed = try await editor.edit(.set(preset: key, field: field, value: .number(Double(choice))))
                    #expect(changed.presets.first { $0.key == key }?.values[field] == .number(Double(choice)))
                    #expect(try await editor.validate().allSatisfy { $0.severity != .error })
                }
            }
            _ = try await editor.edit(.select(preset: 1))
            let switched = try await editor.edit(.select(preset: key))
            #expect(switched.presets.first { $0.key == key }?.useCount == 0)
            let mutation = try await (try await editor.startSave()).value()
            #expect(mutation.preset > 0 && mutation.summary.undoName == "Deck Options")
            #expect(mutation.summary.changes.decks.map(\.id) == [1])
            let savedID = mutation.preset
            _ = try await (try await CoreUndoService().startUndo(scope: .global)).value()
            let undone = try await (try await options.startRead(deck: 1)).value().snapshot()
            #expect(undone.assignedPreset == 1 && undone.presets.count == 1 && undone.deckValues == unchanged.deckValues)
            _ = try await (try await CoreUndoService().startRedo(scope: .global)).value()
            try await service.close()
            #expect(await (try await service.startOpen(location, randomSeed: 7)).outcome() == .succeeded)
            let after = try await (try await options.startRead(deck: 1)).value().snapshot()
            #expect(after.assignedPreset == savedID && after.selectedPreset == savedID && after.presets.count == 2)
            let persisted = try #require(after.presets.first { $0.key == savedID })
            #expect(persisted.name == added.name && persisted.useCount == 1)
            #expect(after.presets.first { $0.key == 1 }?.values[.newPerDay] == .number(37))
            for (field, choices) in order {
                let last = try #require(choices.last)
                #expect(persisted.values[field] == .number(Double(last)))
            }
            // All unrelated defaults and globals survived the same transaction.
            let changedFields = Set(order.map { $0.0 })
            for (field, value) in initial.presets[0].values where !changedFields.contains(field) {
                #expect(persisted.values[field] == value)
            }
            #expect(after.globals == initial.globals && after.deckValues == unchanged.deckValues)
            try await service.close()
        } catch {
            try? await service.close()
            throw error
        }
    }
}
