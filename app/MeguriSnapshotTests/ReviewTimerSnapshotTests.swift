#if MEGURI_TEST_HOOKS
import Review
import SwiftUI
import Testing

/// Static native timer components, separate from existing screen baselines.
@MainActor
@Suite(.serialized, .enabled(if: SnapshotRun.current.mode != .plan && SnapshotRun.current.group == MatrixCell(device: SnapshotMatrix.referenceDevice(.current)).systemGroup
        && SnapshotMatrix.referenceDevice(.current).target == SnapshotRun.current.target))
struct ReviewTimerSnapshotTests {
    @Test(arguments: ReviewTimerPreview.State.allCases)
    func timer(_ state: ReviewTimerPreview.State) async {
        await assertSnapshot(subject: "review.timer", state: state.rawValue,
            cell: MatrixCell(device: SnapshotMatrix.referenceDevice(.current)), layout: .component) {
                ReviewTimerPreview(state: state)
            }
    }
}
#endif
