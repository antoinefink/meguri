import Foundation
import XCTest

#if os(macOS)
/// Native Browse navigation shared by the tag and note-type deletion journeys.
@MainActor
struct MacBrowseSidebar {
    let app: XCUIApplication

    func searchField() throws -> XCUIElement {
        let sidebar = app.outlines.matching(identifier: "browse.sidebar.list").firstMatch
        XCTAssertTrue(sidebar.waitForExistence(timeout: 20), "The native Browse sidebar must exist")
        var matches: [XCUIElement] = []
        XCTAssertTrue(waitUntil {
            let bounds = sidebar.frame
            let toolbarFields = app.toolbars.searchFields.allElementsBoundByIndex.map(\.frame)
            matches = app.windows.firstMatch.searchFields.allElementsBoundByIndex.filter {
                let frame = $0.frame
                return !frame.isEmpty && frame.minX >= bounds.minX && frame.maxX <= bounds.maxX
                    && !toolbarFields.contains(frame)
            }
            return matches.count == 1
        }, "Require one native search within the production sidebar's horizontal bounds, outside the toolbar. "
           + "Sidebar: \(sidebar.debugDescription); search fields: \(app.windows.firstMatch.searchFields.debugDescription)")
        let search = try XCTUnwrap(matches.first)
        XCTAssertTrue(waitUntil { search.isEnabled && search.isHittable },
                      "Require an actionable sidebar search: \(search.debugDescription)")
        return search
    }

    private func waitUntil(timeout: TimeInterval = 20, _ condition: () -> Bool) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if condition() { return true }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        return condition()
    }
}
#endif
