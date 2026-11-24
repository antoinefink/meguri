@testable import AppModel
import Foundation
import Testing

@Suite struct ImportStagingSweepTests {
    @Test func pastCopiesGoAndAFreshDeliveryStays() throws {
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        let drops = root.appending(path: "Drops"), inbox = root.appending(path: "Inbox")
        let other = root.appending(path: "Backups")
        for folder in [drops, inbox, other] { try files.createDirectory(at: folder, withIntermediateDirectories: true) }
        let launch = Date(timeIntervalSince1970: 1_900_000_000)
        let dropped = drops.appending(path: "A1"); try files.createDirectory(at: dropped, withIntermediateDirectories: true)
        try Data("x".utf8).write(to: dropped.appending(path: "Sample.apkg"))
        let old = inbox.appending(path: "Old.apkg"), fresh = inbox.appending(path: "Fresh.apkg")
        let kept = other.appending(path: "backup.colpkg")
        for file in [old, fresh, kept] { try Data("x".utf8).write(to: file) }
        try files.setAttributes([.modificationDate: launch.addingTimeInterval(-3_600)], ofItemAtPath: old.path)
        try files.setAttributes([.modificationDate: launch.addingTimeInterval(-5)], ofItemAtPath: fresh.path)

        #expect(ImportStagingSweep.run(drops: drops, inbox: inbox, launchedAt: launch) == 2)
        #expect(!files.fileExists(atPath: dropped.path) && !files.fileExists(atPath: old.path))
        #expect(files.fileExists(atPath: fresh.path), "a delivery for this launch is kept")
        #expect(files.fileExists(atPath: kept.path), "nothing outside the staging places is touched")
    }

    @Test func aSweepIsBounded() throws {
        let files = FileManager.default
        let drops = files.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: drops) }
        for index in 0..<5 { try files.createDirectory(at: drops.appending(path: "\(index)"), withIntermediateDirectories: true) }
        #expect(ImportStagingSweep.run(drops: drops, inbox: nil, launchedAt: .now, limit: 3) == 3)
        #expect(try files.contentsOfDirectory(atPath: drops.path).count == 2)
    }
}
