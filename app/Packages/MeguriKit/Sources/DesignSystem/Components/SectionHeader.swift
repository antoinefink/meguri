import SwiftUI

/// The title of a content section outside a system `List`/`Form` (whose
/// `Section` headers come first, R1995): a headline, an optional secondary
/// line and an optional trailing accessory such as a button or menu. Long
/// titles wrap; they are never truncated.
public struct SectionHeader<Accessory: View>: View {
    private let title: Text
    private let subtitle: Text?
    private let accessory: Accessory

    public init(_ title: Text, subtitle: Text? = nil, @ViewBuilder accessory: () -> Accessory) {
        self.title = title
        self.subtitle = subtitle
        self.accessory = accessory()
    }

    public var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: Spacing.small) {
            VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                title
                    .font(Typography.sectionTitle)
                    .foregroundStyle(.token(.textPrimary))
                    .accessibilityAddTraits(.isHeader)
                if let subtitle {
                    subtitle
                        .font(Typography.sectionSubtitle)
                        .foregroundStyle(.token(.textSecondary))
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            accessory
        }
    }
}

extension SectionHeader where Accessory == EmptyView {
    public init(_ title: Text, subtitle: Text? = nil) {
        self.init(title, subtitle: subtitle) { EmptyView() }
    }
}
