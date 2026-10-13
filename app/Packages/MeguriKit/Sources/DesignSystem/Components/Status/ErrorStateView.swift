import SwiftUI

/// What a screen or section shows when loading its content failed: the system
/// `ContentUnavailableView` with a warning symbol, the message and, when the
/// failure can be retried, a Try Again button.
public struct ErrorStateView: View {
    private let title: Text
    private let message: Text
    private let retry: (() -> Void)?

    public init(_ title: Text, message: Text, retry: (() -> Void)? = nil) {
        self.title = title
        self.message = message
        self.retry = retry
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
                Button(action: retry) {
                    Text("Try Again", bundle: .module)
                }
            }
        }
    }
}
