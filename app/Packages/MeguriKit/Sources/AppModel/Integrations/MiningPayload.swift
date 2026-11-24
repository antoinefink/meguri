#if os(macOS)
import CoreBridge
import CryptoKit
import Foundation

nonisolated struct MiningFailure: LocalizedError, Sendable {
    var message: String
    var errorDescription: String? { message }
    init(_ message: String) { self.message = message }
}
nonisolated struct MiningPayload: Sendable {
    nonisolated struct Item: Sendable { var id: String; var source: String? }
    var text: [String: String]
    var items: [String: [Item]]
    var digest: String
    var retryKey: String
    static let textKeys = ["targetWord", "sentence", "translation", "definitions", "exampleSentences", "notes"]
    init(_ data: Data) throws {
        guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any] else { throw MiningFailure("Expected a JSON object.") }
        text = [:]; items = [:]; digest = ""; retryKey = ""
        for key in Self.textKeys {
            let value = object[key]
            guard value == nil || value is NSNull || value is String else { throw MiningFailure("\(key) must be text or null.") }
            let string = value as? String ?? ""
            guard !string.contains("__meguri_mining_attachment_") else { throw MiningFailure("Reserved media reference in field text.") }
            // Preserve ordinary study markup; reject explicit active/resource
            // markup. Media acquisition itself accepts embedded bytes only.
            guard !Self.hasRemoteResource(string) else { throw MiningFailure("Remote resources in card HTML are not supported. Send embedded media instead.") }
            text[key] = string
        }
        for key in ["sentenceAudio", "wordAudio", "images"] {
            let value = object[key]
            guard value == nil || value is NSNull || value is [Any] else { throw MiningFailure("\(key) must be an array or null.") }
            let array = value as? [Any] ?? []
            guard array.count <= 256 else { throw MiningFailure("Too many media items.") }
            items[key] = try array.map { value in
                guard let object = value as? [String: Any] else { throw MiningFailure("\(key) items must be objects.") }
                return Item(id: object["id"] as? String ?? "", source: object[key == "images" ? "src" : "input"] as? String)
            }
        }
        digest = Self.hash(try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]))
        let ids = ["sentenceAudio", "wordAudio", "images"].flatMap { items[$0, default: []].map(\.id) }
        let identity = [text["targetWord", default: ""], text["sentence", default: ""]] + ids.sorted()
        if ids.contains("") || (ids.isEmpty && identity.allSatisfy(\.isEmpty)) { retryKey = Self.hash(Data(UUID().uuidString.utf8)) }
        else { retryKey = Self.hash(try JSONEncoder().encode(identity)) }
    }
    static func hash(_ data: Data) -> String { SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined() }
    static func hasRemoteResource(_ text: String) -> Bool {
        // Rendering arbitrary active content is outside this local receiver's
        // grant. Preserve ordinary study markup, reject active/network markup.
        text.range(of: #"(?is)<\s*(script|iframe|object|embed|link|style|base|meta)\b|\b(src|srcset|poster|background)\s*=\s*["']?\s*(https?:|//)|url\s*\(|@import"#, options: .regularExpression) != nil
    }
    func fields(for type: NotetypeRecord) throws -> ([String], [MiningAttachment]) {
        guard type.kind == 0, type.originalStockKind != 6 else { throw MiningFailure("Choose a standard note type with a sentence or Front field. Cloze and image occlusion need the editor.") }
        let map = MiningFieldMap.infer(type.fields.map(\.name))
        guard map.contains(where: { $0 != nil }) else { throw MiningFailure("No fields to map to.") }
        var values = map.map { source in source.flatMap { text[$0] } ?? "" }
        var attachments: [MiningAttachment] = []
        func first(_ source: String) -> Int? { map.firstIndex { $0 == source } }
        func append(_ value: String, at index: Int) {
            if !values[index].isEmpty { values[index] += "\n<br>\n" }; values[index] += value
        }
        for key in ["sentenceAudio", "wordAudio", "images"] {
            for (index, item) in items[key, default: []].enumerated() {
                let destination: Int?
                if key == "images" {
                    destination = first("images") ?? (index == 0 ? first("firstImage") : first("restImages")) ?? first("sentence")
                } else { destination = first(key) ?? first(key == "wordAudio" ? "targetWord" : "sentence") ?? first("sentence") }
                guard let destination else { throw MiningFailure("No destination field for \(key). Choose a note type with a Sentence or Front field.") }
                guard let source = item.source else { throw MiningFailure("Could not read \(key) item \(index + 1): missing media data.") }
                let media = try Self.media(source, key: key, index: index + 1)
                let marker = "__meguri_mining_attachment_\(attachments.count)__"
                attachments.append(media)
                append(key == "images" ? "<img src=\"\(marker)\">" : "[sound:\(marker)]", at: destination)
            }
        }
        guard !values.allSatisfy(\.isEmpty) else { throw MiningFailure("There is nothing to add.") }
        let defaults = MiningFieldMap.defaults(typeName: type.name, fields: type.fields.map(\.name))
        for index in values.indices where values[index].isEmpty { values[index] = defaults[index] }
        return (values, attachments)
    }
    static func media(_ source: String, key: String, index: Int) throws -> MiningAttachment {
        func fail(_ reason: String) -> MiningFailure { MiningFailure("Could not read \(key) item \(index): \(reason)") }
        guard source.hasPrefix("data:"), let comma = source.firstIndex(of: ",") else {
            throw fail("only embedded base64 media is supported; remote URLs are not fetched.")
        }
        let header = source[source.index(source.startIndex, offsetBy: 5)..<comma].lowercased()
        guard header.hasSuffix(";base64") else { throw fail("invalid base64 data URL.") }
        let mime = String(header.dropLast(7))
        let types = ["audio/mp4": "m4a", "audio/m4a": "m4a", "audio/x-m4a": "m4a", "audio/mpeg": "mp3", "audio/mp3": "mp3",
            "audio/ogg": "ogg", "audio/opus": "ogg", "audio/webm": "webm", "audio/wav": "wav", "audio/x-wav": "wav", "audio/flac": "flac", "audio/aac": "aac",
            "image/png": "png", "image/jpeg": "jpg", "image/jpg": "jpg", "image/webp": "webp", "image/gif": "gif", "image/avif": "avif", "image/heic": "heic", "image/heif": "heic", "image/bmp": "bmp"]
        guard let ext = types[mime], (key == "images") == mime.hasPrefix("image/") else { throw fail("unsupported media type \(mime).") }
        let encoded = source[source.index(after: comma)...].filter { !$0.isWhitespace }
        guard let bytes = Data(base64Encoded: String(encoded)), !bytes.isEmpty, bytes.count <= MiningHTTP.bodyLimit else { throw fail("invalid or oversized base64 media.") }
        return MiningAttachment(name: Self.hash(bytes) + "." + ext, bytes: bytes)
    }
}

public nonisolated enum MiningFieldMap {
    /// migaku-routing D5/D6: only a language explicitly named by the imported
    /// type supplies these defaults. Unknown names keep every flag empty.
    public static func defaults(typeName: String, fields: [String]) -> [String] {
        func names(_ language: String) -> Bool {
            typeName.range(of: "(?i)(?<![A-Za-z])" + language + "(?![A-Za-z])", options: .regularExpression) != nil
        }
        let vocabulary = names("German") || names("Spanish")
        let audio = vocabulary || names("Japanese")
        return fields.map { field in
            let name = field.precomposedStringWithCanonicalMapping.lowercased()
            if name == "is vocabulary card", vocabulary { return "x" }
            if name == "is audio card", audio { return "x" }
            return ""
        }
    }

    public static func infer(_ names: [String]) -> [String?] {
        let normalized = names.map { $0.precomposedStringWithCanonicalMapping.lowercased()
            .replacingOccurrences(of: "_", with: " ").replacingOccurrences(of: "-", with: " ")
            .split(whereSeparator: \.isWhitespace).joined(separator: " ") }
        let screenshot = normalized.contains { !$0.hasPrefix("is ") && $0.contains("screenshot") }
        var used = Set<String>()
        return normalized.map { name in
            let source: String?
            if name.hasPrefix("is ") { source = nil }
            else if name.contains("sentence") && name.contains("audio") { source = "sentenceAudio" }
            else if name.contains("audio") && (name.contains("word") || name.contains("vocab")) { source = "wordAudio" }
            else if name.contains("screenshot") { source = "firstImage" }
            else if name.contains("image") || name.contains("picture") { source = screenshot ? "restImages" : "images" }
            else if name.contains("example") { source = "exampleSentences" }
            else if name.contains("translation") || name == "back" || name == "answer" { source = "translation" }
            else if name.contains("definition") { source = "definitions" }
            else if name.contains("sentence") || name == "front" || name == "question" || name == "expression" { source = "sentence" }
            else if name.contains("word") || name.contains("vocab") { source = "targetWord" }
            else if name == "note" || name == "notes" { source = "notes" }
            else if name == "audio" { source = "sentenceAudio" }
            else { source = nil }
            guard let source, used.insert(source).inserted else { return nil }; return source
        }
    }
}
#endif
