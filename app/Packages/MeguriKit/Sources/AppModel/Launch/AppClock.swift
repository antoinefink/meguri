import Foundation
import SwiftUI

/// The app's source of the current instant (KTD18 clock seam). Views and
/// stores read time through it, never through `Date()`, so the test hooks can
/// stop the clock at a fixed instant (R1963).
nonisolated public struct AppClock: Sendable {
    private let source: @Sendable () -> Date
    public let isFixed: Bool

    public init(_ source: @escaping @Sendable () -> Date) {
        self.source = source
        isFixed = false
    }

    public func now() -> Date {
        source()
    }

    private init(instant: Date) { source = { instant }; isFixed = true }

    /// The wall clock.
    public static let system = AppClock { Date() }

    /// A clock that stays at `instant`.
    public static func fixed(_ instant: Date) -> AppClock {
        AppClock(instant: instant)
    }
}

extension EnvironmentValues {
    /// The clock views read the current instant from; the root view injects
    /// the launch configuration's.
    @Entry public var appClock: AppClock = .system
}
