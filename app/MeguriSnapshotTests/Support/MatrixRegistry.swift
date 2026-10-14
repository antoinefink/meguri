#if MEGURI_TEST_HOOKS
import DesignSystem
import SwiftUI
import Testing

@testable import Meguri

/// A registered screen: its state fixtures (`PreviewStates`) and the cells
/// each state is snapshotted in on each platform (nil: not on that platform).
@MainActor
struct ScreenRegistration {
    let type: any PreviewStates.Type
    let set: (SnapshotPlatform, PreviewState) -> MatrixSet?

    var id: String { type.previewID }
}

/// One snapshot: a subject's state in one matrix cell. Sendable, so Swift
/// Testing can run one test case per snapshot; the view is looked up again
/// on the main actor.
struct SnapshotCase: Sendable, CustomTestStringConvertible {
    let subject: String
    let state: String
    let cell: MatrixCell
    let layout: SnapshotLayout

    var testDescription: String { "\(subject)/\(state)/\(cell.name)" }

    @MainActor
    func view() -> AnyView? {
        if layout == .component {
            return ComponentGallery.entries.first { "designsystem.\($0.component)" == subject && $0.state == state }?.makeContent()
        }
        guard let screen = MatrixRegistry.screens.first(where: { $0.id == subject }),
              let state = PreviewState(rawValue: state)
        else { return nil }
        return screen.type.fixture(state)?.makeView()
    }

    @MainActor
    func assert(sourceLocation: SourceLocation = #_sourceLocation) async {
        guard let view = view() else {
            Issue.record("\(testDescription): the registry has no such subject or state", sourceLocation: sourceLocation)
            return
        }
        await assertSnapshot(subject: subject, state: state, cell: cell, layout: layout, sourceLocation: sourceLocation) { view }
    }
}

/// Everything the snapshot matrix covers (R1970–R1972). A screen joins
/// `screens` when it conforms to `PreviewStates`; signature screens
/// (`signature: true` in the inventory) take U252's covering set, the others
/// the pairwise set. Components come from the component gallery
/// (`ComponentGallery.entries`) in the key variants.
@MainActor
enum MatrixRegistry {
    static let screens: [ScreenRegistration] = [
        // The sample screen shows the pairwise set and its settings groups
        // with its overflow state on the iPhone; key variants elsewhere keep
        // its baselines small.
        ScreenRegistration(type: TestHooks.SampleDeckOverview.self) { platform, state in
            platform == .iphone && state == .overflow ? .pairwise : .keyVariants
        },
    ]

    /// Platforms the gallery components are snapshotted on: iPad renders
    /// them as iPhone does, at another scale.
    static let componentPlatforms: [SnapshotPlatform] = [.iphone, .mac]

    static func componentCases(on platform: SnapshotPlatform) -> [SnapshotCase] {
        guard componentPlatforms.contains(platform) else { return [] }
        let cells = SnapshotMatrix.cells(.keyVariants, on: platform)
        return ComponentGallery.entries.flatMap { entry in
            cells.map { SnapshotCase(subject: "designsystem.\(entry.component)", state: entry.state, cell: $0, layout: .component) }
        }
    }

    static func screenCases(on platform: SnapshotPlatform) -> [SnapshotCase] {
        screens.flatMap { screen in
            screen.type.previewStates.flatMap { state -> [SnapshotCase] in
                guard let set = screen.set(platform, state) else { return [] }
                return SnapshotMatrix.cells(set, on: platform).map {
                    SnapshotCase(subject: screen.id, state: state.rawValue, cell: $0, layout: .screen)
                }
            }
        }
    }
}
#endif
