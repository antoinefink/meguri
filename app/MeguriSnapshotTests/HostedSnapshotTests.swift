#if MEGURI_TEST_HOOKS
import DesignSystem
import Foundation
import ImageIO
import SnapshotTesting
import SwiftUI
import Testing

/// The snapshot mechanism itself (R1973, R1974), against baselines in a
/// temporary directory: a one-token colour change fails with a diff image,
/// recording takes the new image, and a difference under the perceptual
/// tolerance passes and rewrites nothing.
@MainActor
@Suite(
    .serialized,
    .enabled(if: SnapshotRun.current.mode != .plan && SnapshotRun.current.group == MatrixCell(device: SnapshotMatrix.referenceDevice(.current)).systemGroup
        && SnapshotMatrix.referenceDevice(.current).target == SnapshotRun.current.target)
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

    private func planRun(at output: URL, fromEnvironment: Bool) -> SnapshotRun {
        if fromEnvironment {
            return SnapshotRun(environment: [
                "MEGURI_SNAPSHOT_MODE": "plan", "MEGURI_SNAPSHOT_OUT": output.path,
                "MEGURI_SNAPSHOT_FILTER": "test.swatch/*",
                "MEGURI_SNAPSHOT_TARGET": SnapshotRun.current.target.rawValue,
                "MEGURI_SNAPSHOT_GROUP": SnapshotRun.current.group, "MEGURI_SNAPSHOT_KEY": "test",
            ])
        }
        return SnapshotRun(
            mode: .plan, filters: ["test.swatch/*"], group: SnapshotRun.current.group,
            output: output, baselines: directory.appending(path: "baselines"), key: "test"
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

    #if os(macOS)
    @Test(arguments: Appearance.allCases)
    func aNativeNavigationFormRetainsItsScreenViewportAndPaintsText(appearance: Appearance) async throws {
        let cell = MatrixCell(appearance: appearance, device: .macDefault)
        func screen(textVisible: Bool) -> AnyView {
            AnyView(
                NavigationStack {
                    Form {
                        Section("Collection") {
                            Text("Choose a collection to import.").opacity(textVisible ? 1 : 0)
                            LabeledContent {
                                Text("Example collection").opacity(textVisible ? 1 : 0)
                            } label: {
                                Text("Name").opacity(textVisible ? 1 : 0)
                            }
                            Button {} label: {
                                Text("Choose File").opacity(textVisible ? 1 : 0)
                            }
                        }
                    }
                    .navigationTitle("Import Collection")
                }
            )
        }
        func pixels(_ image: PlatformImage) throws -> [UInt8] {
            #expect(image.size == cell.device.size)
            let png = PlatformImage.snapshotDiffing.toData(image)
            let source = try #require(CGImageSourceCreateWithData(png as CFData, nil))
            let cgImage = try #require(CGImageSourceCreateImageAtIndex(source, 0, nil))
            // The screen contract is 1180 x 780 pt at 2x, even when the
            // native content's fitting height is much smaller.
            #expect(cgImage.width == 2360)
            #expect(cgImage.height == 1560)
            var bytes = [UInt8](repeating: 0, count: cgImage.width * cgImage.height * 4)
            let context = try #require(CGContext(
                data: &bytes, width: cgImage.width, height: cgImage.height,
                bitsPerComponent: 8, bytesPerRow: cgImage.width * 4,
                space: CGColorSpace(name: CGColorSpace.sRGB)!,
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
            ))
            context.draw(cgImage, in: CGRect(x: 0, y: 0, width: cgImage.width, height: cgImage.height))
            return bytes
        }
        let visible = try pixels(await HostedRenderer.render(screen(textVisible: true), cell: cell, layout: .screen))
        let hidden = try pixels(await HostedRenderer.render(screen(textVisible: false), cell: cell, layout: .screen))
        try #require(visible.count == hidden.count)
        // Hiding only text preserves the native layout and surfaces. A
        // transparent or background-only capture cannot satisfy this check.
        var paintedTextPixels = 0
        for index in stride(from: 0, to: visible.count, by: 4) {
            if visible[index + 3] == 255, hidden[index + 3] == 255,
               (0..<3).contains(where: { abs(Int(visible[index + $0]) - Int(hidden[index + $0])) >= 32 }) {
                paintedTextPixels += 1
            }
        }
        #expect(paintedTextPixels >= 100, "the native Form's text was not painted in the screen capture")
    }
    #endif

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

    @Test(arguments: [false, true])
    func aPlanWithNoTestCasesStillHasAnEmptyArtifact(fromEnvironment: Bool) throws {
        defer { try? FileManager.default.removeItem(at: directory) }
        let plan = planRun(at: directory.appending(path: "empty-plan"), fromEnvironment: fromEnvironment)
        // No snapshot assertion runs, as when a selected parameterized suite has zero cases.
        #expect(try Data(contentsOf: plan.output.appending(path: "plan.jsonl")).isEmpty)
        #expect(!FileManager.default.fileExists(atPath: plan.output.appending(path: "results.jsonl").path))
    }

    @Test func restartingAPlanPreservesItsRowsAndStillAppendsSelectedCells() throws {
        defer { try? FileManager.default.removeItem(at: directory) }
        let first = planRun(at: directory.appending(path: "out-plan"), fromEnvironment: false)
        assertTextSnapshot("unused", subject: "test.swatch", state: "typical", cell: cell, run: first)
        let artifact = first.output.appending(path: "plan.jsonl")
        let before = try Data(contentsOf: artifact)
        #expect(!before.isEmpty)
        let resumed = planRun(at: first.output, fromEnvironment: true)
        #expect(try Data(contentsOf: artifact) == before)
        assertTextSnapshot("unused", subject: "test.swatch", state: "other", cell: cell, run: resumed)
        let rows = try String(contentsOf: artifact, encoding: .utf8).split(separator: "\n").map {
            try JSONSerialization.jsonObject(with: Data($0.utf8)) as! [String: Any]
        }
        #expect(rows.compactMap { $0["name"] as? String } == [
            "test.swatch/typical/\(cell.name)", "test.swatch/other/\(cell.name)",
        ])
        #expect(!FileManager.default.fileExists(atPath: first.output.appending(path: "results.jsonl").path))
    }

    @Test(arguments: [false, true])
    func aPlanStartupIOFailureIsReported(fromEnvironment: Bool) throws {
        defer { try? FileManager.default.removeItem(at: directory) }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let blocked = directory.appending(path: "file-instead-of-directory")
        try Data("keep".utf8).write(to: blocked)
        withKnownIssue {
            _ = planRun(at: blocked, fromEnvironment: fromEnvironment)
        } matching: { issue in
            issue.description.contains("could not initialize plan.jsonl")
        }
        #expect(try Data(contentsOf: blocked) == Data("keep".utf8))
    }
}
#endif
