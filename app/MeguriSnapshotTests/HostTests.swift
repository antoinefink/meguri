import Foundation
import Testing

/// The snapshot bundle runs inside the app (KTD19); U10 adds the snapshot
/// matrix.
@Suite struct HostTests {
    @Test func runsInsideTheApp() {
        #expect(Bundle.main.bundleURL.lastPathComponent == "Meguri.app")
    }
}
