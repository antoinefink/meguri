import DesignSystem
import SwiftUI

struct DeckSummary: View {
    @State private var count = 0

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.small) {
            SectionHeader(Text("Spanish"))
            CountColumns(new: count, learn: 0, due: 3)
                .motion(Motion.quick, value: count)
            Text("Due today")
                .font(.headline)
                .foregroundStyle(.token(.textSecondary))
                .padding(.horizontal, Spacing.medium)
            Image(systemName: "rectangle.stack")
            ControlBar {
                Button("Study") {}
            }
            ToastView(Text("Card buried"))
                .motionTransition(.move(edge: .bottom), Motion.standard)
        }
        .padding(Spacing.medium)
        .background(.token(.surface), in: Radius.rounded(Radius.medium))
        .clipShape(Radius.concentric)
        .frame(minWidth: 320, minHeight: 44)
    }
}
