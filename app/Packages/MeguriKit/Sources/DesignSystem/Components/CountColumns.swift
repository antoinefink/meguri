import SwiftUI

/// The New, Learn and Due counts of a deck row or header (R32): New blue,
/// Learn red, Due green, a zero count dimmed. Colour is never the only cue
/// (R187): VoiceOver reads each count with its name, and `CountColumns.Header`
/// labels the columns on screen.
public struct CountColumns: View {
    public let new: Int
    public let learn: Int
    public let due: Int

    /// Wide enough for four digits at the current text size; wider counts
    /// widen their column.
    @ScaledMetric(relativeTo: .body) private var columnWidth: CGFloat = 44

    public init(new: Int, learn: Int, due: Int) {
        self.new = new
        self.learn = learn
        self.due = due
    }

    public var body: some View {
        HStack(spacing: Spacing.small) {
            column(new, .countNew)
            column(learn, .countLearn)
            column(due, .countDue)
        }
        .font(Typography.count)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text("New \(new), Learn \(learn), Due \(due)", bundle: .module))
    }

    private func column(_ value: Int, _ token: ColorToken) -> some View {
        Text(value, format: .number)
            .foregroundStyle(.token(value == 0 ? .countZero : token))
            .lineLimit(1)
            .frame(minWidth: columnWidth, alignment: .trailing)
    }

    /// Column titles, aligned with the counts below them.
    public struct Header: View {
        @ScaledMetric(relativeTo: .body) private var columnWidth: CGFloat = 44

        public init() {}

        public var body: some View {
            HStack(spacing: Spacing.small) {
                title(Text("New", bundle: .module))
                title(Text("Learn", bundle: .module))
                title(Text("Due", bundle: .module))
            }
            .font(Typography.countHeader)
            .foregroundStyle(.token(.textSecondary))
        }

        private func title(_ text: Text) -> some View {
            text
                .lineLimit(1)
                .frame(minWidth: columnWidth, alignment: .trailing)
                .accessibilityAddTraits(.isHeader)
        }
    }
}
