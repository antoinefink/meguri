import SwiftUI

// The only place Meguri applies Liquid Glass and materials (R1996, KTD20).
// Glass belongs to the navigation and control layer: system bars (which bring
// their own), the answer button bar and floating controls. Card faces, list
// rows and other content never get glass, and glass is never stacked on
// glass: a control bar is one container, and a floating control sits on
// content, not on another glass element. The `glass-outside-chrome` lint rule
// fails glass anywhere else, unless tools/lint-ui/allowlist.tsv names the file
// with a reason.

/// How the control layer is drawn. `.flat` is a test seam for renders that
/// cannot draw Liquid Glass: the Mac's off-screen snapshots (U10) draw no
/// backdrop, so glass buttons there would lose their labels. Flat chrome
/// keeps every control and label on an opaque surface instead. The app never
/// sets it.
public enum ChromeGlass: Sendable {
    case liquid, flat
}

extension EnvironmentValues {
    @Entry public var chromeGlass: ChromeGlass = .liquid
}

/// A bar of controls floating over content, such as the answer button bar:
/// one glass container whose buttons merge into a single glass shape.
public struct ControlBar<Content: View>: View {
    @Environment(\.chromeGlass) private var chromeGlass
    private let content: Content

    public init(@ViewBuilder content: () -> Content) {
        self.content = content()
    }

    public var body: some View {
        switch chromeGlass {
        case .liquid:
            GlassEffectContainer(spacing: Spacing.small) {
                HStack(spacing: Spacing.small) {
                    content
                }
            }
            .buttonStyle(.glass)
        case .flat:
            HStack(spacing: Spacing.small) {
                content
            }
            .buttonStyle(.bordered)
        }
    }
}

/// Glass in a shape, or an opaque surface under flat chrome.
private struct FloatingChrome<ChromeShape: Shape>: ViewModifier {
    @Environment(\.chromeGlass) private var chromeGlass
    let shape: ChromeShape

    func body(content: Content) -> some View {
        switch chromeGlass {
        case .liquid: content.glassEffect(.regular, in: shape)
        case .flat: content.background(.token(.surface), in: shape)
        }
    }
}

/// A glass button, or a bordered one under flat chrome.
private struct ChromeButton: ViewModifier {
    @Environment(\.chromeGlass) private var chromeGlass
    let prominent: Bool

    func body(content: Content) -> some View {
        switch (chromeGlass, prominent) {
        case (.liquid, true): content.buttonStyle(.glassProminent)
        case (.liquid, false): content.buttonStyle(.glass)
        case (.flat, true): content.buttonStyle(.borderedProminent)
        case (.flat, false): content.buttonStyle(.bordered)
        }
    }
}

extension View {
    /// A floating control or message over content (toast, floating button):
    /// glass in a capsule.
    public func floatingChrome() -> some View {
        modifier(FloatingChrome(shape: Capsule()))
    }

    /// A floating control or message over content, in `shape`.
    public func floatingChrome(in shape: some Shape) -> some View {
        modifier(FloatingChrome(shape: shape))
    }

    /// A glass button for the control layer; `prominent` for the primary
    /// action of a group.
    public func chromeButtonStyle(prominent: Bool = false) -> some View {
        modifier(ChromeButton(prominent: prominent))
    }
}
