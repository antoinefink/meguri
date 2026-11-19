import Foundation
import WebKit

/// The editor serves one bundled document, with no collection or file-system
/// access. In particular it never uses the reviewer's script-enabled loader.
final class EditorDocument: NSObject, WKURLSchemeHandler {
    static let url = URL(string: "meguri-editor://editor/document")!
    let html: Data
    let script: String

    init(resources: Bundle) throws {
        // These are developer-authored resources only. Field content never
        // enters this document or these substitutions.
        guard let page = resources.url(forResource: "page", withExtension: "html"),
              let runtime = resources.url(forResource: "runtime", withExtension: "js") else {
            throw CocoaError(.fileNoSuchFile)
        }
        var source = try String(contentsOf: page, encoding: .utf8)
        // N17.1: the page lays its fields out as the platform's form does.
        #if os(macOS)
        source = source.replacingOccurrences(of: "{{editor.platform}}", with: "mac")
        #else
        source = source.replacingOccurrences(of: "{{editor.platform}}", with: "touch")
        #endif
        html = Data(source.utf8)
        script = try String(contentsOf: runtime, encoding: .utf8)
        super.init()
    }

    func webView(_ webView: WKWebView, start urlSchemeTask: any WKURLSchemeTask) {
        guard urlSchemeTask.request.url == Self.url, urlSchemeTask.request.httpMethod == "GET" else {
            urlSchemeTask.didFailWithError(URLError(.noPermissionsToReadFile)); return
        }
        urlSchemeTask.didReceive(URLResponse(url: Self.url, mimeType: "text/html", expectedContentLength: html.count, textEncodingName: "utf-8"))
        urlSchemeTask.didReceive(html); urlSchemeTask.didFinish()
    }
    func webView(_ webView: WKWebView, stop urlSchemeTask: any WKURLSchemeTask) {}
}
