import Foundation
import SnapshotTesting
import Testing
import WebKit
#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// Card faces in a hosted web view (R1975): the second of their two checks.
/// The core's rendered HTML is checked as text against the oracle's goldens
/// by the conformance runner (`tools/mg conformance`, the rendering area's
/// `render` observations: the Meguri side of each scenario golden); this
/// helper checks what the web view makes of a page, as an image snapshot
/// and as a text snapshot of its DOM after layout. The rendering units
/// (U66, U67) own the corpus and the DOM assertions; U252 runs both checks
/// for every corpus case.
@MainActor
enum WebViewSnapshot {
    struct Capture {
        let image: PlatformImage
        /// `document.documentElement.outerHTML` once fonts are loaded and
        /// layout has run after the load.
        let dom: String
    }

    struct LoadError: Error, CustomStringConvertible {
        let description: String
    }

    /// Loads `html` in a web view of the cell's device size and appearance,
    /// in a window (the iOS app's scene, or an off-screen Mac window), and
    /// captures it at `SnapshotTolerance.scale`.
    static func capture(html: String, baseURL: URL? = nil, cell: MatrixCell) async throws -> Capture {
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        let size = cell.device.size
        let webView = WKWebView(frame: CGRect(origin: .zero, size: size), configuration: configuration)
        let host = try Host(webView, cell: cell)
        defer { host.tearDown() }
        let navigation = NavigationWaiter()
        webView.navigationDelegate = navigation
        try await within("the page load") { finish in
            navigation.finish = finish
            webView.loadHTMLString(html, baseURL: baseURL)
        }
        let dom: String = try await within("the layout wait") { finish in
            webView.callAsyncJavaScript(
                // Animation frames never come for a page no screen shows (the
                // Mac's off-screen window): wait for fonts and a layout pass.
                "await document.fonts.ready; await new Promise(r => setTimeout(r, 50)); void document.body.offsetHeight; return document.documentElement.outerHTML;",
                arguments: [:], in: nil, in: .page
            ) { result in finish(result.map { $0 as? String ?? "" }) }
        }
        // Text may still move while the page settles: snapshot until two
        // consecutive snapshots match, as image snapshots wait for frames.
        let diffing = PlatformImage.snapshotDiffing
        var previous: PlatformImage?
        for _ in 0..<HostedRenderer.maximumFrames {
            // As PNG data across the callback: images are not Sendable.
            let png: Data = try await within("the web view snapshot") { finish in
                webView.takeSnapshot(with: nil) { image, error in
                    finish(image.map { .success(PlatformImage.snapshotDiffing.toData($0)) } ?? .failure(error ?? LoadError(description: "no snapshot image")))
                }
            }
            let image = rescaled(diffing.fromData(png), size: size)
            if let previous, diffing.diffV2(previous, image) == nil {
                return Capture(image: image, dom: dom)
            }
            previous = image
            try await Task.sleep(for: HostedRenderer.frameInterval)
        }
        throw LoadError(description: "the web view did not settle within \(HostedRenderer.maximumFrames) snapshots")
    }

    /// Seconds a page load, layout wait or snapshot may take.
    static let timeout: Duration = .seconds(20)

    /// Waits for a WebKit callback, failing after `timeout` instead of
    /// hanging the run.
    private static func within<T: Sendable>(_ what: String, _ start: (@escaping (Result<T, any Error>) -> Void) -> Void) async throws -> T {
        let once = Once<T>()
        let timer = Task { @MainActor in
            try await Task.sleep(for: timeout)
            once.finish(.failure(LoadError(description: "\(what) did not finish within \(timeout)")))
        }
        defer { timer.cancel() }
        return try await withCheckedThrowingContinuation { continuation in
            once.continuation = continuation
            start { once.finish($0) }
        }
    }

    /// The web view snapshot redrawn at the snapshot scale (WebKit draws at
    /// the screen's).
    private static func rescaled(_ image: PlatformImage, size: CGSize) -> PlatformImage {
        #if os(macOS)
        guard let rep = NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: Int(size.width * SnapshotTolerance.scale), pixelsHigh: Int(size.height * SnapshotTolerance.scale),
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
        ) else { return image }
        rep.size = size
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
        image.draw(in: CGRect(origin: .zero, size: size))
        NSGraphicsContext.restoreGraphicsState()
        let result = NSImage(size: size)
        result.addRepresentation(rep)
        return result
        #else
        let format = UIGraphicsImageRendererFormat()
        format.scale = SnapshotTolerance.scale
        format.opaque = true
        format.preferredRange = .standard
        return UIGraphicsImageRenderer(size: size, format: format).image { _ in
            image.draw(in: CGRect(origin: .zero, size: size))
        }
        #endif
    }

    /// A continuation resumed by whichever comes first: the callback or the
    /// timeout.
    private final class Once<T: Sendable> {
        var continuation: CheckedContinuation<T, any Error>?

        func finish(_ result: Result<T, any Error>) {
            continuation?.resume(with: result)
            continuation = nil
        }
    }

    private final class NavigationWaiter: NSObject, WKNavigationDelegate {
        var finish: ((Result<Void, any Error>) -> Void)?

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            finish?(.success(()))
        }

        func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: any Error) {
            finish?(.failure(LoadError(description: "the page failed to load: \(error)")))
        }

        func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: any Error) {
            finish?(.failure(LoadError(description: "the page failed to load: \(error)")))
        }
    }

    #if os(macOS)
    /// An off-screen borderless window, never ordered in.
    private final class Host {
        private let window: NSWindow

        init(_ webView: WKWebView, cell: MatrixCell) throws {
            window = NSWindow(
                contentRect: CGRect(origin: CGPoint(x: -30_000, y: -30_000), size: cell.device.size),
                styleMask: [.borderless], backing: .buffered, defer: false
            )
            window.isReleasedWhenClosed = false
            window.appearance = NSAppearance(named: cell.appearance == .dark ? .darkAqua : .aqua)
            webView.appearance = window.appearance
            window.contentView = webView
        }

        func tearDown() {
            window.contentView = nil
            window.close()
        }
    }
    #else
    /// A window of its own on the app's scene, as image snapshots use.
    private final class Host {
        private let window: UIWindow
        private let previousKeyWindow: UIWindow?

        init(_ webView: WKWebView, cell: MatrixCell) throws {
            let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
            guard let scene = scenes.first(where: { $0.activationState == .foregroundActive }) ?? scenes.first else {
                throw LoadError(description: "the host app has no window scene")
            }
            previousKeyWindow = scene.keyWindow
            window = UIWindow(windowScene: scene)
            window.windowLevel = .alert + 1
            window.frame = CGRect(origin: .zero, size: cell.device.size)
            window.overrideUserInterfaceStyle = cell.appearance == .dark ? .dark : .light
            let controller = UIViewController()
            controller.view = webView
            window.rootViewController = controller
            window.makeKeyAndVisible()
        }

        func tearDown() {
            window.isHidden = true
            window.rootViewController = nil
            previousKeyWindow?.makeKey()
        }
    }
    #endif
}
