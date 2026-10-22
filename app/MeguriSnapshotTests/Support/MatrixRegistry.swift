#if MEGURI_TEST_HOOKS
import DesignSystem
import Home
import Review
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
        ScreenRegistration(type: HomeDeckListPreview.self) { _, _ in .keyVariants },
        ScreenRegistration(type: ReviewQuestionPreview.self) { _, _ in .keyVariants },
        ScreenRegistration(type: ReviewAnswerPreview.self) { _, _ in .keyVariants },
        ScreenRegistration(type: ReviewFinishedPreview.self) { _, _ in .keyVariants },
        // The sample screen shows the pairwise set and its settings groups
        // with its overflow state on the iPhone; key variants elsewhere keep
        // its baselines small.
        ScreenRegistration(type: TestHooks.SampleDeckOverview.self) { platform, state in
            platform == .iphone && state == .overflow ? .pairwise : .keyVariants
        },
        // The shell in key variants: the tab layout on the iPhone, the
        // sidebar layout on the iPad and the Mac. The iPad renders only the
        // light ones with Reduce Transparency on and lists the others as
        // deferred: with Liquid Glass the system sidebar renders with a
        // grain that differs between runs beyond the tolerance (3 of 4 cells
        // failed in one of three runs, 2026-10-16), and with Reduce
        // Transparency the sidebar toggle's glass still differs by one
        // level, which failed a dark cell in 2 of 7 runs (near-black
        // pixels). The Mac's draw the split view's columns flat
        // (`ChromeGlass.flat`, the Mac matrix's own): the off-screen render
        // (cacheDisplay) cannot draw their materials. The pairwise set's
        // dark iPhone cells with Increase Contrast, Reduce Motion or a glass
        // opacity extreme show the same grain in the tab bar, so the iPhone
        // keeps the key variants too.
        ScreenRegistration(type: ShellRootPreview.self) { platform, _ in
            switch platform {
            case .iphone, .mac: .keyVariants
            case .ipad: .unstableGlassKeyVariants
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
