#if os(macOS)
import AppKit
import AppModel
import SwiftUI

/// A main-sidebar row's button on the Mac, acting when the mouse goes down.
///
/// A native source list (AppKit's `NSTableView`) selects a row on mouse-down;
/// a SwiftUI `Button` acts on release, so the row's highlight waited for the
/// click's release. The label is drawn as `.plain` draws it, and the system
/// list draws the selection; only the moment the action runs changes. The
/// double-click, the context menu and keyboard selection keep their own paths.
public struct SidebarRowPressStyle: PrimitiveButtonStyle {
    public init() {}

    public func makeBody(configuration: Configuration) -> some View {
        Pressed(configuration: configuration)
    }

    private struct Pressed: View {
        let configuration: Configuration
        /// One action per press, however far the pointer then moves.
        @State private var down = false

        var body: some View {
            configuration.label
                .contentShape(Rectangle())
                .gesture(DragGesture(minimumDistance: 0)
                    .onChanged { _ in
                        guard !down else { return }
                        down = true
                        // A Control-click opens the row's menu, as in a source list: it
                        // does not select the row.
                        guard !NSEvent.modifierFlags.contains(.control) else { return }
                        configuration.trigger()
                    }
                    .onEnded { _ in down = false })
                // The gesture replaces the button's own: VoiceOver's press keeps the action.
                .accessibilityAction { configuration.trigger() }
        }
    }
}
/// A sidebar row's selected trait, in a modifier of its own: a press redraws
/// this modifier on every row, never the rows' bodies (their names, menus and
/// badges).
public struct SidebarSelectedTrait: ViewModifier {
    let navigation: AppNavigation?
    let item: SidebarItem

    public init(navigation: AppNavigation?, item: SidebarItem) {
        self.navigation = navigation
        self.item = item
    }

    public func body(content: Content) -> some View {
        content.accessibilityAddTraits(navigation?.displayedSidebarSelection == item ? .isSelected : [])
    }
}
#endif
