#if os(macOS)
import AppKit
import CryptoKit
import Foundation
import ImageIO
import UniformTypeIdentifiers

/// The local-picture subset of editor/paste P9. A supported representation is
/// snapshotted before any suspension. HTML, private field HTML, promises and
/// remote URLs are left to the full paste pipeline; none can fall through to
/// a Finder icon or an unrelated bitmap here.
struct FieldPicturePaste {
    let name: String
    let bytes: Data
    var isBitmap = false
    private static let maximumSize = 104_857_600
    private static let imageExtensions: Set<String> = ["jpg", "jpeg", "png", "gif", "webp", "ico", "avif"]

    static func read(_ board: NSPasteboard) throws -> [Self] {
        let items = board.pasteboardItems ?? []
        let unsupported = ["net.meguri.field-html", "public.html", "com.apple.webarchive", "public.rtf", "com.apple.flat-rtfd"]
        guard !items.contains(where: { item in unsupported.contains { item.types.contains(.init($0)) } }) else { return [] }
        let urls = items.compactMap { item -> URL? in
            for type in [NSPasteboard.PasteboardType.fileURL, .URL] {
                if let value = item.string(forType: type), let url = URL(string: value) { return url }
            }
            return nil
        }
        if let first = urls.first, first.isFileURL {
            // File URLs win over their icon/image representations (P9).
            // Refuse an unsupported item, without importing its Finder icon.
            guard urls.allSatisfy({ $0.isFileURL && ($0.host == nil || $0.host == "" || $0.host == "localhost") &&
                imageExtensions.contains($0.pathExtension.lowercased()) }) else { return [] }
            return try urls.map { url in
                let access = url.startAccessingSecurityScopedResource()
                defer { if access { url.stopAccessingSecurityScopedResource() } }
                let values = try url.resourceValues(forKeys: [.isRegularFileKey, .fileSizeKey])
                guard values.isRegularFile == true, let size = values.fileSize, size > 0, size <= maximumSize else {
                    throw CocoaError(.fileReadTooLarge, userInfo: [NSFilePathErrorKey: url.path])
                }
                let data = try Data(contentsOf: url)
                guard !data.isEmpty, data.count <= maximumSize, decodable(data) else { throw CocoaError(.fileReadCorruptFile) }
                return Self(name: url.lastPathComponent, bytes: data)
            }
        }
        for item in items {
            let preferred: [NSPasteboard.PasteboardType] = [.png, .tiff, .init("public.jpeg")]
            let types = preferred + item.types.filter { !preferred.contains($0) && UTType($0.rawValue)?.conforms(to: .image) == true }
            for type in types where item.types.contains(type) {
                guard let data = item.data(forType: type), !data.isEmpty, data.count <= maximumSize, decodable(data) else { continue }
                return [Self(name: "", bytes: data, isBitmap: true)]
            }
        }
        return []
    }

    private static func decodable(_ data: Data) -> Bool {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil) else { return false }
        return CGImageSourceGetCount(source) > 0 && CGImageSourceCreateImageAtIndex(source, 0, nil) != nil
    }

    /// P7: first frame, orientation applied, profile carried by CGImage's color
    /// space, no source metadata, JPEG over opaque white (default) or PNG.
    static func encode(_ data: Data, asPNG: Bool) throws -> Self {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil),
              let original = CGImageSourceCreateImageAtIndex(source, 0, nil),
              let image = CGImageSourceCreateThumbnailAtIndex(source, 0, [
                kCGImageSourceCreateThumbnailFromImageAlways: true,
                kCGImageSourceCreateThumbnailWithTransform: true,
                kCGImageSourceThumbnailMaxPixelSize: max(original.width, original.height)
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
        guard !bytes.isEmpty, bytes.count <= maximumSize else { throw CocoaError(.fileReadTooLarge) }
        let hash = Insecure.SHA1.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
        return Self(name: "paste-\(hash).\(asPNG ? "png" : "jpg")", bytes: bytes)
    }
}
#endif
