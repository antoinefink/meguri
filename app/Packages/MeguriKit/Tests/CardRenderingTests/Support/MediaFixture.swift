import CardRendering
import Foundation
import WebKit

/// Filesystem fixtures and observations only; all serving is production code.
final class MediaFixture {
    nonisolated struct Request: Equatable, CustomStringConvertible {
        let url: String
        let path: String
        let range: String?
        let status: Int
        let contentRange: String?
        let bodyLength: Int
        var description: String { "\(path) range=\(range ?? "-") → \(status) \(contentRange ?? "") \(bodyLength) bytes" }
    }
    let root: URL
    let handler = MediaSchemeHandler()
    private var resolver: MediaPathResolver?
    private var names: Set<String> = []
    private(set) var requests: [Request] = []

    init() {
        root = FileManager.default.temporaryDirectory.appending(path: "meguri-media-\(UUID())", directoryHint: .isDirectory)
        try! FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        handler.onResponse = { [weak self] receipt in
            self?.requests.append(Request(url: receipt.url.absoluteString, path: receipt.url.path(percentEncoded: false),
                                          range: receipt.range, status: receipt.status, contentRange: receipt.contentRange,
                                          bodyLength: Int(receipt.bodyLength)))
        }
    }
    isolated deinit {
        handler.close()
        try? FileManager.default.removeItem(at: root)
    }
    func add(_ path: String, _ data: Data, _ mime: String) {
        try! addThrowing(path, data, mime)
    }
    func addThrowing(_ path: String, _ data: Data, _ mime: String) throws {
        let name = String(path.dropFirst())
        precondition(!name.contains("/"))
        try data.write(to: root.appending(path: name), options: .atomic)
        names.insert(name)
    }
    func add(_ path: String, text: String, _ mime: String) { add(path, Data(text.utf8), mime) }
    func requests(for path: String) -> [Request] { requests.filter { $0.path == path } }
    func bind(_ page: WKWebView, document: String) async throws {
        if resolver == nil {
            resolver = try await MediaPathResolver.open(root: root, trackedNames: names.map { .init(name: $0, caseKey: $0.lowercased()) })
        }
        handler.bind(page, to: .init(resolver: resolver!, document: document, diagnostics: CardDiagnostics()))
    }
    func resolved() async throws -> MediaPathResolver {
        if let resolver { return resolver }
        let result = try await MediaPathResolver.open(root: root, trackedNames: names.map { .init(name: $0, caseKey: $0.lowercased()) })
        resolver = result
        return result
    }
}
