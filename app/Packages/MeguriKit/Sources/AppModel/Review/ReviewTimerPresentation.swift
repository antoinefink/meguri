import Foundation

/// A sample of the answer clock, never an independently advancing clock.
public nonisolated struct ReviewTimerPresentation: Equatable, Sendable {
    /// Identifies the retained clock's start/resume state without advancing it.
    public nonisolated struct ClockIdentity: Hashable, Sendable {
        let began: TimeInterval?
        let accumulated: TimeInterval
    }

    public let clockIdentity: ClockIdentity?
    public let milliseconds: UInt64
    public let capSeconds: UInt32
    public let isVisible: Bool
    public let isCounting: Bool

    public init(milliseconds: UInt64, capSeconds: UInt32, isVisible: Bool, isCounting: Bool,
                clockIdentity: ClockIdentity? = nil) {
        self.clockIdentity = clockIdentity
        self.milliseconds = min(milliseconds, UInt64(capSeconds) * 1_000)
        self.capSeconds = capSeconds
        self.isVisible = isVisible
        self.isCounting = isVisible && isCounting && milliseconds < UInt64(capSeconds) * 1_000
    }

    public var seconds: UInt64 { milliseconds / 1_000 }
    public var capped: Bool { seconds >= UInt64(capSeconds) }
    public var text: String { String(format: "%02llu:%02llu", seconds / 60, seconds % 60) }
}
