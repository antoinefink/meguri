import AppModel
import DesignSystem
import SwiftUI

public enum ManualTransferSettingsPreview: PreviewStates {
    public static let previewID = "settings.sync"
    public static var previewFixtures: [PreviewFixture] {
        [PreviewFixture(.empty) { screen(.empty) }, PreviewFixture(.typical) { screen(.configured) },
         PreviewFixture(.error) { screen(.error) }, PreviewFixture(.loading) { screen(.loading) },
         PreviewFixture(.overflow) { mediaFixture(.mediaConsent) }]
    }
    /// Supplemental selected-media scenarios use the same production Form.
    /// Only consent adds a snapshot cell (overflow); routes cover the remaining
    /// action/result states without expanding the global snapshot vocabulary.
    public static func mediaFixture(_ state: ManualTransferCoordinator.RenderState) -> some View { screen(state) }
    private static func screen(_ state: ManualTransferCoordinator.RenderState) -> some View {
        let model = ManualTransferCoordinator.renderFixture(state)
        return NavigationStack {
            Form {
                if [.mediaConsent, .mediaProgress, .mediaPartial, .mediaComplete, .mediaUnknown].contains(state) {
                    Section {
                        Text("Fixture only: simulated media effects. No server work or acceptance.")
                            .accessibilityIdentifier("settings.anki.fixture")
                    }
                }
                ManualTransferSections(model: model)
            }
                .formStyle(.grouped)
                .background(.token(.backgroundPrimary))
                .accessibilityIdentifier("settings.anki.content")
                .manualTransferPresentation(model: model)
                .navigationTitle(Text("Settings", bundle: .module))
                .task { if state == .mediaProgress { await model.startMedia() } }
        }
    }
}

#Preview("Media consent / long content") { ManualTransferSettingsPreview.mediaFixture(.mediaConsent) }
#Preview("Media progress") { ManualTransferSettingsPreview.mediaFixture(.mediaProgress) }
#Preview("Media partial / resume") { ManualTransferSettingsPreview.mediaFixture(.mediaPartial) }
#Preview("Media complete / exclusions") { ManualTransferSettingsPreview.mediaFixture(.mediaComplete) }
#Preview("Media unknown effects") { ManualTransferSettingsPreview.mediaFixture(.mediaUnknown) }
