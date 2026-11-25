#if os(macOS)
import AppKit
import CoreFoundation
import CryptoKit
import Foundation
import ImageIO
import UniformTypeIdentifiers

/// Immutable local representations. Only snapshot() touches AppKit; selection,
/// disk access and image encoding run on a worker, one file at a time.
nonisolated struct FieldPicturePaste: Sendable {
    let name: String
    let bytes: Data
    static let maximumSyncSize = 104_857_600
    private static let imageExtensions: Set<String> = ["jpg", "jpeg", "png", "gif", "svg", "webp", "ico", "avif"]

    nonisolated struct Snapshot: Sendable {
        var urls: [URL] = []
        var bitmap: Data?
        var html: Data?
        var archive: Data?
        var text: String?
        var privateOrRichText = false
    }
    nonisolated enum Source: Sendable {
        case file(URL), bitmap(Data)
        var name: String { if case let .file(url) = self { return url.lastPathComponent }; return "" }
        var isBitmap: Bool { if case .bitmap = self { return true }; return false }
        func size() throws -> Int {
            switch self {
            case let .file(url):
                let access = url.startAccessingSecurityScopedResource()
                defer { if access { url.stopAccessingSecurityScopedResource() } }
                let values = try url.resourceValues(forKeys: [.isRegularFileKey, .fileSizeKey])
                guard values.isRegularFile == true, let size = values.fileSize else { throw CocoaError(.fileReadUnknown) }
                return size
            case let .bitmap(data): return data.count
            }
        }
        func prepare(asPNG: Bool) throws -> FieldPicturePaste {
            switch self {
            case let .file(url):
                let access = url.startAccessingSecurityScopedResource()
                defer { if access { url.stopAccessingSecurityScopedResource() } }
                // Files (including SVG) keep their bytes. They render through
                // the existing local-media img path, never as injected markup.
                return FieldPicturePaste(name: url.lastPathComponent, bytes: try Data(contentsOf: url))
            case let .bitmap(data): return try FieldPicturePaste.encode(data, asPNG: asPNG)
            }
        }
    }

    @MainActor static func snapshot(_ board: NSPasteboard) -> Snapshot {
        var result = Snapshot()
        let items = board.pasteboardItems ?? []
        // Private field content and actual rich selections retain precedence.
        result.privateOrRichText = items.contains { item in
            ["net.meguri.field-html", "public.rtf", "com.apple.flat-rtfd"].contains { item.types.contains(.init($0)) }
        }
        if result.privateOrRichText { return result }
        result.text = board.string(forType: .string) ?? board.string(forType: .init("public.plain-text"))
        for item in items {
            for type in [NSPasteboard.PasteboardType.fileURL, .URL] {
                if let value = item.string(forType: type), let url = URL(string: value) { result.urls.append(url); break }
            }
            if result.html == nil && result.archive == nil {
                result.html = item.data(forType: .html)
                if result.html == nil { result.archive = item.data(forType: .init("com.apple.webarchive")) }
            }
        }
        // A local file representation wins over every bitmap/icon offered
        // with it. Snapshot paths only; do not read all files into memory.
        if result.urls.first?.isFileURL == true { return result }
        for item in items {
            let preferred: [NSPasteboard.PasteboardType] = [.png, .tiff, .init("public.jpeg")]
            let types = preferred + item.types.filter { !preferred.contains($0) && UTType($0.rawValue)?.conforms(to: .image) == true }
            if let data = types.lazy.filter({ item.types.contains($0) }).compactMap({ item.data(forType: $0) }).first {
                result.bitmap = data; break
            }
        }
        return result
    }

    static func sources(_ snapshot: Snapshot) -> [Source] {
        guard !snapshot.privateOrRichText else { return [] }
        // An offered bitmap may accompany Copy Image's HTML or webarchive.
        // Inspect only enough to distinguish that from a meaningful selection;
        // no clipboard HTML, subresource or URL is ever inserted or fetched.
        var html = snapshot.html.map { String(decoding: $0, as: UTF8.self) }
        if html == nil, let data = snapshot.archive {
            guard let archive = try? PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any],
                  let main = archive["WebMainResource"] as? [String: Any],
                  main["WebResourceMIMEType"] as? String == "text/html",
                  let bytes = main["WebResourceData"] as? Data else { return [] }
            let charset = (main["WebResourceTextEncodingName"] as? String ?? "utf-8") as CFString
            let encoding = CFStringConvertIANACharSetNameToEncoding(charset)
            let nsEncoding = CFStringConvertEncodingToNSStringEncoding(encoding == kCFStringEncodingInvalidId ? CFStringBuiltInEncodings.UTF8.rawValue : encoding)
            html = String(data: bytes, encoding: String.Encoding(rawValue: nsEncoding)) ?? String(decoding: bytes, as: UTF8.self)
        }
        if let html {
            let body = html.replacingOccurrences(of: "(?is)<!--.*?-->|<(head|script|style)\\b[^>]*>.*?</\\1\\s*>", with: "", options: .regularExpression)
            guard let images = try? NSRegularExpression(pattern: "<img(?:\\s|>)", options: .caseInsensitive),
                  images.numberOfMatches(in: body, range: NSRange(body.startIndex..., in: body)) == 1 else { return [] }
            let text = body.replacingOccurrences(of: "(?s)<[^>]*>", with: "", options: .regularExpression)
                .replacingOccurrences(of: "&nbsp;", with: " ").trimmingCharacters(in: .whitespacesAndNewlines)
            guard text.isEmpty || snapshot.urls.contains(where: { $0.absoluteString == text }) else { return [] }
            if let text = snapshot.text?.trimmingCharacters(in: .whitespacesAndNewlines), !text.isEmpty,
               !snapshot.urls.contains(where: { $0.absoluteString == text }) { return [] }
        }
        if snapshot.urls.first?.isFileURL == true {
            return snapshot.urls.filter { $0.isFileURL && ($0.host == nil || $0.host == "" || $0.host == "localhost") &&
                imageExtensions.contains($0.pathExtension.lowercased()) }.map(Source.file)
        }
        return snapshot.bitmap.map { [.bitmap($0)] } ?? []
    }

    static func needsSizeWarning(_ size: Int) -> Bool { size == 0 || size > maximumSyncSize }

    /// P7: first frame, orientation applied, profile carried by CGImage's color
    /// space, no source metadata, JPEG over opaque white (default) or PNG.
    static func encode(_ data: Data, asPNG: Bool) throws -> Self {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil),
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
              let width = properties[kCGImagePropertyPixelWidth] as? Int,
              let height = properties[kCGImagePropertyPixelHeight] as? Int, width > 0, height > 0,
              let image = CGImageSourceCreateThumbnailAtIndex(source, 0, [
                kCGImageSourceCreateThumbnailFromImageAlways: true,
                kCGImageSourceCreateThumbnailWithTransform: true,
                kCGImageSourceThumbnailMaxPixelSize: max(width, height)
              ] as CFDictionary) else { throw CocoaError(.fileReadCorruptFile) }
        let result: CGImage
        if asPNG { result = image }
        else {
            let space = image.colorSpace?.model == .rgb ? image.colorSpace! : CGColorSpaceCreateDeviceRGB()
            guard let context = CGContext(data: nil, width: image.width, height: image.height, bitsPerComponent: 8,
                bytesPerRow: 0, space: space, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else {
                throw CocoaError(.fileReadCorruptFile)
            }
            let rect = CGRect(x: 0, y: 0, width: CGFloat(image.width), height: CGFloat(image.height))
            context.setFillColor(CGColor(gray: 1, alpha: 1)); context.fill(rect); context.draw(image, in: rect)
            guard let opaque = context.makeImage() else { throw CocoaError(.fileReadCorruptFile) }
            result = opaque
        }
        let output = NSMutableData()
        let type = asPNG ? UTType.png.identifier : UTType.jpeg.identifier
        guard let destination = CGImageDestinationCreateWithData(output as CFMutableData, type as CFString, 1, nil) else {
            throw CocoaError(.fileWriteUnknown)
        }
        let properties: [CFString: Any] = asPNG ? [:] : [kCGImageDestinationLossyCompressionQuality: 0.8,
            kCGImagePropertyJFIFDictionary: [kCGImagePropertyJFIFIsProgressive: false]]
        CGImageDestinationAddImage(destination, result, properties as CFDictionary)
        guard CGImageDestinationFinalize(destination) else { throw CocoaError(.fileWriteUnknown) }
        let bytes = output as Data
        guard !bytes.isEmpty else { throw CocoaError(.fileWriteUnknown) }
        let hash = Insecure.SHA1.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
        return Self(name: "paste-\(hash).\(asPNG ? "png" : "jpg")", bytes: bytes)
    }
}
#endif
