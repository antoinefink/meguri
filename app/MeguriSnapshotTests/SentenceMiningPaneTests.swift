#if MEGURI_TEST_HOOKS && os(macOS)
import AppModel
import DesignSystem
import Settings
import SwiftUI
import Testing

/// VM-runnable capture of the production Settings pane; no listener or
/// collection is opened. `tools/mg snapshots --platform mac --record
/// 'settings.integrations/*'` retains the actual images for review.
@MainActor @Suite(.serialized) struct SentenceMiningPaneTests {
    @Test(arguments: [false, true]) func actualPane(portConflict: Bool) async {
        let state = portConflict ? "port-conflict" : "listening"
        let model = SentenceMining.renderFixture(portConflict: portConflict)
        let cell = MatrixCell(device: .macDefault)
        await assertSnapshot(subject: "settings.integrations", state: state, cell: cell, layout: .screen) {
            NavigationStack { SentenceMiningPane(model: model) }
        }
    }
}
#endif
