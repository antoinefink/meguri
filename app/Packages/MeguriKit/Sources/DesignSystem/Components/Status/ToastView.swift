import SwiftUI

/// A short message floating over content ("Card buried", "3 cards added"),
/// on the floating control layer (glass from `Chrome/`, R1996). The view only
/// draws the toast; when it appears, how long it stays and its VoiceOver
/// announcement belong to the presenter.
public struct ToastView: View {
    public enum Style: Sendable {
        case info
        case error
    }

    private let message: Text
    private let systemImage: String?
    private let style: Style

    public init(_ message: Text, systemImage: String? = nil, style: Style = .info) {
        self.message = message
        self.systemImage = systemImage
        self.style = style
    }

    public var body: some View {
        HStack(spacing: Spacing.xSmall) {
            if let systemImage {
                Image(systemName: systemImage)
                    .foregroundStyle(.token(style == .error ? .statusError : .textPrimary))
                    .accessibilityHidden(true)
            }
            message
                .foregroundStyle(.token(.textPrimary))
                .multilineTextAlignment(.leading)
        }
        .font(.subheadline)
        .padding(.horizontal, Spacing.medium)
        .padding(.vertical, Spacing.small)
        .floatingChrome(in: Radius.rounded(Radius.large))
        .accessibilityElement(children: .combine)
    }
}
