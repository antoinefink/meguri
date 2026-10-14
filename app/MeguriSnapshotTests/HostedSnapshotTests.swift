#if MEGURI_TEST_HOOKS
import DesignSystem
import Foundation
import ImageIO
import SwiftUI
import Testing

/// The snapshot mechanism itself (R1973, R1974), against baselines in a
/// temporary directory: a one-token colour change fails with a diff image,
/// recording takes the new image, and a difference under the perceptual
/// tolerance passes and rewrites nothing.
@MainActor
@Suite(
    .serialized,
    .enabled(if: SnapshotRun.current.mode != .plan && SnapshotRun.current.group == MatrixCell(device: SnapshotMatrix.referenceDevice(.current)).systemGroup)
) struct HostedSnapshotTests {
    /// A swatch drawn in one colour token, the way a component draws it.
    struct Swatch: View {
        let token: ColorToken

        var body: some View {
            HStack(spacing: Spacing.small) {
                RoundedRectangle(cornerRadius: Radius.medium).fill(.token(token)).frame(width: 60, height: 40)
                Text(verbatim: "12 new").font(.body.bold()).foregroundStyle(.token(token))
            }
        }
    }

    private let directory = FileManager.default.temporaryDirectory.appending(path: "hosted-snapshot-tests-\(UUID().uuidString)")
    private let cell = MatrixCell(device: SnapshotMatrix.referenceDevice(.current))

    private func run(_ mode: SnapshotRun.Mode) -> SnapshotRun {
        SnapshotRun(
            mode: mode, group: SnapshotRun.current.group, output: directory.appending(path: "out-\(mode.rawValue)"),
            baselines: directory.appending(path: "baselines"), key: "test"
        )
    }

    private func results(_ mode: SnapshotRun.Mode) throws -> [[String: Any]] {
        let text = try String(contentsOf: directory.appending(path: "out-\(mode.rawValue)/results.jsonl"), encoding: .utf8)
        return try text.split(separator: "\n").map { try JSONSerialization.jsonObject(with: Data($0.utf8)) as! [String: Any] }
    }

    private var baseline: URL { directory.appending(path: "baselines/test.swatch/typical/\(cell.name).png") }

    /// What `tools/mg snapshots --record` does with a recorded image.
    private func copyRecorded() throws {
        let entry = try #require(try results(.record).last)
        let actual = URL(filePath: try #require(entry["actual"] as? String))
        try FileManager.default.createDirectory(at: baseline.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? FileManager.default.removeItem(at: baseline)
        try FileManager.default.copyItem(at: actual, to: baseline)
    }

    private func snapshot(_ token: ColorToken, _ mode: SnapshotRun.Mode) async -> SnapshotStatus? {
        await assertSnapshot(subject: "test.swatch", state: "typical", cell: cell, layout: .component, run: run(mode)) {
            Swatch(token: token)
        }
    }

    @Test func aTokenColourChangeFailsWithADiffImageAndRecordingTakesTheNewImage() async throws {
        defer { try? FileManager.default.removeItem(at: directory) }
        // No baseline yet: recording hands the image over.
        #expect(await snapshot(.countNew, .record) == .recorded)
        try copyRecorded()
        #expect(await snapshot(.countNew, .verify) == .pass)

        // One token changed (New blue drawn as Learn red): the comparison fails
        // and names a diff image.
        await withKnownIssue {
            #expect(await snapshot(.countLearn, .verify) == .fail)
        }
        let failure = try #require(try results(.verify).last)
        #expect(failure["status"] as? String == "fail")
        let diff = try #require(failure["diff"] as? String, "no diff path in \(failure)")
        #expect(FileManager.default.fileExists(atPath: diff))
        #expect(FileManager.default.fileExists(atPath: try #require(failure["actual"] as? String)))

        // Recording that snapshot again takes the new image; it then passes.
        #expect(await snapshot(.countLearn, .record) == .recorded)
        try copyRecorded()
        #expect(await snapshot(.countLearn, .verify) == .pass)
    }

    /// The PNG with every colour channel moved by `delta` levels: a ΔE
    /// near 1, the kind of difference glass and blur renders show.
    private func shifted(_ png: Data, by delta: Int) throws -> Data {
        let source = try #require(CGImageSourceCreateWithData(png as CFData, nil))
        let image = try #require(CGImageSourceCreateImageAtIndex(source, 0, nil))
        let width = image.width, height = image.height
        var pixels = [UInt8](repeating: 0, count: width * height * 4)
        let space = CGColorSpace(name: CGColorSpace.sRGB)!
        let context = try #require(CGContext(
            data: &pixels, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4, space: space,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ))
        context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
        for index in pixels.indices where index % 4 != 3 {
            pixels[index] = UInt8(clamping: Int(pixels[index]) + delta)
        }
        let shiftedImage = try #require(context.makeImage())
        let output = NSMutableData()
        let destination = try #require(CGImageDestinationCreateWithData(output, "public.png" as CFString, 1, nil))
        CGImageDestinationAddImage(destination, shiftedImage, nil)
        #expect(CGImageDestinationFinalize(destination))
        return output as Data
    }

    @Test func aDifferenceUnderThePerceptualToleranceRewritesNothing() async throws {
        defer { try? FileManager.default.removeItem(at: directory) }
        #expect(await snapshot(.countDue, .record) == .recorded)
        try copyRecorded()
        // The baseline drifts by one level per channel, as glass renders do
        // between runs; recording over it must keep it byte for byte.
        try shifted(try Data(contentsOf: baseline), by: -1).write(to: baseline)
        let before = try Data(contentsOf: baseline)
        let status = await snapshot(.countDue, .record)
        let entry = try #require(try results(.record).last)
        #expect(status == .pass, "\(entry["message"] ?? "")")
        #expect(entry["status"] as? String == "pass")
        #expect(entry["actual"] == nil, "a passing snapshot hands no image over to record")
        #expect(try Data(contentsOf: baseline) == before)
    }

    @Test func aMissingBaselineFailsAndSaysHowToRecordIt() async throws {
        defer { try? FileManager.default.removeItem(at: directory) }
        await withKnownIssue {
            #expect(await snapshot(.countNew, .verify) == .new)
        } matching: { issue in
            issue.description.contains("tools/mg snapshots --record 'test.swatch/typical/\(cell.name)'")
        }
    }

    @Test func theFilterSkipsOtherSnapshots() async throws {
        defer { try? FileManager.default.removeItem(at: directory) }
        let filtered = SnapshotRun(
            mode: .verify, filters: ["other.*"], group: SnapshotRun.current.group, output: directory.appending(path: "out-verify"),
            baselines: directory.appending(path: "baselines"), key: "test"
        )
        let status = await assertSnapshot(subject: "test.swatch", state: "typical", cell: cell, layout: .component, run: filtered) {
            Swatch(token: .countNew)
        }
        #expect(status == nil)
        #expect(filtered.selects("designsystem.toast/typical/light-standard-iphoneLarge") == false)
        #expect(SnapshotRun(environment: ["MEGURI_SNAPSHOT_FILTER": "designsystem.toast/*, sample.*"]).selects("designsystem.toast/error/x"))
    }

    @Test func plansListCellsWithoutRendering() async throws {
        defer { try? FileManager.default.removeItem(at: directory) }
        let tinted = MatrixCell(glassLook: .tinted, device: cell.device)
        for target in [cell, tinted] {
            await assertSnapshot(subject: "test.swatch", state: "typical", cell: target, layout: .component, run: run(.plan)) {
                Swatch(token: .countNew)
            }
        }
        let text = try String(contentsOf: directory.appending(path: "out-plan/plan.jsonl"), encoding: .utf8)
        let lines = text.split(separator: "\n")
        #expect(lines.count == 2)
        #expect(lines.last?.contains("\"deferred\"") == true)
        #expect(!FileManager.default.fileExists(atPath: directory.appending(path: "out-plan/results.jsonl").path))
    }
}
#endif
