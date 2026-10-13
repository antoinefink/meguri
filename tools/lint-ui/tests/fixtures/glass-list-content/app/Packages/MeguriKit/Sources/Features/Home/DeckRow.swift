import SwiftUI

struct DeckRow: View {
    var body: some View {
        GlassEffectContainer {
            Text("Spanish")
                .background(.regularMaterial)
        }
        .buttonStyle(.glass)
    }
}
