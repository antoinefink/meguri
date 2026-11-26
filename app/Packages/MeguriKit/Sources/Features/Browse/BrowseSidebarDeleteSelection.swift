import AppModel
import CoreBridge

/// S9.3's acted-on rows, independent of the query's filter highlights. The
/// sidebar restores its List selection to Browse after clicks (N21.1).
struct BrowseSidebarDeleteSelection {
    private(set) var rows: Set<BrowseSidebarTag> = []
    private(set) var target: BrowseTagDeletionTarget?

    /// Capture when selection changes, never when the Delete key arrives.
    mutating func capture(deletion: BrowseTagDeletion, tree: [TagNode]) {
        target = deletion.target(Self.tagNames(in: rows, tree: tree))
    }

    mutating func changed(from previous: Set<BrowseSidebarTag>, to next: Set<BrowseSidebarTag>,
                          byKey: Bool, commandClick: Bool) {
        if commandClick && !byKey {
            // List reports the changed rows alongside its restored Browse row
            // (or the row the arrow keys stood on). Only the changed rows toggle.
            var changed = previous.symmetricDifference(next)
            if previous == [.section(.browse)] { changed.remove(.section(.browse)) }
            rows.formSymmetricDifference(changed)
        } else {
            rows = next
        }
    }

    mutating func clicked(_ row: BrowseSidebarTag, commandClick: Bool) {
        if commandClick { rows.formSymmetricDifference([row]) } else { rows = [row] }
    }

    /// Display order, with no partial deletion of a mixed or stale selection.
    static func tagNames(in rows: Set<BrowseSidebarTag>, tree: [TagNode]) -> [String] {
        guard !rows.isEmpty, rows.allSatisfy({ if case .tag(let name) = $0 { !name.isEmpty } else { false } }) else { return [] }
        func walk(_ nodes: [TagNode]) -> [String] {
            nodes.flatMap { (rows.contains(.tag($0.fullName)) ? [$0.fullName] : []) + walk($0.children) }
        }
        let names = walk(tree)
        return names.count == rows.count ? names : []
    }
}
