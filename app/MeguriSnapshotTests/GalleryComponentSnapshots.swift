#if MEGURI_TEST_HOOKS
import Testing

/// Every component gallery entry (U9, R1999) in the key variants, on the
/// iPhone and the Mac (R1970).
@MainActor
@Suite(.serialized) struct GalleryComponentSnapshots {
    @Test(.enabled { await !MatrixRegistry.componentCases(on: .current).isEmpty }, arguments: await MatrixRegistry.componentCases(on: .current))
    func component(_ snapshot: SnapshotCase) async {
        await snapshot.assert()
    }
}

/// Every registered screen state (`MatrixRegistry.screens`) in its cells.
@MainActor
@Suite(.serialized) struct ScreenSnapshots {
    @Test(.enabled { await !MatrixRegistry.screenCases(on: .current).isEmpty }, arguments: await MatrixRegistry.screenCases(on: .current))
    func screen(_ snapshot: SnapshotCase) async {
        await snapshot.assert()
    }
}
#endif
