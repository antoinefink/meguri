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
                ForEach(AppSection.sidebar, id: \.self) { section in
                    Label {
                        // Rows grow with the text size rather than cut it.
                        Text(section.title)
                            .fixedSize(horizontal: false, vertical: true)
                            .accessibilityIdentifier("shell.root.sidebar.\(section.rawValue)")
                    } icon: {
                        // The title names the row; the symbol is decoration.
                        Image(systemName: section.systemImage)
                            .accessibilityHidden(true)
                    }
                    .tag(SidebarItem.section(section))
                }
            }
            Section {
                features.sidebarDecks(navigation)
            } header: {
                // The system's header grey falls short of 4.5:1 on the sidebar.
                Text("Decks")
                    .foregroundStyle(.token(.textSecondary))
                    .accessibilityIdentifier("shell.root.sidebarDecks")
            }
        }
        .sidebarColumnChrome()
        .navigationTitle(Text(verbatim: "Meguri"))
    }
}
