import Foundation
import UniformTypeIdentifiers
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

    /// Browse's inspector: the collection's media folder, whose files the
    /// page's images load from by name. Nil: the page loads no media.
    var mediaDirectory: URL?

    func webView(_ webView: WKWebView, start urlSchemeTask: any WKURLSchemeTask) {
        if let url = urlSchemeTask.request.url, url != Self.url, urlSchemeTask.request.httpMethod == "GET" {
            return serveMedia(url, urlSchemeTask)
        }
        guard urlSchemeTask.request.url == Self.url, urlSchemeTask.request.httpMethod == "GET" else {
            urlSchemeTask.didFailWithError(URLError(.noPermissionsToReadFile)); return
        }
        urlSchemeTask.didReceive(URLResponse(url: Self.url, mimeType: "text/html", expectedContentLength: html.count, textEncodingName: "utf-8"))
        urlSchemeTask.didReceive(html); urlSchemeTask.didFinish()
    }
    func webView(_ webView: WKWebView, stop urlSchemeTask: any WKURLSchemeTask) {}

    /// An image of a field: `meguri-editor://editor/<name>` from the media
    /// folder, a plain file name only.
    private func serveMedia(_ url: URL, _ task: any WKURLSchemeTask) {
        guard let directory = mediaDirectory, url.host == "editor",
              let name = MediaName.plain(String(url.path.dropFirst())),
              let data = try? Data(contentsOf: directory.appending(path: name, directoryHint: .notDirectory)) else {
            task.didFailWithError(URLError(.fileDoesNotExist)); return
        }
        let type = UTType(filenameExtension: (name as NSString).pathExtension)?.preferredMIMEType ?? "application/octet-stream"
        task.didReceive(URLResponse(url: url, mimeType: type, expectedContentLength: data.count, textEncodingName: nil))
        task.didReceive(data); task.didFinish()
    }
}
