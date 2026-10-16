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
        // The shell: the tab layout on the iPhone in every state (typical in
        // the pairwise set), the sidebar layout on the iPad in its typical
        // state's key variants (the loading and error states change only the
        // detail column, and the iPad's full-screen renders weigh about 1 MB
        // each). Not on the Mac:
        // the off-screen render (cacheDisplay) cannot draw the AppKit split
        // view's sidebar materials (a black selection, a white detail column
        // in dark mode); the Mac shell is judged from `tools/mg screenshot
        // --platform mac` and the device session.
        ScreenRegistration(type: ShellRootPreview.self) { platform, state in
            switch platform {
            case .mac: nil
            case .iphone: state == .typical ? .pairwise : .keyVariants
            case .ipad: state == .typical ? .keyVariants : nil
            }
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

    /// The cases a test process on `target` renders: its platform's cells
    /// whose device class renders there.
    static func componentCases(for target: SnapshotTarget) -> [SnapshotCase] {
        componentCases(on: target.platform).filter { $0.cell.device.target == target }
    }

    static func screenCases(for target: SnapshotTarget) -> [SnapshotCase] {
        screenCases(on: target.platform).filter { $0.cell.device.target == target }
    }

    /// The platforms a screen is registered on: those where one of its
    /// states has cells (`tools/mg critic` grades it on all of them).
    static func platforms(of screen: ScreenRegistration) -> [SnapshotPlatform] {
        SnapshotPlatform.allCases.filter { platform in screen.type.previewStates.contains { screen.set(platform, $0) != nil } }
    }

    /// Per platform, every `state/cell` of the screen a simulator or the Mac
    /// renders (deferred cells left out), in matrix order: what a full
    /// snapshot run gives the critic.
    static func renderedCells(of screen: ScreenRegistration) -> [String: [String]] {
        Dictionary(uniqueKeysWithValues: platforms(of: screen).map { platform in
            (platform.rawValue, screenCases(on: platform).filter { $0.subject == screen.id && $0.cell.deferredReason == nil }.map { "\($0.state)/\($0.cell.name)" })
        })
    }
}
#endif
