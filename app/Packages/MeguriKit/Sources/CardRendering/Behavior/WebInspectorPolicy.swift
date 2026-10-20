import Foundation

public nonisolated enum CardInspectionBuild: String, CaseIterable, Sendable {
    case debug, profile, personalRelease, appStore, unknown
    public var defaultEnabled: Bool { self == .personalRelease }
}

public nonisolated struct WebInspectorPolicy: Equatable, Sendable {
    public var build: CardInspectionBuild
    public var enabled: Bool
    public init(build: CardInspectionBuild = .unknown, enabled: Bool? = nil) {
        self.build = build
        self.enabled = enabled ?? build.defaultEnabled
    }
    public var isInspectable: Bool {
        enabled && [.debug, .profile, .personalRelease].contains(build)
    }
}
