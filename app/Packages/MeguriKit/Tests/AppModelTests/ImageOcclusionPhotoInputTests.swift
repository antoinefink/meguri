import CoreGraphics
import Foundation
import ImageIO
import Testing
import UniformTypeIdentifiers
@testable import Editor

@MainActor
@Suite struct ImageOcclusionPhotoInputTests {
    private static func bitmap(_ type: UTType = .png, width: Int = 80, height: Int = 40,
                               orientation: Int = 1, transparent: Bool = false) throws -> Data {
        let context = try #require(CGContext(data: nil, width: width, height: height, bitsPerComponent: 8,
            bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        if !transparent {
            context.setFillColor(CGColor(gray: 0.5, alpha: 1))
            context.fill(CGRect(x: 0, y: 0, width: CGFloat(width), height: CGFloat(height)))
        }
        let bytes = NSMutableData()
        let destination = try #require(CGImageDestinationCreateWithData(bytes as CFMutableData,
            type.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(destination, try #require(context.makeImage()),
            [kCGImagePropertyOrientation: orientation] as CFDictionary)
        try #require(CGImageDestinationFinalize(destination))
        return bytes as Data
    }

    private static func storedPixels(_ input: OcclusionImageInput, type: UTType) throws -> CGImage {
        let source = try #require(CGImageSourceCreateWithData(input.bytes as CFData, nil))
        #expect(CGImageSourceGetType(source) as String? == type.identifier)
        #expect(input.name.hasPrefix("paste-") && input.name.hasSuffix(type == .png ? ".png" : ".jpg"))
        return try OcclusionImageInput.decodeBitmap(input.bytes, maximumDimension: nil)
    }

    @Test func photoLimitAndOffKeepIndependentStoredAndPreviewSizes() async throws {
        let bytes = try Self.bitmap(width: 8192, height: 32)
        for (limit, storedWidth, previewWidth) in [(1024 as Int?, 1024, 1024), (nil, 8192, 4096)] {
            let input = try await OcclusionImageInput.prepare(bytes, name: nil, asPNG: true, maximumDimension: limit)
            let stored = try Self.storedPixels(input, type: .png)
            #expect(stored.width == storedWidth && stored.height == storedWidth / 256)
            #expect(input.pixels.width == previewWidth && input.pixels.height == previewWidth / 256)
        }
        let small = try await OcclusionImageInput.prepare(Self.bitmap(), name: nil, asPNG: true, maximumDimension: 640)
        #expect(small.size == CGSize(width: 80, height: 40))
    }

    @Test func jpegFlattensTransparencyOverWhiteAndPNGKeepsAlpha() async throws {
        let bytes = try Self.bitmap(transparent: true)
        for asPNG in [false, true] {
            let input = try await OcclusionImageInput.prepare(bytes, name: nil, asPNG: asPNG, maximumDimension: 20)
            let stored = try Self.storedPixels(input, type: asPNG ? .png : .jpeg)
            let context = try #require(CGContext(data: nil, width: 1, height: 1, bitsPerComponent: 8,
                bytesPerRow: 4, space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGBitmapInfo.byteOrder32Big.rawValue | CGImageAlphaInfo.premultipliedLast.rawValue))
            context.draw(stored, in: CGRect(x: 0, y: 0, width: 1, height: 1))
            let pixel = try #require(context.data).assumingMemoryBound(to: UInt8.self)
            if asPNG { #expect(pixel[3] == 0) }
            else { #expect(pixel[0] > 245 && pixel[1] > 245 && pixel[2] > 245 && pixel[3] == 255) }
        }
    }

    @Test func compatibleOriginalsKeepTheirNamesAndBytesDespitePhotoSettings() async throws {
        for (type, names) in [(UTType.jpeg, ["Photo.JPG", "Photo.JPEG"]), (.png, ["Diagram.PNG"]), (.gif, ["Animation.gif"])] {
            let bytes = try Self.bitmap(type)
            for name in names {
                let input = try await OcclusionImageInput.prepare(bytes, name: name, asPNG: true, maximumDimension: 20)
                #expect(input.name == name && input.bytes == bytes)
                #expect(input.size == CGSize(width: 80, height: 40))
            }
        }
    }
}
