#if MEGURI_TEST_HOOKS
import Testing

/// Every component gallery entry (U9, R1999) in the key variants, on the
/// iPhone and the Mac (R1970).
@MainActor
@Suite(.serialized) struct GalleryComponentSnapshots {
    @Test(.enabled { await !MatrixRegistry.componentCases(for: .current).isEmpty }, arguments: await MatrixRegistry.componentCases(for: .current))
    func component(_ snapshot: SnapshotCase) async {
        await snapshot.assert()
    }
}

/// Every registered screen state (`MatrixRegistry.screens`) in its cells.
@MainActor
@Suite(.serialized) struct ScreenSnapshots {
    @Test(.enabled { await !MatrixRegistry.screenCases(for: .current).isEmpty }, arguments: await MatrixRegistry.screenCases(for: .current))
    func screen(_ snapshot: SnapshotCase) async {
        await snapshot.assert()
    }
}
#endif
