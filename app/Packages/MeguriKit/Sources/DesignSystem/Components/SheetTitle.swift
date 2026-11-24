import SwiftUI

/// A sheet's title on the Mac, where a sheet has no title bar (N21.6): a
/// headline, and an optional secondary line, leading-aligned with the
/// content under it. Browse's command sheets put it above their form; a
/// grouped form puts it as its first section's header, so it starts where
/// the rows start. Long titles wrap; they are never truncated.
public struct SheetTitle: View {
    private let title: Text
    private let subtitle: Text?
    private let identifier: String?

    public init(_ title: Text, subtitle: Text? = nil, identifier: String? = nil) {
        self.title = title
        self.subtitle = subtitle
        self.identifier = identifier
    }

    private var heading: some View {
        title
            .font(.headline)
            .foregroundStyle(.primary)
            .fixedSize(horizontal: false, vertical: true)
            .accessibilityAddTraits(.isHeader)
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: Spacing.xxSmall) {
            if let identifier {
                heading.accessibilityIdentifier("\(identifier).title")
            } else {
                heading
            }
            if let subtitle {
                subtitle
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
        }
        .textCase(nil)
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}
