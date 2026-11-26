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
        let windows = app.windows.containing(.outline, identifier: "browse.sidebar.list")
        XCTAssertEqual(windows.count, 1, "One window owns the Browse sidebar")
        let window = windows.element(boundBy: 0)
        var match: (index: Int, frame: CGRect)?
        XCTAssertTrue(waitUntil {
            // One native snapshot per attempt: walking its children and reading
            // frames is local, unlike nested AX queries and element.frame calls.
            guard let snapshot = try? window.snapshot() else { return false }
            var bounds: CGRect?
            var fields: [(frame: CGRect, inToolbar: Bool)] = []
            func walk(_ node: any XCUIElementSnapshot, inToolbar: Bool) {
                let inToolbar = inToolbar || node.elementType == .toolbar
                if node.elementType == .outline && node.identifier == "browse.sidebar.list" {
                    bounds = node.frame
                }
                if node.elementType == .searchField {
                    fields.append((node.frame, inToolbar))
                }
                node.children.forEach { walk($0, inToolbar: inToolbar) }
            }
            walk(snapshot, inToolbar: false)
            guard let bounds, !bounds.isEmpty else { return false }
            let matches = fields.indices.filter {
                let field = fields[$0]
                return !field.inToolbar && !field.frame.isEmpty && snapshot.frame.contains(field.frame)
                    && field.frame.minX >= bounds.minX && field.frame.maxX <= bounds.maxX
            }
            match = matches.count == 1 ? (matches[0], fields[matches[0]].frame) : nil
            return matches.count == 1
        }, "Require one native search within the production sidebar's horizontal bounds, outside the toolbar. "
           + "Resolve its geometry from the owning window snapshot.")
        let resolved = try XCTUnwrap(match)
        let search = window.searchFields.element(boundBy: resolved.index)
        // Verify query order against the snapshot before returning a live control.
        XCTAssertEqual(search.frame, resolved.frame, "The live native field matches the scoped snapshot")
        XCTAssertTrue(waitUntil { search.isEnabled && search.isHittable },
                      "Require an actionable sidebar search")
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
