#if MEGURI_TEST_HOOKS && !os(macOS)
import DesignSystem
import SwiftUI
import Testing
import UIKit

/// The settings group of a simulator run (R1971, spike S5): `tools/mg
/// snapshots` sets Reduce Transparency, Reduce Motion and the Liquid Glass
/// opacity with devicectl before the run and passes the group. These tests
/// check that the simulator really has those settings, and render the probe
/// (system glass over content) in every group; the job then compares the
/// probes and fails when a setting leaves the rendered chrome unchanged.
@MainActor
@Suite(.serialized, .enabled(if: SnapshotRun.current.mode != .plan)) struct SystemSettingsTests {
    struct Group: Equatable {
        let reduceTransparency: Bool
        let reduceMotion: Bool
        let transparency: Transparency

        init?(_ name: String) {
            guard let match = name.wholeMatch(of: /rt([01])-rm([01])-t(\d+)/),
                  let transparency = Transparency.allCases.first(where: { Int($0.opacity * 100) == Int(match.3) })
            else { return nil }
            reduceTransparency = match.1 == "1"
            reduceMotion = match.2 == "1"
            self.transparency = transparency
        }
    }

    /// Glass controls over striped content, the way the answer bar sits on
    /// a card.
    struct Probe: View {
        var body: some View {
            ZStack(alignment: .bottom) {
                VStack(spacing: 0) {
                    ForEach(0..<6) { row in
                        Rectangle().fill(.token(row.isMultiple(of: 2) ? .countNew : .countDue)).frame(height: 24)
                    }
                }
                ControlBar {
                    Button {} label: { Text(verbatim: "Again") }
                    Button {} label: { Text(verbatim: "Good") }
                }
                .padding(Spacing.small)
            }
            .frame(width: 320)
        }
    }

    @Test func theSimulatorHasTheGroupsSettings() throws {
        let group = try #require(Group(SnapshotRun.current.group), "no settings group '\(SnapshotRun.current.group)'")
        #expect(UIAccessibility.isReduceTransparencyEnabled == group.reduceTransparency, "Reduce Transparency is not as the group says: devicectl did not apply it")
        #expect(UIAccessibility.isReduceMotionEnabled == group.reduceMotion, "Reduce Motion is not as the group says: devicectl did not apply it")
    }

    @Test func probe() async throws {
        let group = try #require(Group(SnapshotRun.current.group))
        let cell = MatrixCell(
            reduceTransparency: group.reduceTransparency, reduceMotion: group.reduceMotion, transparency: group.transparency,
            device: SnapshotMatrix.referenceDevice(.current)
        )
        await assertSnapshot(subject: "matrix.probe", state: "glass", cell: cell, layout: .component, keepActual: true) { Probe() }
    }
}
#endif
