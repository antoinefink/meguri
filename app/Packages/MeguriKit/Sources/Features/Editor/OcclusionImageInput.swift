import CoreBridge
import CoreGraphics
import CryptoKit
import Foundation
import ImageIO
import UniformTypeIdentifiers

/// Decoding and encoding are shared by every IO source. Only a validated image
/// reaches the collection media writer; unsupported file extensions convert.
nonisolated struct OcclusionImageInput: Sendable {
    let name: String
    let bytes: Data
    let pixels: CGImage
    var size: CGSize { CGSize(width: CGFloat(pixels.width), height: CGFloat(pixels.height)) }

    // Use the mask editor's existing 4096 × 4096 raster budget for previews.
    // ImageIO fits the source proportionally; stored file bytes stay original.
    static let previewMaximumDimension = 4096

    static func decode(_ bytes: Data, maximumDimension: Int? = OcclusionImageInput.previewMaximumDimension) async throws -> CGImage {
        if let image = try? decodeBitmap(bytes, maximumDimension: maximumDimension) { return image }
        return try await OcclusionSVGDecoder.decode(bytes, maximumDimension: maximumDimension)
    }

    static func decodeBitmap(_ bytes: Data, maximumDimension: Int? = OcclusionImageInput.previewMaximumDimension) throws -> CGImage {
        guard (maximumDimension ?? 1) > 0,
              let source = CGImageSourceCreateWithData(bytes as CFData, [kCGImageSourceShouldCache: false] as CFDictionary),
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
              let width = properties[kCGImagePropertyPixelWidth] as? Int,
              let height = properties[kCGImagePropertyPixelHeight] as? Int, width > 0, height > 0,
              let pixels = CGImageSourceCreateThumbnailAtIndex(source, 0, [
                kCGImageSourceCreateThumbnailFromImageAlways: true,
                kCGImageSourceCreateThumbnailWithTransform: true,
                kCGImageSourceThumbnailMaxPixelSize: min(max(width, height), maximumDimension ?? max(width, height))
              ] as CFDictionary) else { throw CocoaError(.fileReadCorruptFile) }
        return pixels
    }

    static func prepare(_ bytes: Data, name: String?, asPNG: Bool, maximumDimension: Int? = nil) async throws -> Self {
        if let name, OcclusionImageField.isUsableName(name) {
            return Self(name: name, bytes: bytes, pixels: try await decode(bytes))
        }
        // Conversion still honors P7's output-size setting, including Off.
        // The preview ceiling must not silently reduce the written image.
        var pixels = try await decode(bytes, maximumDimension: nil)
        if let maximumDimension, maximumDimension > 0, max(pixels.width, pixels.height) > maximumDimension {
            let scale = Double(maximumDimension) / Double(max(pixels.width, pixels.height))
            pixels = try render(pixels, width: max(1, Int((Double(pixels.width) * scale).rounded())),
                                height: max(1, Int((Double(pixels.height) * scale).rounded())), opaque: !asPNG)
        } else if !asPNG { pixels = try render(pixels, width: pixels.width, height: pixels.height, opaque: true) }
        let output = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(output as CFMutableData,
            (asPNG ? UTType.png.identifier : UTType.jpeg.identifier) as CFString, 1, nil) else {
            throw CocoaError(.fileWriteUnknown)
        }
        let options: [CFString: Any] = asPNG ? [:] : [kCGImageDestinationLossyCompressionQuality: 0.8,
            kCGImagePropertyJFIFDictionary: [kCGImagePropertyJFIFIsProgressive: false]]
        CGImageDestinationAddImage(destination, pixels, options as CFDictionary)
        guard CGImageDestinationFinalize(destination) else { throw CocoaError(.fileWriteUnknown) }
        let data = output as Data
        let hash = Insecure.SHA1.hash(data: data).map { String(format: "%02x", $0) }.joined()
        let preview = max(pixels.width, pixels.height) > previewMaximumDimension ? try await decode(data) : pixels
        return Self(name: "paste-\(hash).\(asPNG ? "png" : "jpg")", bytes: data, pixels: preview)
    }

    private static func render(_ image: CGImage, width: Int, height: Int, opaque: Bool) throws -> CGImage {
        let space = image.colorSpace?.model == .rgb ? image.colorSpace! : CGColorSpaceCreateDeviceRGB()
        guard let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0,
            space: space, bitmapInfo: (opaque ? CGImageAlphaInfo.noneSkipLast : .premultipliedLast).rawValue) else {
            throw CocoaError(.fileReadCorruptFile)
        }
        let rect = CGRect(x: 0, y: 0, width: CGFloat(width), height: CGFloat(height))
        if opaque { context.setFillColor(CGColor(gray: 1, alpha: 1)); context.fill(rect) }
        context.interpolationQuality = .high
        context.draw(image, in: rect)
        guard let result = context.makeImage() else { throw CocoaError(.fileReadCorruptFile) }
        return result
    }

    static func readFile(_ url: URL) throws -> Data {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        guard try url.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile == true else {
            throw CocoaError(.fileReadUnsupportedScheme)
        }
        return try Data(contentsOf: url)
    }
}
