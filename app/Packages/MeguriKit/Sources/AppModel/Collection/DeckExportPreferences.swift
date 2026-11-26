import CoreBridge
import Foundation

/// Installation-local, separate from collection export and synced collection config.
@MainActor struct DeckExportPreferences {
    let defaults: UserDefaults?
    private let key = "deckExport.latest.options"
    func read() -> DeckExportOptions {
        let values = defaults?.dictionary(forKey: key)
        return DeckExportOptions(includeScheduling: values?["scheduling"] as? Bool ?? false,
                                 includePresets: values?["presets"] as? Bool ?? false,
                                 includeMedia: values?["media"] as? Bool ?? true)
    }
    func write(_ options: DeckExportOptions) {
        defaults?.set(["scheduling": options.includeScheduling, "presets": options.includePresets,
                       "media": options.includeMedia], forKey: key)
    }
}
