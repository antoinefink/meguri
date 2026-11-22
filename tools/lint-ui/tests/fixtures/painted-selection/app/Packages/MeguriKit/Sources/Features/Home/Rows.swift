import SwiftUI

struct Rows: View {
    let selected: Bool
    @State private var selection: Int?
    var body: some View {
        List(selection: $selection) {
            Text("Today").listRowBackground(selected ? Color.gray : nil)
            Text("Decks").listRowBackground(isSelected ? Color.gray : nil)
            Text("Wrapped").listRowBackground(
                selected ? Color.gray : nil
            )
            Text("Plain").tag(1)
            Text("Asset").listRowBackground(Color("selection"))
            Text("Clear").listRowBackground(Color.clear)
            // .listRowBackground(selected ? Color.gray : nil) in a comment is not code.
        }
    }
    private var isSelected: Bool { false }
}
