import Foundation
import XCTest

/// Browse's table on the 150k fixture (U270a; R1829, `browser/workspace`
/// V17): scrolling has a hitch time ratio below 5 ms/s. The app opens the
/// generated 150k collection (`tools/mg fixtures 150k`, which `tools/mg
/// perf-ui` makes and puts in the test VM), shows Browse on the whole
/// collection, and its table is scrolled with no synthesized input: the
/// hook `browseScroll` jumps it a few dozen rows a frame for four seconds,
/// so new rows keep arriving, each page read from the core as it comes on
/// screen.
final class BrowserSearchPerfTests: PerfTestCase {
    override nonisolated class var intervalNames: [String] { [] }
    override nonisolated class var fixture: String? { "150k" }
    override nonisolated class var iterations: Int { 3 }

    func testScrollWholeCollection() throws {
        #if os(macOS)
        let build = try BuildUnderTest.current()
        let fixture = try Self.fixture150k(build.repositoryRoot)
        let app = launch(MeguriLaunch(fixture: fixture, animations: false, windowSize: "large"))
        let bundle = build.expectedBundleIdentifier
        // The collection opens on Today; then Browse, as its sidebar row does.
        XCTAssert(app.descendants(matching: .any)["shell.root.sidebar.browse"].waitForExistence(timeout: 120),
                  "the shell did not show its sidebar")
        post("\(bundle).testhooks.showBrowse")
        let status = app.staticTexts["browse.status.text"]
        XCTAssert(status.waitForExistence(timeout: 60), "Browse's table did not show")
        // The whole collection's first page of rows is shown: a row named by its card.
        let firstRow = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH 'browse.table.row.'")).firstMatch
        XCTAssert(firstRow.waitForExistence(timeout: 120), "Browse showed no rows of the 150k collection")
        let done = "\(bundle).testhooks.browseScrolled"
        try measure(app: app) { _ in
            let finished = expectation(forNotification: Notification.Name(done), object: nil,
                                       notificationCenter: DistributedNotificationCenter.default())
            post("\(bundle).testhooks.browseScroll")
            wait(for: [finished], timeout: 60)
        }
        #else
        throw XCTSkip("Browse's table is the Mac's in this version (U270a); the touch layouts are U278 and U279")
        #endif
    }

    /// The generated 150k collection in this checkout (`build/fixtures/
    /// <profile>-1-150k`).
    static func fixture150k(_ root: URL) throws -> URL {
        let fixtures = root.appending(path: "build/fixtures", directoryHint: .isDirectory)
        let names = (try? FileManager.default.contentsOfDirectory(atPath: fixtures.path(percentEncoded: false))) ?? []
        guard let name = names.filter({ $0.hasSuffix("-1-150k") }).sorted().last else {
            throw PreconditionFailure(description: "no 150k fixture under \(fixtures.path): tools/mg perf-ui makes it")
        }
        return fixtures.appending(path: name, directoryHint: .isDirectory)
    }
}
