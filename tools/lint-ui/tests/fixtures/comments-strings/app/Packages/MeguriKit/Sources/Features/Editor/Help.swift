import SwiftUI

/// Never write Color(red: 1, green: 0, blue: 0) or .padding(12) or .glassEffect() here.
struct Help: View {
    // A hamburger menu or a custom TabBar would be wrong; .shadow(radius: 2) too.
    var body: some View {
        Text("Use .font(.system(size: 17)) and Image(\"logo\") nowhere")
    }
}
