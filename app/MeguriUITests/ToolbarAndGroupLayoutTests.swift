import Foundation
import XCTest
#if os(iOS)
import UIKit
#endif

/// Layout faults a passing build does not show, in the running app over the
/// test hooks' made-up content:
///
/// - Note Types (N22.5): while a save job runs its status replaces Revert and
///   Save, and no toolbar item overlaps another, idle or busy, on the Mac at
///   the default and minimum window sizes and on iPad in both orientations.
/// - The welcome screen (N18.3): the grouped choices sit inside the detail
///   with the heading's margin, never under the iPad sidebar.
@MainActor
final class ToolbarAndGroupLayoutTests: XCTestCase {
    // MARK: Note Types toolbar

    func testARunningSaveJobReplacesRevertAndSaveWithOneStatus() throws {
        try skipOnPhone()
        for layout in Self.layouts {
            let app = try launch("notetypes/save/uploading", layout)
            let status = element(app, "noteTypes.list.progress")
            XCTAssertTrue(status.waitForExistence(timeout: 20), "\(layout): no job status")
            XCTAssertFalse(element(app, "noteTypes.list.save").exists, "\(layout): Save shows while the job runs")
            XCTAssertFalse(element(app, "noteTypes.list.revert").exists, "\(layout): Revert shows while the job runs")
            assertApart(app, ["noteTypes.list.tabs", "noteTypes.list.progress"], layout)
            app.terminate()
        }
    }

    func testIdleToolbarItemsDoNotOverlap() throws {
        try skipOnPhone()
        for layout in Self.layouts {
            let app = try launch("notetypes/fields/edited", layout)
            XCTAssertTrue(element(app, "noteTypes.list.save").waitForExistence(timeout: 20), "\(layout): no Save")
            XCTAssertFalse(element(app, "noteTypes.list.progress").exists, "\(layout): a job status with no job")
            assertApart(app, ["noteTypes.list.tabs", "noteTypes.list.revert", "noteTypes.list.save"], layout)
            app.terminate()
        }
    }

    // MARK: Welcome

    /// In the iPad bug the group began under the sidebar (its leading corners
    /// hidden) while the heading kept the system's margin: the rows' cells
    /// started 130 points before the heading.
    func testTheWelcomeGroupKeepsTheHeadingsMarginInsideTheDetail() throws {
        for layout in Self.layouts {
            let app = try launch("notetypes/first-run", layout)
            let title = element(app, "home.welcome.title")
            XCTAssertTrue(title.waitForExistence(timeout: 20), "\(layout): no welcome")
            let sidebar = element(app, "shell.root.sidebar.home")
            for id in ["home.collection.accountSync", "home.collection.import", "home.collection.startEmpty"] {
                let row = element(app, id)
                XCTAssertTrue(row.waitForExistence(timeout: 5), "\(layout): no \(id)")
                let frame = groupFrame(app, row)
                let inset = title.frame.minX - frame.minX
                XCTAssertTrue((0...32).contains(inset),
                              "\(layout): \(id)'s group starts \(inset) pt before the heading (\(frame) vs \(title.frame))")
                if sidebar.exists, sidebar.isHittable {
                    XCTAssertGreaterThan(frame.minX, sidebar.frame.maxX, "\(layout): \(id)'s group starts under the sidebar")
                }
                let window = app.windows.firstMatch.frame
                XCTAssertLessThanOrEqual(frame.maxX, window.maxX, "\(layout): \(id)'s group runs off the window")
            }
            app.terminate()
        }
    }

    // MARK: Helpers

    /// Note Types' routes open the section in the sidebar layout (Mac, iPad).
    private func skipOnPhone() throws {
        #if os(iOS)
        if UIDevice.current.userInterfaceIdiom == .phone { throw XCTSkip("Note Types routes are for the sidebar layout") }
        #endif
    }

    private enum Layout: CustomStringConvertible {
        case window(String), landscape, portrait
        var description: String {
            switch self {
            case .window(let size): "window \(size)"
            case .landscape: "landscape"
            case .portrait: "portrait"
            }
        }
    }

    private static var layouts: [Layout] {
        #if os(macOS)
        [.window("default"), .window("minimum")]
        #else
        UIDevice.current.userInterfaceIdiom == .pad ? [.landscape, .portrait] : [.portrait]
        #endif
    }

    private func launch(_ route: String, _ layout: Layout) throws -> XCUIApplication {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("The routes need a hook build") }
        let app = XCUIApplication()
        var launch = MeguriLaunch(locale: "en_US", animations: false, route: route)
        #if os(macOS)
        if case .window(let size) = layout { launch.windowSize = size }
        #else
        XCUIDevice.shared.orientation = { if case .landscape = layout { .landscapeLeft } else { .portrait } }()
        #endif
        app.launchArguments = launch.arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        return app
    }

    /// The frame of a welcome row's group: on touch the list cell (the
    /// button inside it keeps the cell's margins), on the Mac the row.
    private func groupFrame(_ app: XCUIApplication, _ row: XCUIElement) -> CGRect {
        #if os(macOS)
        return row.frame
        #else
        let cell = app.cells.containing(.any, identifier: row.identifier).firstMatch
        return cell.exists ? cell.frame : row.frame
        #endif
    }

    /// Each pair of the elements must not overlap, and each must be inside the window.
    private func assertApart(_ app: XCUIApplication, _ ids: [String], _ layout: Layout,
                             file: StaticString = #filePath, line: UInt = #line) {
        let window = app.windows.firstMatch.frame
        let frames = ids.map { id -> (String, CGRect) in
            let item = element(app, id)
            XCTAssertTrue(item.waitForExistence(timeout: 10), "\(layout): no \(id)", file: file, line: line)
            return (id, item.frame)
        }
        for (id, frame) in frames {
            XCTAssertTrue(window.contains(frame), "\(layout): \(id) \(frame) is outside the window \(window)", file: file, line: line)
        }
        for (index, (first, a)) in frames.enumerated() {
            for (second, b) in frames.dropFirst(index + 1) {
                XCTAssertFalse(a.intersects(b), "\(layout): \(first) \(a) overlaps \(second) \(b)", file: file, line: line)
            }
        }
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }
}
