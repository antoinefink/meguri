#if MEGURI_TEST_HOOKS
import DesignSystem
import Foundation
import ImageIO
import SnapshotTesting
import SwiftUI
import Testing

/// Flat chrome (`ChromeGlass.flat`, the Mac's off-screen snapshots) draws
/// floating chrome on a surface that stands out from the screen background
/// in light and dark, so a toast's container, padding and extent show in
/// its snapshots (U10 review: the light surface was the background's white).
@MainActor
@Suite(
    .serialized,
    .enabled(if: SnapshotRun.current.mode != .plan && SnapshotRun.current.group == MatrixCell(device: SnapshotMatrix.referenceDevice(.current)).systemGroup
        && SnapshotMatrix.referenceDevice(.current).target == SnapshotRun.current.target)
) struct FlatChromeTests {
    /// An empty capsule of floating chrome, as a toast draws it.
    struct Capsule: View {
        var body: some View {
            Color.clear.frame(width: 160, height: 48).floatingChrome().environment(\.chromeGlass, .flat)
        }
    }

    /// The RGBA bytes of an image, 8 bits per channel.
    private func pixels(_ image: PlatformImage) throws -> (width: Int, height: Int, bytes: [UInt8]) {
        let png = PlatformImage.snapshotDiffing.toData(image)
        let source = try #require(CGImageSourceCreateWithData(png as CFData, nil))
        let cgImage = try #require(CGImageSourceCreateImageAtIndex(source, 0, nil))
        let width = cgImage.width, height = cgImage.height
        var bytes = [UInt8](repeating: 0, count: width * height * 4)
        let context = try #require(CGContext(
            data: &bytes, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
            space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ))
        context.draw(cgImage, in: CGRect(x: 0, y: 0, width: width, height: height))
        return (width, height, bytes)
    }

    @Test(arguments: Appearance.allCases)
    func floatingChromeStandsOutFromTheBackground(appearance: Appearance) async throws {
        let cell = MatrixCell(appearance: appearance, device: SnapshotMatrix.referenceDevice(.current))
        let image = try await HostedRenderer.render(AnyView(Capsule()), cell: cell, layout: .component)
        let (width, height, bytes) = try pixels(image)
        func pixel(_ x: Int, _ y: Int) -> [Int] { (0..<3).map { Int(bytes[(y * width + x) * 4 + $0]) } }
        // The corner is the component's background; the centre is inside the capsule.
        let background = pixel(1, 1)
        let capsule = pixel(width / 2, height / 2)
        let difference = zip(background, capsule).map { abs($0 - $1) }.max() ?? 0
        #expect(difference >= 8, "in \(appearance) the flat chrome \(capsule) is the background \(background): the toast has no visible container")
    }
}
#endif
