import Foundation
import SwiftUI

/// What the app takes from its launch (KTD18 seams). Normal launches use the
/// defaults; in Debug and Profile builds the app's test hooks override them
/// from the launch arguments before the first scene appears (R1963). Nothing
/// here knows about the hooks.
public struct LaunchConfiguration {
    public var nativeReviewInput: NativeReviewInput?
    public var clock: AppClock = .system
    public var collectionLocation: CollectionLocation = .standard
    /// Overrides of the system time zone and locale; nil keeps the system's.
    public var timeZone: TimeZone?
    public var locale: Locale?
    /// Ports for the Mac's local servers; nil keeps each server's default,
    /// so tests never bind the ports a running daily-driver app holds.
    public var ankiConnectPort: UInt16?
    public var migakuPort: UInt16?
    /// A screen, sheet or alert the root shows at launch; nil shows the
    /// navigation's own state.
    public var initialPresentation: InitialPresentation?

    public init() {}
}

/// A screen, sheet or alert shown over the root at launch.
public struct InitialPresentation {
    public enum Style: Sendable, Hashable {
        case screen, sheet, alert
    }

    public let style: Style
    /// The view of a screen or sheet.
    public let content: AnyView?
    public let alertTitle: String?
    public let alertMessage: String?
    /// The buttons of an alert.
    public let alertActions: AnyView?

    /// A screen shown on its own, in place of the app's navigation.
    public static func screen<Content: View>(@ViewBuilder _ content: () -> Content) -> InitialPresentation {
        InitialPresentation(style: .screen, content: AnyView(content()), alertTitle: nil, alertMessage: nil, alertActions: nil)
    }

    /// A sheet over the root.
    public static func sheet<Content: View>(@ViewBuilder _ content: () -> Content) -> InitialPresentation {
        InitialPresentation(style: .sheet, content: AnyView(content()), alertTitle: nil, alertMessage: nil, alertActions: nil)
    }

    /// An alert over the root, for error states. `actions` are its buttons
    /// (retry, cancel…); given none, the system would add a bare OK.
    public static func alert<Actions: View>(
        title: String, message: String, @ViewBuilder actions: () -> Actions
    ) -> InitialPresentation {
        InitialPresentation(style: .alert, content: nil, alertTitle: title, alertMessage: message, alertActions: AnyView(actions()))
    }
}
