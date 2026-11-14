#if MEGURI_TEST_HOOKS
import Review
import SwiftUI
import Testing

/// Fixed production terminal content inside the review layout. These off-screen
/// screen fixtures do not prove Core transitions, live WebKit or window chrome.
extension ScreenSnapshots {
    @Test(arguments: ReviewFinishedPreview.State.allCases)
    func terminalContinuation(_ state: ReviewFinishedPreview.State) async {
        await assertSnapshot(subject: "review.terminalContinuation", state: state.rawValue,
            cell: MatrixCell(device: SnapshotMatrix.referenceDevice(.current)), layout: .screen) {
                ReviewFinishedPreview.preview(state)
            }
    }
}
#endif
