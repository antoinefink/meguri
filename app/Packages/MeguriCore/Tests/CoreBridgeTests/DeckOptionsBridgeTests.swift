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
