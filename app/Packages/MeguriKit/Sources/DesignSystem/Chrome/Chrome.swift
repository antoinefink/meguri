import SwiftUI

// The only place Meguri applies Liquid Glass and materials (R1996, KTD20).
// Glass belongs to the navigation and control layer: system bars (which bring
// their own), the answer button bar and floating controls. Card faces, list
// rows and other content never get glass, and glass is never stacked on
// glass: a control bar is one container, and a floating control sits on
// content, not on another glass element. The `glass-outside-chrome` lint rule
// fails glass anywhere else, unless tools/lint-ui/allowlist.tsv names the file
// with a reason.

/// A bar of controls floating over content, such as the answer button bar:
/// one glass container whose buttons merge into a single glass shape.
public struct ControlBar<Content: View>: View {
    private let content: Content

    public init(@ViewBuilder content: () -> Content) {
        self.content = content()
    }

    public var body: some View {
        GlassEffectContainer(spacing: Spacing.small) {
            HStack(spacing: Spacing.small) {
                content
            }
        }
        .buttonStyle(.glass)
    }
}

extension View {
    /// A floating control or message over content (toast, floating button):
    /// glass in a capsule.
    public func floatingChrome() -> some View {
        glassEffect(.regular, in: .capsule)
    }

    /// A floating control or message over content, in `shape`.
    public func floatingChrome(in shape: some Shape) -> some View {
        glassEffect(.regular, in: shape)
    }

    /// A glass button for the control layer; `prominent` for the primary
    /// action of a group.
    @ViewBuilder
    public func chromeButtonStyle(prominent: Bool = false) -> some View {
        if prominent {
            buttonStyle(.glassProminent)
        } else {
            buttonStyle(.glass)
        }
    }
}
