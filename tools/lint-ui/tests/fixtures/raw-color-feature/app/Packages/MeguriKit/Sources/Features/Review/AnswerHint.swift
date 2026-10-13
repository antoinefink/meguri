import SwiftUI

struct AnswerHint: View {
    var body: some View {
        Text("Again")
            .foregroundStyle(Color(red: 1, green: 0, blue: 0))
            .background(Color(white: 0.9))
            .overlay(Color(.sRGB, red: 0.2, green: 0.4, blue: 0.6))
            .border(Color(hex: "#FF0000"))
    }
}
