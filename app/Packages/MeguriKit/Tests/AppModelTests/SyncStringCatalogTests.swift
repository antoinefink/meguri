import Foundation
import Testing
@testable import AppModel

@MainActor struct SyncStringCatalogTests {
    @Test func supportedServicesAndSetupHavePlainEnglishCopy() {
        for (key, text) in [("sync.service.off", "Off"), ("sync.service.meguri", "Meguri Sync"),
                            ("sync.status.openSettings", "Sync Settings")] {
            #expect(SyncText.value(key) == text)
        }
        #expect(SyncText.format("sync.meguri.media.left", locale: Locale(identifier: "en_US"), [Int64(4_431)])
            == "4,431 media files are left. Syncing starts when they finish.")
    }
    @Test func syncCatalogContainsNoRetiredServiceCopy() throws {
        let root = URL(filePath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent()
        let path = root.appending(path: "Sources/AppModel/Sync/Resources/Sync.xcstrings")
        let text = try String(contentsOf: path, encoding: .utf8).lowercased()
        #expect(!text.contains("icloud") && !text.contains("cloudkit"))
    }
    @Test func reachableSyncCopyKeysExistInTheCatalog() throws {
        let package = URL(filePath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent()
        let catalog = package.appending(path: "Sources/AppModel/Sync/Resources/Sync.xcstrings")
        let document = try #require(JSONSerialization.jsonObject(with: Data(contentsOf: catalog)) as? [String: Any])
        let strings = try #require(document["strings"] as? [String: Any])
        let modelFiles = ["SyncKinds.swift", "ServiceSwitchConfirmation.swift"]
        let literal = try NSRegularExpression(pattern: #""(sync\.[A-Za-z0-9_.]+)""#)
        for name in modelFiles {
            let source = try String(contentsOf: package.appending(path: "Sources/AppModel/Sync/\(name)"), encoding: .utf8)
            for match in literal.matches(in: source, range: NSRange(source.startIndex..., in: source)) {
                let range = try #require(Range(match.range(at: 1), in: source))
                let key = String(source[range])
                #expect(strings[key] != nil, "Missing sync catalog key: \(key)")
            }
        }
        #expect(strings["sync.service.recovery.message"] != nil)
        #expect(SyncText.value("sync.service.recovery.message").contains("Meguri Sync"))
    }

}
