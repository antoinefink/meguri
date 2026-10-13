import SwiftUI

struct BottomTabBar: View {
    var body: some View { EmptyView() }
}

struct SettingsNavBar: View {
    var body: some View { EmptyView() }
}

struct SettingsMenu: View {
    @State private var showsHamburger = false

    var body: some View {
        Button {} label: { Image(systemName: "line.3.horizontal") }
        Button {} label: { Image(systemName: "line.3.horizontal.decrease") }
    }
}
