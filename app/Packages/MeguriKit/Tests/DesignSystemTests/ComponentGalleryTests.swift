import Testing

import DesignSystem

/// The component gallery's registry (R1999); the UI test
/// `ComponentGalleryTests` in MeguriUITests checks the route shows it.
@Suite struct ComponentGalleryTests {
    /// The shared components U9 provides (plan, U9 Files), plus the chrome.
    static let plannedComponents: Set = [
        "countColumns", "sectionHeader", "emptyState", "errorState", "loadingState", "toast", "progressBar",
        "destructiveConfirmation", "controlBar", "floatingControl",
    ]

    @Test func everyPlannedComponentIsRegistered() {
        let registered = Set(ComponentGallery.entries.map(\.component))
        #expect(Self.plannedComponents.subtracting(registered).isEmpty, "missing: \(Self.plannedComponents.subtracting(registered).sorted())")
    }

    @Test func identifiersFollowTheGalleryPatternAndAreUnique() {
        let identifiers = ComponentGallery.entries.map(\.identifier)
        for identifier in identifiers {
            #expect(identifier.wholeMatch(of: /designsystem\.gallery\.[a-z][A-Za-z0-9]*\.[a-z][A-Za-z0-9]*/) != nil, "\(identifier)")
        }
        #expect(Set(identifiers).count == identifiers.count, "duplicated identifiers")
        #expect(ComponentGallery.entries.first?.identifier == "designsystem.gallery.countColumns.typical")
    }
}
