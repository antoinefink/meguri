import SwiftUI

/// AE90: a change that puts a glass background on the card face.
struct CardFaceBackground: View {
    var body: some View {
        Color.clear
            .glassEffect(.regular, in: .rect)
    }
}
