import SwiftUI

struct Welcome: View {
    var body: some View {
        VStack {
            Image("AppLogo")
            Image(systemName: "rectangle.stack")
            Image(decorative: "Wave")
            Label("Import", image: "ImportGlyph")
            Label("Sync", systemImage: "arrow.triangle.2.circlepath")
        }
    }
}
