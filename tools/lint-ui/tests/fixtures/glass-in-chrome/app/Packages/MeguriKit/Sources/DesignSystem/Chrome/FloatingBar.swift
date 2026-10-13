import SwiftUI

struct FloatingBar: View {
    var body: some View {
        GlassEffectContainer {
            Button("Undo") {}
                .buttonStyle(.glass)
        }
        .glassEffect(.regular, in: .capsule)
        .shadow(radius: 2)
    }
}
