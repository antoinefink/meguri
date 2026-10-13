import SwiftUI

struct DeckCard: View {
    @State private var expanded = false

    var body: some View {
        Text("Deck")
            .clipShape(.rect(cornerRadius: 12))
            .background(RoundedRectangle(cornerRadius: 0))
            .animation(.easeInOut(duration: 0.3), value: expanded)
            .transition(.move(edge: .bottom))
            .onTapGesture {
                withAnimation(.spring) { expanded.toggle() }
            }
    }
}
