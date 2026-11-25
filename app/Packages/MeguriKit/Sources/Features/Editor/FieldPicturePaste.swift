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
        var bitmaps: [Data] = []
        var html: Data?
        var archive: Data?
        var text: String?
    }
    nonisolated enum Source: Sendable {
        case file(URL), bitmap([Data])
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
            case let .bitmap(candidates): return candidates.reduce(0) { $0 + $1.count }
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
            case let .bitmap(candidates):
                for data in candidates {
                    if let picture = try? FieldPicturePaste.encode(data, asPNG: asPNG) { return picture }
                }
                throw CocoaError(.fileReadCorruptFile)
            }
        }
    }

    @MainActor static func snapshot(_ board: NSPasteboard) -> Snapshot {
        var result = Snapshot()
        let items = board.pasteboardItems ?? []
        // Private field content and actual rich selections retain precedence.
        if items.contains(where: { item in
            ["net.meguri.field-html", "public.rtf", "com.apple.flat-rtfd"].contains { item.types.contains(.init($0)) }
        }) { return result }
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
            result.bitmaps += types.filter { item.types.contains($0) }.compactMap { item.data(forType: $0) }
        }
        return result
    }

    // Decode only a bounded main HTML resource, never archive subresources.
    static func htmlText(_ snapshot: Snapshot) -> String? {
        let maximum = 1_048_576
        if let data = snapshot.html {
            guard data.count <= maximum else { return nil }
            return String(decoding: data, as: UTF8.self)
        }
        guard let data = snapshot.archive, data.count <= 8 * maximum,
              let archive = try? PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any],
              let main = archive["WebMainResource"] as? [String: Any],
              main["WebResourceMIMEType"] as? String == "text/html",
              let bytes = main["WebResourceData"] as? Data, bytes.count <= maximum else { return nil }
        let charset = (main["WebResourceTextEncodingName"] as? String ?? "utf-8") as CFString
        let encoding = CFStringConvertIANACharSetNameToEncoding(charset)
        let nsEncoding = CFStringConvertEncodingToNSStringEncoding(encoding == kCFStringEncodingInvalidId ? CFStringBuiltInEncodings.UTF8.rawValue : encoding)
        return String(data: bytes, encoding: String.Encoding(rawValue: nsEncoding)) ?? String(decoding: bytes, as: UTF8.self)
    }

    static func sources(_ snapshot: Snapshot, isCopyImage: Bool = false) -> [Source] {
        // The isolated runtime classifies HTML using inert element nodes.
        // Only the local bitmap is imported; clipboard resources never load.
        if snapshot.html != nil || snapshot.archive != nil {
            guard isCopyImage else { return [] }
            if let text = snapshot.text?.trimmingCharacters(in: .whitespacesAndNewlines), !text.isEmpty,
               !snapshot.urls.contains(where: { $0.absoluteString == text }) { return [] }
        }
        if snapshot.urls.first?.isFileURL == true {
            return snapshot.urls.filter { $0.isFileURL && ($0.host == nil || $0.host == "" || $0.host == "localhost") &&
                imageExtensions.contains($0.pathExtension.lowercased()) }.map(Source.file)
        }
        return snapshot.bitmaps.isEmpty ? [] : [.bitmap(snapshot.bitmaps)]
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
        let destinationProperties: [CFString: Any] = asPNG ? [:] : [kCGImageDestinationLossyCompressionQuality: 0.8,
            kCGImagePropertyJFIFDictionary: [kCGImagePropertyJFIFIsProgressive: false]]
        CGImageDestinationAddImage(destination, result, destinationProperties as CFDictionary)
        guard CGImageDestinationFinalize(destination) else { throw CocoaError(.fileWriteUnknown) }
        let bytes = output as Data
        guard !bytes.isEmpty else { throw CocoaError(.fileWriteUnknown) }
        let hash = Insecure.SHA1.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
        return Self(name: "paste-\(hash).\(asPNG ? "png" : "jpg")", bytes: bytes)
    }
}
#endif
