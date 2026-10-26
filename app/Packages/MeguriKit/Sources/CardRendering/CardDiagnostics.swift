import Foundation

/// Bounded, technical receipts. Never store HTML, typed text or absolute paths.
public final class CardDiagnostics {
    public nonisolated struct ScriptLocation: Sendable, Equatable {
        public let errorClass: String
        public let line: Int?
        public let column: Int?
        public let source: String
    }
    public nonisolated struct Entry: Sendable, Equatable {
        public let update: CardUpdate?
        public let kind: String
        public let name: String
        public let index: Int?
        public let script: ScriptLocation?
        public var repeats: Int
        public let bridgeRejectionReason: CardBridge.RejectionReason?

        init(update: CardUpdate?, kind: String, name: String, index: Int?, script: ScriptLocation?, repeats: Int,
             bridgeRejectionReason: CardBridge.RejectionReason? = nil) {
            self.update = update
            self.kind = kind
            self.name = name
            self.index = index
            self.script = script
            self.repeats = repeats
            self.bridgeRejectionReason = bridgeRejectionReason
        }
    }

    /// Private user content, in memory only. Deliberately not Codable and
    /// separate from sanitized technical receipts. Never log/export this state.
    public nonisolated struct TemplateError: Sendable, Equatable {
        public let update: CardUpdate
        public let templateOrdinal: Int
        public let templateDescriptor: String?
        public let kind: String
        public let phase: String
        public let message: String
        public let line: Int?
        public let column: Int?
        public let scriptIndex: Int?
    }
    public private(set) var templateErrors: [TemplateError] = []
    func captureTemplateError(update: CardUpdate, payload: CardPayload, kind: String, phase: String,
                              message: String, line: Int?, column: Int?, index: Int?) {
        let error = TemplateError(update: update, templateOrdinal: payload.ordinal, templateDescriptor: payload.templateDescriptor,
                                  kind: kind, phase: phase, message: String(message.prefix(2_000)), line: line, column: column, scriptIndex: index)
        guard !templateErrors.contains(error) else { return }
        if templateErrors.count == 64 { templateErrors.removeFirst() }
        templateErrors.append(error)
    }

    public private(set) var entries: [Entry] = []
    public init() {}

    public func record(_ kind: String, update: CardUpdate? = nil, name: String = "", index: Int? = nil, script: ScriptLocation? = nil,
                       bridgeRejectionReason: CardBridge.RejectionReason? = nil) {
        // Page error text can contain private fields. Keep its technical class,
        // not arbitrary exception messages/source excerpts or URL credentials.
        let safeName: String
        if name.contains("://") {
            safeName = URL(string: name).flatMap { $0.scheme == "meguri-media" ? $0.lastPathComponent : nil } ?? "external-resource"
        } else {
            safeName = name.split(separator: "/").last.map(String.init) ?? ""
        }
        let bounded = String(safeName.prefix(240))
        if let i = entries.firstIndex(where: { $0.kind == kind && $0.update == update && $0.name == bounded && $0.index == index && $0.script == script && $0.bridgeRejectionReason == bridgeRejectionReason }) {
            entries[i].repeats = min(entries[i].repeats + 1, 1_000_000)
            return
        }
        if entries.count == 256 { entries.removeFirst() }
        entries.append(Entry(update: update, kind: String(kind.prefix(64)), name: bounded, index: index, script: script, repeats: 1,
                             bridgeRejectionReason: bridgeRejectionReason))
    }

    func scriptLocation(message: String, line: Int?, column: Int?, source: String?) -> ScriptLocation {
        let prefix = message.split(separator: ":", maxSplits: 1).first.map(String.init) ?? ""
        let errorClass = ["Error", "TypeError", "SyntaxError", "RangeError", "ReferenceError", "URIError", "EvalError"].contains(prefix) ? prefix : "Error"
        let sourceName: String
        if let source, let url = URL(string: source), url.scheme == MediaOrigin.scheme, url.host == MediaOrigin.host {
            sourceName = String(url.lastPathComponent.prefix(240))
        } else { sourceName = source == "inline" ? "inline" : "external-resource" }
        return ScriptLocation(errorClass: errorClass, line: line, column: column, source: sourceName)
    }
}
