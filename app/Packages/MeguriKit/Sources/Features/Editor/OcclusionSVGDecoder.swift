import CoreGraphics
import Foundation
import WebKit

/// WebKit is the system SVG decoder on both platforms. Decode as a data image
/// (SVG image-document restrictions), with no file access, external resources,
/// scripts from the SVG, persistent store, or navigation to a remote origin.
@MainActor
private final class SVGImagePage: NSObject, WKNavigationDelegate {
    let view: WKWebView
    private var loaded: CheckedContinuation<Void, any Error>?

    override init() {
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        view = WKWebView(frame: .zero, configuration: configuration)
        super.init()
        view.navigationDelegate = self
    }

    func png(_ data: Data) async throws -> Data {
        try await withTaskCancellationHandler {
            try Task.checkCancellation()
            try await withCheckedThrowingContinuation { continuation in
                loaded = continuation
                view.loadHTMLString("""
                    <!doctype html><meta http-equiv="Content-Security-Policy"
                    content="default-src 'none'; img-src data:; connect-src 'none'">
                    """, baseURL: nil)
            }
            try Task.checkCancellation()
            let result = try await view.callAsyncJavaScript("""
                const image = new Image();
                await new Promise((resolve, reject) => {
                  image.onload = resolve;
                  image.onerror = () => reject(new Error('The SVG cannot be decoded.'));
                  image.src = 'data:image/svg+xml;base64,' + encoded;
                });
                if (!(image.naturalWidth > 0 && image.naturalHeight > 0))
                  throw new Error('The SVG has no image size.');
                const canvas = document.createElement('canvas');
                canvas.width = image.naturalWidth; canvas.height = image.naturalHeight;
                canvas.getContext('2d').drawImage(image, 0, 0);
                return canvas.toDataURL('image/png').split(',')[1];
                """, arguments: ["encoded": data.base64EncodedString()], in: nil,
                contentWorld: .world(name: "MeguriOcclusionImageDecoder"))
            try Task.checkCancellation()
            guard let encoded = result as? String, let png = Data(base64Encoded: encoded), !png.isEmpty else {
                throw CocoaError(.fileReadCorruptFile)
            }
            return png
        } onCancel: {
            Task { @MainActor in self.finish(CancellationError()); self.view.stopLoading() }
        }
    }

    private func finish(_ error: (any Error)? = nil) {
        let continuation = loaded; loaded = nil
        if let error { continuation?.resume(throwing: error) }
        else { continuation?.resume() }
    }
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) { finish() }
    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: any Error) { finish(error) }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: any Error) { finish(error) }
    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) { finish(CocoaError(.fileReadCorruptFile)) }
    func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction) async -> WKNavigationActionPolicy {
        navigationAction.request.url?.absoluteString == "about:blank" ? .allow : .cancel
    }
}

enum OcclusionSVGDecoder {
    static func decode(_ data: Data) async throws -> CGImage {
        try OcclusionImageInput.decodeBitmap(try await SVGImagePage().png(data))
    }
}
