import AppModel
import CoreBridge
import SwiftUI

struct CloudSyncCommands: Commands {
    let collection: ReviewCollection?
    var body: some Commands {
        CommandGroup(after: .importExport) {
            Button(SyncText.value("sync.now.command")) {
                guard let collection else { return }
                collection.requestSyncNow(showStatus: collection.syncPresentation.showStatusForCommands)
            }
            .keyboardShortcut("y", modifiers: [.command, .shift])
            .disabled(collection?.syncNowAvailability() != .available || collection?.navigation.sheet != nil)
            .accessibilityIdentifier("sync.now.command")
        }
    }
}
