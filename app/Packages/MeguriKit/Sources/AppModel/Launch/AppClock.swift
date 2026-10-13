import Foundation
import SwiftUI

/// The app's source of the current instant (KTD18 clock seam). Views and
/// stores read time through it, never through `Date()`, so the test hooks can
/// stop the clock at a fixed instant (R1963).
nonisolated public struct AppClock: Sendable {
    private let source: @Sendable () -> Date

    public init(_ source: @escaping @Sendable () -> Date) {
        self.source = source
    }

    public func now() -> Date {
        source()
    }

    /// The wall clock.
    public static let system = AppClock { Date() }

    /// A clock that stays at `instant`.
    public static func fixed(_ instant: Date) -> AppClock {
        AppClock { instant }
    }
}

extension EnvironmentValues {
    /// The clock views read the current instant from; the root view injects
    /// the launch configuration's.
    @Entry public var appClock: AppClock = .system
}
