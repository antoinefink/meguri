import SwiftUI

struct RowLayout: View {
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Front")
                .padding(12)
            Text("Back")
                .padding(.horizontal, 0)
                .padding([.top, .bottom], 6.5)
            HStack(spacing: 0) {
                Spacer(minLength: 4)
            }
            .padding(EdgeInsets(top: 0, leading: 10, bottom: 0, trailing: 0))
            .padding()
        }
    }
}
