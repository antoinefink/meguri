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
        ("sync.meguri.now.unavailable.stopped.sync", "Meguri Sync is stopped. Open Sync settings to see what to do.")
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
        ("sync.needsYou.meguri.finishSetup", "The server accepted this device’s collection, but setup didn’t finish on this device."),
        ("sync.needsYou.meguri.activation", "Replacing this device’s collection didn’t finish, and your backup is kept."),
        ("sync.needsYou.meguri.signIn", "The server didn’t accept your sign-in, so Meguri Sync stopped."),
        ("sync.needsYou.meguri.identityChanged", "The server address or account changed, so you need to sign in again."),
        ("sync.needsYou.meguri.storage", "Meguri couldn’t save to this device, so setup stopped."),
        ("sync.needsYou.meguri.setupFailed", "Setting up Meguri Sync didn’t finish, and nothing was replaced."),
        ("sync.needsYou.meguri.chooseCollection", "Choose which collection to keep before syncing continues.")
    ]
    /// Needs you rows whose text is an earlier section's key.
    private static let needsYouReused = ["sync.status.unavailable.noAccount", "sync.status.unavailable.restricted",
        "sync.status.unavailable.setupNeeded", "sync.attention.collectionCorrupt", "sync.attention.trackingError",
        "sync.attention.signedOut", "sync.attention.accountChangedBack", "sync.attention.replacedElsewhere",
        "sync.attention.fileMismatch", "sync.attention.isolated", "sync.attention.chooseCollection",
        "sync.attention.crtMismatch", "sync.attention.adoptUnproven", "sync.attention.publishConflict",
        "sync.attention.publishQuota", "sync.attention.publishAccount", "sync.attention.publishWaiting",
        "sync.meguri.status.signedOut.detail"]

    @Test func everyT19KeyHasItsEnglishValue() {
        for (key, english) in Self.t19 + Self.needsYou { #expect(SyncText.value(key) == english, "\(key)") }
        #expect(SyncText.format("sync.issues.row", Int64(1)) == "1 sync issue")
        #expect(SyncText.format("sync.issues.row", Int64(3)) == "3 sync issues")
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
