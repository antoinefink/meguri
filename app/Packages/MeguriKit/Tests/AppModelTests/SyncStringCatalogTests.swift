@testable import AppModel
import Foundation
import Testing

/// `sync/status-strings` T19.9: a row for each key of T19.
@MainActor @Suite struct SyncStringCatalogTests {
    private static let t19: [(String, String)] = [
        ("sync.settings.sync.section", "Sync"), ("sync.service.control", "Service"), ("sync.service.off", "Off"),
        ("sync.service.meguri", "Meguri Sync"), ("sync.service.iCloud.name", "iCloud"), ("sync.service.beta", "Beta"),
        ("sync.service.iCloud.accessibility", "iCloud, Beta"),
        ("sync.service.none.footer", "Choose a service to sync this collection."),
        ("sync.service.settingUp", "Setting Up…"),
        ("sync.service.opFirst", "Finish or cancel the collection replacement before you change the service."),
        ("sync.switch.title.off", "Turn Off Sync"),
        ("sync.switch.next.iCloud.setup", "Next, Meguri sets up iCloud. It asks before it replaces a collection with your cards."),
        ("sync.switch.next.meguri.setup", "Next, sign in to your Meguri Sync server. Meguri asks before it replaces a collection with your cards."),
        ("sync.switch.confirm.off", "Turn Off Sync"),
        ("sync.meguri.signIn.footer.setup", "After you sign in, Meguri sets up syncing with this server, then syncs automatically. It asks before it replaces a collection with your cards."),
        ("sync.meguri.signIn.footer.setup.manual", "After you sign in, Meguri sets up syncing with this server. It asks before it replaces a collection with your cards."),
        ("sync.setup.done.uploaded.iCloud", "This device’s collection was uploaded to iCloud."),
        ("sync.setup.done.downloaded.iCloud", "The iCloud collection was downloaded to this device."),
        ("sync.setup.done.resumed.iCloud", "This collection syncs with iCloud again."),
        ("sync.setup.done.uploaded.meguri", "This device’s collection was uploaded to your Meguri Sync server."),
        ("sync.setup.done.downloaded.meguri", "The collection on your Meguri Sync server was downloaded to this device."),
        ("sync.setup.done.created.iCloud", "A new, empty collection was created on this device and syncs with iCloud."),
        ("sync.setup.done.backup", "The collection that was on this device is in Backups."),
        ("sync.keep.title", "Which Collection Do You Want to Keep?"),
        ("sync.keep.message", "Meguri doesn’t merge two collections. The one you don’t keep is replaced."),
        ("sync.keep.device", "Keep This Device’s Collection"),
        ("sync.keep.device.effect.iCloud", "The iCloud collection is replaced on all your devices. It stays in iCloud for 30 days and can be restored from there."),
        ("sync.keep.device.effect.meguri", "The collection on your Meguri Sync server is replaced. Meguri first saves a copy of it on this device."),
        ("sync.keep.service.iCloud", "Keep the iCloud Collection"), ("sync.keep.service.meguri", "Keep the Meguri Sync Collection"),
        ("sync.keep.service.effect", "This device’s collection is replaced. Meguri backs it up on this device first; media files are not in the backup."),
        ("sync.keep.downloadOnly.message.meguri", "This device has no collection, but it may have media files. Meguri can only download the collection from your Meguri Sync server."),
        ("sync.keep.unavailable.none", "There is no collection in iCloud to keep."),
        ("sync.keep.unavailable.unreadable", "The iCloud collection can’t be read, so it can’t be kept."),
        ("sync.status.off.title", "Sync Off"), ("sync.issues.title", "Sync Issues"),
        ("sync.issues.footer", "Your other changes keep syncing."),
        ("sync.needsYou.action.setUp", "Set Up Again…"), ("sync.needsYou.action.repair", "Show Repair Options"),
        ("sync.waiting.cloudError", "iCloud sync isn’t working right now. Meguri keeps trying."),
        ("sync.settings.account", "Account"), ("sync.settings.troubleshooting", "Troubleshooting"),
        ("sync.trouble.replaceDevice", "Replace This Device’s Collection…"),
        ("sync.trouble.replaceService.iCloud", "Replace the iCloud Collection…"),
        ("sync.trouble.replaceService.meguri", "Replace the Meguri Sync Collection…"),
        ("sync.trouble.log", "Sync Log"), ("sync.action.copyDebugInfo", "Copy Debug Info"),
        ("sync.trouble.replacedMedia", "Show Replaced Media"),
        ("sync.trouble.replacedMedia.footer", "Media files that sync replaced are kept here until you delete them."),
        ("sync.now.unavailable.off", "Sync is off. Choose a service in Sync settings to sync this collection."),
        ("sync.meguri.now.unavailable.signIn.sync", "Sign in to Meguri Sync in Sync settings to sync."),
        ("sync.meguri.now.unavailable.stopped.sync", "Meguri Sync is stopped. Open Sync settings to see what to do."),
        // T20.2 (round 10): the status panel's sign-in action.
        ("sync.status.signIn", "Sign In…"), ("sync.status.openSettings", "Sync Settings"),
        ("sync.status.cancelSetup", "Cancel Setup"), ("sync.button.label.off", "Sync"),
        // T19.5 (changed 2026-11-21, resume fix): setup's media step paused.
        ("sync.meguri.status.downloadPaused.title", "Media Download Paused"),
        ("sync.meguri.status.transferPaused.title", "Media Transfer Paused"),
        ("sync.meguri.media.left.unknown", "Syncing starts when the media finishes.")
    ]
    /// Every needs you text of T19.4 and T19.5.
    private static let needsYou: [(String, String)] = [
        ("sync.needsYou.fence.restore", "Choose which collection to keep after restoring a backup."),
        ("sync.needsYou.fence.import", "Choose which collection to keep after importing a collection."),
        ("sync.needsYou.fence.none", "Choose which collection to keep."),
        ("sync.needsYou.localStorageFull", "This device is out of storage space, so syncing stopped."),
        ("sync.needsYou.update", "Update Meguri on this device to sync all your changes."),
        ("sync.needsYou.accountChanged", "You’re signed in to a different iCloud account, and this collection was not sent to it."),
        ("sync.needsYou.missing", "This collection is missing from iCloud, and it is still on this device."),
        ("sync.needsYou.deleted", "Meguri’s iCloud data was deleted, and your collection is still on this device."),
        ("sync.needsYou.encryptionReset", "The iCloud copy can no longer be read because your iCloud encrypted data was reset."),
        ("sync.needsYou.rootUnreadable", "Meguri can’t read which collection is current in iCloud."),
        ("sync.needsYou.setUp", "iCloud isn’t set up for this collection yet."),
        ("sync.needsYou.mediaMissingInICloud", "Media files can’t sync because Meguri’s media data is missing from iCloud."),
        ("sync.needsYou.quotaFull", "Your iCloud storage is full, so changes stay on this device."),
        ("sync.needsYou.switchUnfinished", "Changing the sync service didn’t finish, so nothing syncs."),
        ("sync.needsYou.blocked", "Meguri can’t read this device’s sync settings, so nothing syncs, and Export Collection still saves everything."),
        ("sync.needsYou.meguri.signInStore", "Meguri can’t read or save your Meguri Sync sign-in on this device."),
        ("sync.needsYou.meguri.finishSetup", "The server accepted this device’s collection, but setup didn’t finish; finishing replaces any changes made here since the upload, and Meguri backs them up first."),
        ("sync.needsYou.meguri.localUnusable", "Meguri can’t open the collection on this device, so setup stopped."),
        ("sync.needsYou.meguri.activation", "Replacing this device’s collection didn’t finish, and your backup is kept."),
        ("sync.needsYou.meguri.signIn", "The server didn’t accept your sign-in, so Meguri Sync stopped."),
        ("sync.needsYou.meguri.identityChanged", "The server address or account changed, so you need to sign in again."),
        ("sync.needsYou.meguri.storage", "Meguri couldn’t save to this device, so setup stopped."),
        ("sync.needsYou.meguri.setupFailed", "Setting up Meguri Sync didn’t finish, and nothing was replaced."),
        ("sync.needsYou.meguri.chooseCollection", "Choose which collection to keep before syncing continues."),
        ("sync.needsYou.meguri.mediaStopped.download", "Downloading the media didn’t finish, so syncing hasn’t started."),
        ("sync.needsYou.meguri.mediaStopped", "Transferring the media didn’t finish, so syncing hasn’t started.")
    ]
    /// Needs you rows whose text is an earlier section's key.
    private static let needsYouReused = ["sync.status.unavailable.noAccount", "sync.status.unavailable.restricted",
        "sync.status.unavailable.setupNeeded", "sync.attention.collectionCorrupt", "sync.attention.trackingError",
        "sync.attention.signedOut", "sync.attention.accountChangedBack", "sync.attention.replacedElsewhere",
        "sync.attention.fileMismatch", "sync.attention.isolated", "sync.attention.chooseCollection",
        "sync.attention.crtMismatch", "sync.attention.adoptUnproven", "sync.attention.publishConflict",
        "sync.attention.publishQuota", "sync.attention.publishAccount", "sync.attention.publishWaiting",
        "sync.meguri.status.signedOut.detail"]

    // T19.8, T19.9: the keys that became unused, and the source check.

    private static let unused: [String] = [
        "sync.status.localOnly.title", "sync.status.paused.user", "sync.status.needsAttention.count",
        "sync.media.off", "sync.now.unavailable.notConnected", "sync.now.unavailable.paused", "sync.result.media.paused",
        "sync.attention.accountChanged", "sync.attention.missing", "sync.attention.deleted", "sync.attention.encryptionReset",
        "sync.attention.rootUnreadable", "sync.attention.conflicts", "sync.attention.mediaMissingInICloud",
        "sync.attention.quotaFull", "sync.attention.localStorageFull", "sync.attention.unsupportedSchema",
        "sync.attention.cloudError", "sync.attention.storageGuidance",
        "sync.action.connect", "sync.action.useICloud", "sync.action.replaceICloud", "sync.action.keepLocal",
        "sync.action.reviewConflicts", "sync.action.showDetails", "sync.action.syncNow", "sync.action.useThat",
        "sync.action.replaceAgain",
        "sync.decision.sheetTitle", "sync.decision.title.restore", "sync.decision.title.import", "sync.decision.title.none",
        "sync.decision.message", "sync.decision.replace", "sync.decision.replace.caption", "sync.decision.keepLocal",
        "sync.decision.keepLocal.caption", "sync.decision.notNow",
        "sync.connect.action", "sync.adopt.action", "sync.publish.action",
        "sync.settings.section", "sync.settings.media", "sync.settings.media.footer", "sync.settings.pause",
        "sync.settings.resumePause", "sync.settings.stop", "sync.settings.stop.title", "sync.settings.stop.message",
        "sync.settings.stop.confirm", "sync.settings.conflicts", "sync.settings.advanced",
        "sync.service.label", "sync.service.recovery", "sync.switch.title.localOnly", "sync.switch.confirm.localOnly",
        "sync.switch.next.iCloud", "sync.switch.next.meguri",
        "sync.meguri.signIn.footer", "sync.meguri.signOut", "sync.meguri.signOut.title", "sync.meguri.signOut.message",
        "sync.meguri.signOut.confirm", "sync.meguri.signingOut", "sync.meguri.signOut.failed", "sync.meguri.stop",
        "sync.meguri.status.connectionBlocked.detail", "sync.meguri.status.paused.title", "sync.meguri.status.paused.detail",
        "sync.meguri.status.on.title", "sync.meguri.lastExchange", "sync.meguri.status.pending.title",
        "sync.meguri.status.pending.detail", "sync.meguri.status.syncingCollection.title",
        "sync.meguri.status.syncingMedia.title", "sync.meguri.status.signInRequired.detail",
        "sync.meguri.round.fullSyncRequired", "sync.meguri.round.cancelled",
        "sync.meguri.setup.choose", "sync.meguri.setup.useDevice", "sync.meguri.setup.useServer",
        "sync.meguri.setup.download.detail", "sync.meguri.setup.download", "sync.meguri.setup.review",
        "sync.meguri.setup.error.cancelled", "sync.meguri.preview.title.download", "sync.meguri.preview.direction.download",
        "sync.meguri.preview.confirm.download",
        "sync.meguri.now.unavailable.paused", "sync.meguri.now.unavailable.signIn", "sync.meguri.now.unavailable.stopped",
        "sync.meguri.pause", "sync.meguri.resume", "sync.meguri.now.chooseCollection"
    ]
    /// "every `sync.connect.two.*`", "every `sync.connect.continue.*`", "every `sync.keepLocal.*`".
    private static let unusedPrefixes = ["sync.connect.two.", "sync.connect.continue.", "sync.keepLocal."]
    /// Not removed yet: the iCloud connect and fence sheets keep their earlier
    /// confirmed flow until V27.2 and V27.4 are built for iCloud. Only that one
    /// file may name them, and each must leave this list with its last use.
    private static let pendingICloudSetup: Set<String> = [
        "sync.decision.sheetTitle", "sync.decision.message", "sync.decision.replace", "sync.decision.replace.caption",
        "sync.decision.keepLocal", "sync.decision.keepLocal.caption", "sync.decision.notNow",
        "sync.connect.two.title", "sync.connect.two.message", "sync.connect.two.useICloud",
        "sync.connect.two.useICloud.caption", "sync.connect.two.replace", "sync.connect.two.replace.caption",
        "sync.connect.two.keepLocal",
        "sync.connect.continue.title", "sync.connect.continue.message", "sync.connect.continue.confirm"
    ]
    private static let pendingFile = "Features/Settings/Sync/CloudConnectionView.swift"
    /// Not removed yet: the Meguri setup preview's form for a device with no collection.
    private static let pendingMeguriSetup: Set<String> = ["sync.meguri.preview.title.download",
        "sync.meguri.preview.direction.download", "sync.meguri.preview.confirm.download"]
    private static let pendingMeguriFile = "Features/Settings/Sync/MeguriSyncSetupViews.swift"

    private static func repository() -> URL {
        var url = URL(filePath: #filePath)
        for _ in 0..<6 { url.deleteLastPathComponent() }
        return url
    }
    /// Every Swift source of the app: the package's modules and the app target.
    private static func appSources() throws -> [(path: String, text: String)] {
        let root = repository()
        var found: [(String, String)] = []
        for directory in ["app/Packages/MeguriKit/Sources", "app/Meguri"] {
            let base = root.appending(path: directory)
            let files = try #require(FileManager.default.enumerator(at: base, includingPropertiesForKeys: nil))
            for case let file as URL in files where file.pathExtension == "swift" {
                found.append((String(file.path.dropFirst(root.path.count + 1)), try String(contentsOf: file, encoding: .utf8)))
            }
        }
        return found
    }
    /// The string literals of a source that could be a sync key.
    private static func literals(_ text: String) -> Set<String> {
        var found: Set<String> = []
        for match in text.matches(of: /"(sync\.[A-Za-z0-9.]+)/) { found.insert(String(match.1)) }
        return found
    }
    private static func isUnused(_ key: String) -> Bool {
        unused.contains(key) || unusedPrefixes.contains { key.hasPrefix($0) }
    }

    @Test func noAppSourceNamesAnUnusedKey() throws {
        let sources = try Self.appSources()
        try #require(sources.count > 100)
        var pendingSeen: Set<String> = []
        for (path, text) in sources {
            for literal in Self.literals(text) {
                guard Self.isUnused(literal) else { continue }
                if path.hasSuffix(Self.pendingFile), Self.pendingICloudSetup.contains(literal) {
                    pendingSeen.insert(literal)
                } else {
                    Issue.record("\(path) names the unused key \(literal)")
                }
            }
        }
        // The exception is exact: a pending key no file names any more must be removed.
        #expect(pendingSeen == Self.pendingICloudSetup)
        // Interpolated keys: the service switch sheet cannot reach an unused one.
        for destination in ["iCloud", "meguri", "off"] {
            #expect(!Self.isUnused("sync.switch.title.\(destination)") && !Self.isUnused("sync.switch.confirm.\(destination)"))
        }
        // The Meguri setup preview still builds its download form's keys, until
        // MP6.3 row 1 downloads with no question. Only that sheet does.
        let downloadForm = sources.filter { $0.text.contains("\"download\"") && $0.text.contains("sync.meguri.preview.") }.map(\.path)
        #expect(downloadForm == ["app/Packages/MeguriKit/Sources/\(Self.pendingMeguriFile)"])
    }

    @Test func unusedKeysAreRemovedFromTheCatalog() throws {
        let url = Self.repository().appending(path: "app/Packages/MeguriKit/Sources/AppModel/Sync/Resources/Sync.xcstrings")
        let object = try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any]
        let strings = try #require(object?["strings"] as? [String: Any])
        try #require(strings.count > 300)
        let left = strings.keys.filter(Self.isUnused).sorted()
        #expect(Set(left) == Self.pendingICloudSetup.union(Self.pendingMeguriSetup), "\(left)")
        // The keys this unit added outside T19's tables.
        #expect(SyncText.value("sync.status.openSettings") == "Sync Settings")
        #expect(SyncText.value("sync.attention.adoptWaiting") == "Replacing this collection with the iCloud collection was not finished.")
    }

    @Test func everyT19KeyHasItsEnglishValue() {
        for (key, english) in Self.t19 + Self.needsYou { #expect(SyncText.value(key) == english, "\(key)") }
        #expect(SyncText.format("sync.issues.row", Int64(1)) == "1 sync issue")
        #expect(SyncText.format("sync.issues.row", Int64(3)) == "3 sync issues")
        #expect(SyncText.format("sync.meguri.media.left", Int64(1)) == "1 media file is left. Syncing starts when it finishes.")
        #expect(SyncText.format("sync.meguri.media.left", Int64(12)) == "12 media files are left. Syncing starts when they finish.")
    }

    @Test func everyNeedsYouTextIsOneSentence() {
        let marks: Set<Character> = [".", "!", "?"]
        for key in Self.needsYou.map(\.0) + Self.needsYouReused {
            let text = SyncText.value(key)
            #expect(text != key, "\(key)")
            #expect(text.last.map(marks.contains) == true, "\(key)")
            #expect(text.filter(marks.contains).count == 1, "\(key): \(text)")
        }
    }
}
