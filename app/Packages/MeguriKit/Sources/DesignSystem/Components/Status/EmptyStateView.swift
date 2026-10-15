import SwiftUI

/// What a screen or section shows when it has nothing to list: the system
/// `ContentUnavailableView` (R1995) with an SF Symbol, a title, a description
/// and an optional action, drawn as a full touch target (`stateViewActions`).
/// Give each action button its accessibility identifier (R1964).
public struct EmptyStateView<Actions: View>: View {
    private let title: Text
    private let systemImage: String
    private let description: Text?
    private let actions: Actions
    @Environment(\.dynamicTypeSize) private var typeSize

    public init(_ title: Text, systemImage: String, description: Text? = nil, @ViewBuilder actions: () -> Actions) {
        self.title = title
        self.systemImage = systemImage
        self.description = description
        self.actions = actions()
    }

    public var body: some View {
        ContentUnavailableView {
            // Text in text tokens: the system's greys fall below 4.5:1 on
            // white (about 3.5:1 on iOS, 3.95:1 on the Mac). The symbol is
            // decoration (the title says it) and is left out at the
            // accessibility text sizes, where the text needs the room: the
            // view makes the symbol part of the title's accessibility
            // element, and there the contrast audit failed that element on
            // the iPad, whatever the symbol's colour.
            Label { title.foregroundStyle(.token(.textPrimary)) } icon: {
                if !typeSize.isAccessibilitySize {
                    Image(systemName: systemImage).foregroundStyle(.token(.textSecondary))
                }
            }
        } description: {
            description?.foregroundStyle(.token(.textSecondary))
        } actions: {
            actions.stateViewActions()
        }
        // The Mac sizes the view to its content: centred in its container
        // on every platform.
        .frame(maxWidth: .infinity)
    }
}

extension EmptyStateView where Actions == EmptyView {
    public init(_ title: Text, systemImage: String, description: Text? = nil) {
        self.init(title, systemImage: systemImage, description: description) { EmptyView() }
    }
}
