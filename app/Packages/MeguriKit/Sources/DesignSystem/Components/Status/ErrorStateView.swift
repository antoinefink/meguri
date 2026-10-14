import SwiftUI

/// What a screen or section shows when loading its content failed: the system
/// `ContentUnavailableView` with a warning symbol, the message and, when the
/// failure can be retried, a Try Again button. The button carries the
/// caller's accessibility identifier (R1964), so it can never be left without
/// one.
public struct ErrorStateView: View {
    private struct Retry {
        let identifier: String
        let action: () -> Void
    }

    private let title: Text
    private let message: Text
    private let retry: Retry?

    /// A failure that cannot be retried.
    public init(_ title: Text, message: Text) {
        self.title = title
        self.message = message
        retry = nil
    }

    /// A failure with a Try Again button whose identifier is
    /// `retryIdentifier` (`area.screen.retry`).
    public init(_ title: Text, message: Text, retryIdentifier: String, retry: @escaping () -> Void) {
        self.title = title
        self.message = message
        self.retry = Retry(identifier: retryIdentifier, action: retry)
    }

    public var body: some View {
        ContentUnavailableView {
            Label {
                title
            } icon: {
                Image(systemName: "exclamationmark.triangle")
                    .foregroundStyle(.token(.statusError))
            }
        } description: {
            message
        } actions: {
            if let retry {
                Button(action: retry.action) {
                    Text("Try Again", bundle: .module)
                }
                .accessibilityIdentifier(retry.identifier)
                .stateViewActions()
            }
        }
    }
}

extension View {
    /// The actions of an empty or error state: bordered, at the large control
    /// size, so each is a full touch target (R1980: 44×44 pt on iPhone and
    /// iPad, 28×28 pt on the Mac; the system's default actions are only as
    /// tall as their text), with the primary text colour, whose contrast on
    /// the bordered fill passes the audit where the accent colour does not.
    func stateViewActions() -> some View {
        buttonStyle(.bordered).controlSize(.large).tint(ColorToken.textPrimary.color)
    }
}
