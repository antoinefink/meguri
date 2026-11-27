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
}
