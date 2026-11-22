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

/// Glass in a shape, or an opaque surface under flat chrome. The flat
/// surface is the grouped background, which differs from the screen
/// background in light and dark (`surface` is the screen's white in light,
/// so a toast would lose its container).
private struct FloatingChrome<ChromeShape: Shape>: ViewModifier {
    @Environment(\.chromeGlass) private var chromeGlass
    let shape: ChromeShape

    func body(content: Content) -> some View {
        switch chromeGlass {
        case .liquid: content.glassEffect(.regular, in: shape)
        case .flat: content.background(.token(.backgroundSecondary), in: shape)
        }
    }
}

/// A small panel of transient information over content, such as a chart's
/// selection callout (contract N25.4): the system's regular material, or the
/// grouped background under flat chrome. A panel, not a control: no glass.
private struct MaterialPanel<PanelShape: Shape>: ViewModifier {
    @Environment(\.chromeGlass) private var chromeGlass
    let shape: PanelShape

    func body(content: Content) -> some View {
        switch chromeGlass {
        case .liquid: content.background(.regularMaterial, in: shape)
        case .flat: content.background(.token(.backgroundSecondary), in: shape)
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

/// A split view's column under flat chrome: an opaque surface where the
/// system draws its sidebar material or window background, which the Mac's
/// off-screen renders cannot draw.
private struct SplitColumnChrome: ViewModifier {
    @Environment(\.chromeGlass) private var chromeGlass
    let sidebar: Bool

    func body(content: Content) -> some View {
        switch chromeGlass {
        case .liquid:
            content
        case .flat where sidebar:
            // The sidebar list style draws its selection through a material
            // (black off-screen) and its text vibrant (dark off-screen, also
            // in dark mode): an inset list in text tokens draws both
            // plainly.
            content
                .listStyle(.inset)
                .scrollContentBackground(.hidden)
                .foregroundStyle(.token(.textPrimary))
                .background(.token(.backgroundSecondary))
        case .flat:
            content
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(.token(.backgroundPrimary))
        }
    }
}

extension View {
    /// A split view's sidebar column: the system's sidebar material, or an
    /// opaque surface under flat chrome.
    public func sidebarColumnChrome() -> some View {
        modifier(SplitColumnChrome(sidebar: true))
    }

    /// A split view's detail column: the system's background, or an opaque
    /// one under flat chrome.
    public func detailColumnChrome() -> some View {
        modifier(SplitColumnChrome(sidebar: false))
    }

    /// A floating control or message over content (toast, floating button):
    /// glass in a capsule.
    public func floatingChrome() -> some View {
        modifier(FloatingChrome(shape: Capsule()))
    }

    /// A floating control or message over content, in `shape`.
    public func floatingChrome(in shape: some Shape) -> some View {
        modifier(FloatingChrome(shape: shape))
    }

    /// A small panel of transient information over content (a chart's
    /// callout): the regular material in `shape`.
    public func materialPanel(in shape: some Shape) -> some View {
        modifier(MaterialPanel(shape: shape))
    }

    /// A glass button for the control layer; `prominent` for the primary
    /// action of a group.
    public func chromeButtonStyle(prominent: Bool = false) -> some View {
        modifier(ChromeButton(prominent: prominent))
    }
}
