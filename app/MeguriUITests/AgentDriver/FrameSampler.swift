import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers
import XCTest
#if os(macOS)
import AppKit
#endif

/// Screenshots at a fixed rate while a step marked with a transition runs, for
/// motion capture where no screen recording runs: the Mac without Screen
/// Recording (U8, R1976). `tools/mg drive --platform mac --record` turns it on
/// with `MEGURI_DRIVE_FRAMES=1` and makes contact sheets from the frames.
///
/// A timer on the main run loop takes the screenshots. XCUITest runs that
/// loop while it waits for an action and for the app to idle, so the frames
/// cover the action itself, not only its end. The rate is whatever the
/// screenshots allow under the interval (about 10 per second). Each frame is
/// cut to the app's window on the Mac (nothing else on the screen is kept)
/// and scaled down to `maxWidth` pixels at once, which bounds the memory a
/// long step holds.
@MainActor
final class FrameSampler {
    struct Frame {
        /// Epoch seconds, halfway through the screenshot call.
        let time: Double
        let image: CGImage
    }

    static let interval: TimeInterval = 0.05
    /// How long sampling goes on after the action returns.
    static let tail: TimeInterval = 1.5
    /// About 15 s of frames; a step that runs longer keeps its first ones.
    static let maxFrames = 150
    /// Contact-sheet tiles are 320 pixels wide.
    static let maxWidth = 800

    private let window: CGRect
    private var timer: Timer?
    private(set) var frames: [Frame] = []
    private(set) var truncated = false
    private(set) var dropped = 0

    /// `window`: the app's window in screen points, for the Mac crop.
    init(window: CGRect) {
        self.window = window
    }

    func start() {
        sample()
        let timer = Timer(timeInterval: Self.interval, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.sample() }
        }
        RunLoop.main.add(timer, forMode: .common)
        self.timer = timer
    }

    /// Samples for the tail, then stops.
    func finish() {
        RunLoop.current.run(until: Date().addingTimeInterval(Self.tail))
        stop()
    }

    func stop() {
        timer?.invalidate()
        timer = nil
    }

    private func sample() {
        guard frames.count < Self.maxFrames else {
            truncated = true
            stop()
            return
        }
        let before = Date().timeIntervalSince1970
        let screenshot = XCUIScreen.main.screenshot()
        let after = Date().timeIntervalSince1970
        guard let image = reduce(screenshot) else {
            dropped += 1
            return
        }
        frames.append(Frame(time: (before + after) / 2, image: image))
    }

    /// The window's part of the screen (Mac) or the screen (iOS), at most
    /// `maxWidth` pixels wide, in a buffer of its own. Nil when the window is
    /// off the main screen.
    private func reduce(_ screenshot: XCUIScreenshot) -> CGImage? {
        #if os(macOS)
        guard let screen = NSScreen.screens.first,
              let full = screenshot.image.cgImage(forProposedRect: nil, context: nil, hints: nil)
        else { return nil }
        let scale = CGFloat(full.width) / screen.frame.width
        let area = CGRect(x: window.minX * scale, y: window.minY * scale, width: window.width * scale, height: window.height * scale)
            .integral
            .intersection(CGRect(x: 0, y: 0, width: full.width, height: full.height))
        guard !area.isEmpty, let source = full.cropping(to: area) else { return nil }
        #else
        guard let source = screenshot.image.cgImage else { return nil }
        #endif
        let factor = min(1, CGFloat(Self.maxWidth) / CGFloat(source.width))
        let width = max(1, Int((CGFloat(source.width) * factor).rounded()))
        let height = max(1, Int((CGFloat(source.height) * factor).rounded()))
        guard let space = CGColorSpace(name: CGColorSpace.sRGB),
              let context = CGContext(
                  data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
              )
        else { return nil }
        context.interpolationQuality = .medium
        context.draw(source, in: CGRect(x: 0, y: 0, width: width, height: height))
        return context.makeImage()
    }

    static func png(_ image: CGImage) -> Data? {
        let data = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(data, UTType.png.identifier as CFString, 1, nil) else { return nil }
        CGImageDestinationAddImage(destination, image, nil)
        return CGImageDestinationFinalize(destination) ? data as Data : nil
    }
}
