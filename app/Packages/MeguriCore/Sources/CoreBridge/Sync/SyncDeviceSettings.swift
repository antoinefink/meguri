/// Persisted device settings shared by Off and Meguri Sync. Installed legacy
/// settings keep the same fields; media is normalized on by the coordinator.
public struct SyncDeviceSettings: Sendable, Equatable {
    public let enabled: Bool
    public let paused: Bool
    public let automatic: Bool
    public let media: Bool
    public init(enabled: Bool, paused: Bool, automatic: Bool, media: Bool) {
        self.enabled = enabled; self.paused = paused; self.automatic = automatic; self.media = media
    }
}
public enum SyncSource: Sendable, Hashable { case command, key, ankiConnect }
public enum SyncUnavailable: Sendable, Hashable {
    case collectionUnavailable, reviewActive, notConnected, paused, decisionRequired, recoveryRequired
}
public enum SyncNowAvailability: Sendable, Equatable {
    case available, unavailable(SyncUnavailable)
}
