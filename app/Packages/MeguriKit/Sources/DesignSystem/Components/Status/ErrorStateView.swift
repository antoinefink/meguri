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
    #if DEBUG
    private var retryDiagnosticValue: String?

    /// Apply a category diagnostic to the retry button itself, never its container.
    public func debugRetryAccessibilityValue(_ value: String) -> Self {
        var view = self
        view.retryDiagnosticValue = value
        return view
    }
    #endif

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
                // Text tokens: the system's greys fall below 4.5:1 on white.
                title.foregroundStyle(.token(.textPrimary))
            } icon: {
                Image(systemName: "exclamationmark.triangle")
                    .foregroundStyle(.token(.statusError))
            }
        } description: {
            message.foregroundStyle(.token(.textSecondary))
        } actions: {
            if let retry {
                let button = Button(action: retry.action) {
                    Text("Try Again", bundle: .module)
                }
                .accessibilityIdentifier(retry.identifier)
                .stateViewActions()
                #if os(macOS)
                .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget)
                .contentShape(Rectangle())
                #endif
                #if DEBUG
                if let retryDiagnosticValue {
                    button.accessibilityValue(Text(verbatim: retryDiagnosticValue))
                } else {
                    button
                }
                #else
                button
                #endif
            }
        }
        // The Mac sizes the view to its content: centred in its container
        // on every platform.
        .frame(maxWidth: .infinity)
    }
}

extension View {
    /// Native state actions: regular on Mac, large with audited text contrast
    /// on touch platforms.
    func stateViewActions() -> some View {
        #if os(macOS)
        buttonStyle(.bordered).controlSize(.regular)
        #else
        buttonStyle(.bordered).controlSize(.large).tint(ColorToken.textPrimary.color)
        #endif
    }
}
