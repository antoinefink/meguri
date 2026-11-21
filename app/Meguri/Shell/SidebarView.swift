import AppModel
import DesignSystem
import SwiftUI
#if os(macOS)
import AppKit
#endif

/// The sidebar (R17): Home, Browse, Stats, Note Types and Settings, then the
/// "Decks" group with the collection's deck tree. One selection covers both
/// groups: a section row shows that section, a deck row its page.
struct SidebarView: View {
    let navigation: AppNavigation
    let features: ShellFeatures

    var body: some View {
        // N20.1: the deck search's field tops the sidebar; its results replace this list.
        features.sidebarSearch(AnyView(list))
            .sidebarColumnChrome()
            .navigationTitle(Text(verbatim: "Meguri"))
            #if !os(macOS)
            // N14.9: the column keeps its name for accessibility; the bar does not draw it.
            .toolbar(removing: .title)
            #endif
    }

    private var list: some View {
        List(selection: Binding<SidebarItem?>(get: { navigation.sidebarSelection }, set: { item in
            if let item { navigation.select(item) }
        })) {
            Section {
                ForEach(features.availableSections, id: \.self) { section in
                    Button {
                        navigation.select(.section(section))
                    } label: {
                        Label {
                            // Rows grow with the text size rather than cut it.
                            Text(homeTitle(section))
                                .fixedSize(horizontal: false, vertical: true)
                        } icon: {
                            // The title names the row; the symbol is decoration.
                            Image(systemName: homeSymbol(section))
                                .accessibilityHidden(true)
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("shell.root.sidebar.\(section.rawValue)")
                    .accessibilityAddTraits(navigation.sidebarSelection == .section(section) ? .isSelected : [])
                    .tag(SidebarItem.section(section))
                }
            }
            if features.nativeMacWorkspace {
                // N14.6: a system section label under Today.
                Section {
                    features.sidebarDecks(navigation)
                } header: {
                    Text("Decks")
                        .accessibilityIdentifier("shell.root.sidebarDecks")
                }
            } else {
                Section {
                    features.sidebarDecks(navigation)
                } header: {
                    // The system's header grey falls short of 4.5:1 on the sidebar.
                    Text("Decks")
                        .foregroundStyle(.token(.textSecondary))
                        .accessibilityIdentifier("shell.root.sidebarDecks")
                }
            }
        }
        #if os(macOS)
        // H12: in the Mac's sidebar layout Return starts the selected deck's review, or
        // Today's Study with Today selected, whether the sidebar or the page beside it has
        // focus (H9.2 gives Return the same meaning on a deck page). A row's click leaves
        // focus with SwiftUI's button, not the list, so neither the table nor a primary
        // action sees the key: a key monitor of this window does. Never while a text input,
        // a sheet or a review has the keys, and never on a key's repeat. S is not bound on
        // the Mac sidebar: the list's type-select keeps the letters (H12).
        .background(ReturnKeyStudyMonitor { navigation.review == nil && navigation.sheet == nil
            && !textInputHasFocus && features.sidebarStudy(navigation.sidebarSelection) })
        #else
        // H12: with the list focused (a hardware keyboard), Return or S starts the selected
        // deck's review, or Today's Study on the Today row; once per press, never on a key's
        // repeat, and never while a text input (the deck search field) has focus (R197).
        .onKeyPress(keys: [.return, KeyEquivalent("s")], phases: .down) { press in
            guard press.modifiers.isEmpty, !textInputHasFocus,
                  features.sidebarStudy(navigation.sidebarSelection) else { return .ignored }
            return .handled
        }
        #endif
    }

    private var textInputHasFocus: Bool {
        #if os(macOS)
        NSApp.keyWindow?.firstResponder is NSText
        #else
        TextInputFocus.shared.editing
        #endif
    }

    /// N14.1: with a collection, the first row is Today on every sidebar layout.
    private func homeTitle(_ section: AppSection) -> LocalizedStringResource {
        guard section == .home else { return section.title }
        return features.todayRoot ? "Today" : section.title
    }

    private func homeSymbol(_ section: AppSection) -> String {
        guard section == .home else { return section.systemImage }
        return features.todayRoot ? "sun.max" : section.systemImage
    }
}

#if os(macOS)
/// H12: Return in the main window's sidebar layout, through a local key monitor that
/// lives as long as the sidebar. `act` returns whether it used the key.
private struct ReturnKeyStudyMonitor: View {
    let act: @MainActor () -> Bool
    @State private var monitor: Any?
    var body: some View {
        Color.clear
            .onAppear {
                guard monitor == nil else { return }
                monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
                    guard !event.isARepeat, event.keyCode == 36 || event.keyCode == 76,
                          event.modifierFlags.intersection(.deviceIndependentFlagsMask).subtracting([.numericPad, .function]).isEmpty,
                          let window = event.window, window === NSApp.mainWindow, window.attachedSheet == nil,
                          MainActor.assumeIsolated({ act() }) else { return event }
                    return nil
                }
            }
            .onDisappear {
                if let monitor { NSEvent.removeMonitor(monitor) }
                monitor = nil
            }
    }
}
#endif

