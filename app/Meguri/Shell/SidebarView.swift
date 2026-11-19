import AppModel
import DesignSystem
import SwiftUI

/// The sidebar (R17): Home, Browse, Stats, Note Types and Settings, then the
/// "Decks" group with the collection's deck tree. One selection covers both
/// groups: a section row shows that section, a deck row its page.
struct SidebarView: View {
    let navigation: AppNavigation
    let features: ShellFeatures

    var body: some View {
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
        .sidebarColumnChrome()
        .navigationTitle(Text(verbatim: "Meguri"))
        #if !os(macOS)
        // N14.9: the column keeps its name for accessibility; the bar does not draw it.
        .toolbar(removing: .title)
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
