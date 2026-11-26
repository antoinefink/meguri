import Foundation

/// Logical positions, never raw stored ordinals (`image-occlusion-format` F2).
public nonisolated struct OcclusionFieldRoles: Sendable, Equatable {
    public let occlusions: Int
    public let image: Int
    public let header: Int
    public let backExtra: Int
    public var hidden: Set<Int> { [occlusions, image] }

    public init?(notetype: NotetypeRecord) {
        guard notetype.originalStockKind == 6, notetype.fields.count >= 4 else { return nil }
        let tagged = (0..<4).map { tag in notetype.fields.firstIndex { $0.tag == UInt32(tag) } }
        let indices = tagged.allSatisfy { $0 != nil } ? tagged.map { $0! } : Array(0..<4)
        occlusions = indices[0]; image = indices[1]; header = indices[2]; backExtra = indices[3]
    }
}

public nonisolated enum OcclusionImageField {
    public static let extensions: Set<String> = ["jpg", "jpeg", "png", "gif", "svg", "webp", "ico", "avif"]
    public static func isUsableName(_ name: String) -> Bool {
        extensions.contains((name as NSString).pathExtension.lowercased())
    }
    /// F13 intentionally reads raw names; percent escapes and entities stay raw.
    public static func name(in field: String) -> String? {
        guard let tags = try? NSRegularExpression(pattern: #"<img\s[^>]*>"#),
              let sources = try? NSRegularExpression(pattern: #"src\s*=\s*"([^"]+)""#) else { return nil }
        let text = field as NSString
        for tag in tags.matches(in: field, range: NSRange(location: 0, length: text.length)) {
            let html = text.substring(with: tag.range), value = html as NSString
            if let match = sources.matches(in: html, range: NSRange(location: 0, length: value.length)).last {
                return value.substring(with: match.range(at: 1))
            }
        }
        return nil
    }
    /// O4.2 uses the stored name verbatim, as the normal image insertion does.
    public static func html(name: String) -> String { "<img src=\"\(name)\">" }
}
