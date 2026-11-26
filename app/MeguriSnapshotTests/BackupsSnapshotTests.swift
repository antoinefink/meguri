#if MEGURI_TEST_HOOKS
import DesignSystem
import Settings
import Testing

@MainActor
@Suite struct BackupsSnapshotTests {
    @Test func everyInventoryStateUsesThePairwiseMatrixOnEveryPlatform() throws {
        let screen = try #require(MatrixRegistry.screens.first { $0.id == "settings.backups" })
        #expect(screen.type.previewStates == PreviewState.allCases)
        #expect(MatrixRegistry.platforms(of: screen) == [.iphone, .ipad, .mac])
        for platform in SnapshotPlatform.allCases {
            let cases = MatrixRegistry.screenCases(on: platform).filter { $0.subject == "settings.backups" }
            for state in PreviewState.allCases {
                let expected = SnapshotMatrix.cells(.pairwise, on: platform)
                #expect(cases.filter { $0.state == state.rawValue }.map(\.cell) == expected)
                #expect(cases.first { $0.state == state.rawValue }?.view() != nil)
                let layout: SnapshotLayout = [.empty, .overflow, .error].contains(state) ? .backupRestore : .screen
                #expect(cases.filter { $0.state == state.rawValue }.allSatisfy { $0.layout == layout })
            }
        }
    }
}
#endif
