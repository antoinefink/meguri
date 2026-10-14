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

    public init(_ title: Text, systemImage: String, description: Text? = nil, @ViewBuilder actions: () -> Actions) {
        self.title = title
        self.systemImage = systemImage
        self.description = description
        self.actions = actions()
    }

    public var body: some View {
        ContentUnavailableView {
            // The symbol in a text token: the system's default grey fails
            // the contrast audit at the largest text sizes.
            Label { title } icon: { Image(systemName: systemImage).foregroundStyle(.token(.textSecondary)) }
        } description: {
            description
        } actions: {
            actions.stateViewActions()
        }
    }
}

extension EmptyStateView where Actions == EmptyView {
    public init(_ title: Text, systemImage: String, description: Text? = nil) {
        self.init(title, systemImage: systemImage, description: description) { EmptyView() }
    }
}
