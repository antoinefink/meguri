import AppModel
import DesignSystem
import SwiftUI

public enum ManualTransferSettingsPreview: PreviewStates {
    public static let previewID = "settings.sync"
    public static var previewFixtures: [PreviewFixture] {
        [PreviewFixture(.empty) { screen(.empty) }, PreviewFixture(.typical) { screen(.configured) },
         PreviewFixture(.error) { screen(.error) }, PreviewFixture(.loading) { screen(.loading) }]
    }
    private static func screen(_ state: ManualTransferCoordinator.RenderState) -> some View {
        let model = ManualTransferCoordinator.renderFixture(state)
        return NavigationStack {
            Form { ManualTransferSections(model: model) }
                .background(.token(.backgroundPrimary))
                .accessibilityIdentifier("settings.anki.content")
                .manualTransferPresentation(model: model)
                .navigationTitle("Anki Server Import / Export")
        }
    }
}
