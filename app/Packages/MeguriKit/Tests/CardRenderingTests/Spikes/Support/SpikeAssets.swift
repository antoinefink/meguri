import AVFoundation
import CoreGraphics
import CoreText
import Foundation
import ImageIO
import UniformTypeIdentifiers

/// Media the spikes serve, made at run time so no binary fixture is
/// committed (U65).
enum SpikeAssets {
    // MARK: Audio

    /// A constant-bitrate MP3 of silence at least `minimumBytes` long: MPEG-1
    /// Layer III, 128 kbit/s, 44.1 kHz, mono, no ID3 tag. Each frame is a
    /// header and zeroed side information and main data, which decodes to
    /// silence, so playing it in a test makes no sound.
    static func silentMP3(minimumBytes: Int) -> (data: Data, duration: TimeInterval) {
        // Header: sync, MPEG-1, Layer III, no CRC | 128 kbit/s, 44.1 kHz, no
        // padding | mono, original.
        let header: [UInt8] = [0xFF, 0xFB, 0x90, 0xC4]
        // 144 × 128000 / 44100 = 417 bytes per frame without padding.
        let frameLength = 417
        var frame = [UInt8](repeating: 0, count: frameLength)
        frame.replaceSubrange(0..<4, with: header)
        let frames = (minimumBytes + frameLength - 1) / frameLength
        var data = Data(capacity: frames * frameLength)
        for _ in 0..<frames { data.append(contentsOf: frame) }
        return (data, Double(frames) * 1152 / 44100)
    }

    // MARK: Images

    /// A PNG of seeded noise, which PNG cannot compress: at least
    /// `minimumBytes` long.
    static func noisePNG(minimumBytes: Int, seed: UInt64 = 0x9E37_79B9_7F4A_7C15) throws -> Data {
        // PNG keeps three bytes a pixel (no alpha), which noise leaves
        // uncompressed: a little over the target.
        let side = Int((Double(minimumBytes) / 3).squareRoot().rounded(.up)) + 16
        var state = seed
        var pixels = [UInt8](repeating: 0, count: side * side * 4)
        for index in stride(from: 0, to: pixels.count, by: 4) {
            // xorshift64*
            state ^= state >> 12
            state ^= state << 25
            state ^= state >> 27
            let value = state &* 0x2545_F491_4F6C_DD1D
            pixels[index] = UInt8(truncatingIfNeeded: value)
            pixels[index + 1] = UInt8(truncatingIfNeeded: value >> 8)
            pixels[index + 2] = UInt8(truncatingIfNeeded: value >> 16)
            pixels[index + 3] = 255
        }
        let image = try pixels.withUnsafeMutableBytes { buffer -> CGImage in
            guard
                let context = CGContext(
                    data: buffer.baseAddress, width: side, height: side, bitsPerComponent: 8, bytesPerRow: side * 4,
                    space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue),
                let image = context.makeImage()
            else { throw Spike.Failure("could not draw the noise image") }
            return image
        }
        let data = try png(image)
        guard data.count >= minimumBytes else { throw Spike.Failure("the noise PNG is only \(data.count) bytes") }
        return data
    }

    /// A small solid PNG.
    static func solidPNG(side: Int = 8) throws -> Data {
        guard
            let context = CGContext(
                data: nil, width: side, height: side, bitsPerComponent: 8, bytesPerRow: 0,
                space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)
        else { throw Spike.Failure("could not draw a PNG") }
        context.setFillColor(CGColor(srgbRed: 0.2, green: 0.5, blue: 0.8, alpha: 1))
        context.fill(CGRect(x: 0, y: 0, width: side, height: side))
        guard let image = context.makeImage() else { throw Spike.Failure("could not draw a PNG") }
        return try png(image)
    }

    private static func png(_ image: CGImage) throws -> Data {
        let output = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(output, UTType.png.identifier as CFString, 1, nil) else {
            throw Spike.Failure("no PNG encoder")
        }
        CGImageDestinationAddImage(destination, image, nil)
        guard CGImageDestinationFinalize(destination) else { throw Spike.Failure("PNG encoding failed") }
        return output as Data
    }

    // MARK: Fonts

    /// System families whose faces the font tests serve as web fonts.
    static let fontFamilies = [
        "Georgia", "Verdana", "Courier New", "Times New Roman", "Arial", "Trebuchet MS",
        "Helvetica Neue", "Avenir Next", "Menlo", "Palatino", "Gill Sans", "Futura",
    ]

    /// `count` distinct system faces, each rebuilt as a standalone sfnt file
    /// (TrueType or OpenType) from its tables, so a collection face works
    /// too. Read at run time, never committed.
    static func fonts(_ count: Int) throws -> [Data] {
        var faces: [Data] = []
        var seen: Set<String> = []
        for family in fontFamilies where faces.count < count {
            let font = CTFontCreateWithName(family as CFString, 16, nil)
            let name = CTFontCopyPostScriptName(font) as String
            guard seen.insert(name).inserted else { continue }
            faces.append(try sfnt(font))
        }
        guard faces.count == count else { throw Spike.Failure("only \(faces.count) distinct system faces, \(count) needed") }
        return faces
    }

    /// The font's tables as one sfnt file, with the table directory,
    /// checksums and head adjustment an sfnt reader expects.
    static func sfnt(_ font: CTFont) throws -> Data {
        // The array holds the tags themselves, not CFNumbers.
        guard let array = CTFontCopyAvailableTables(font, []), CFArrayGetCount(array) > 0 else {
            throw Spike.Failure("\(CTFontCopyPostScriptName(font)) has no tables")
        }
        let tags = (0..<CFArrayGetCount(array)).map { CTFontTableTag(UInt(bitPattern: CFArrayGetValueAtIndex(array, $0))) }
        var tables: [(tag: UInt32, data: Data)] = []
        for tag in tags.sorted() {
            guard let table = CTFontCopyTable(font, tag, []) as Data? else { continue }
            tables.append((tag, table))
        }
        func fourCC(_ text: String) -> UInt32 { text.utf8.reduce(0) { $0 << 8 | UInt32($1) } }
        let isCFF = tables.contains { $0.tag == fourCC("CFF ") || $0.tag == fourCC("CFF2") }
        var out = Data()
        func u16(_ value: Int) { out.append(contentsOf: [UInt8(value >> 8 & 0xFF), UInt8(value & 0xFF)]) }
        func u32(_ value: UInt32) { out.append(contentsOf: [UInt8(value >> 24), UInt8(value >> 16 & 0xFF), UInt8(value >> 8 & 0xFF), UInt8(value & 0xFF)]) }
        func checksum(_ data: Data) -> UInt32 {
            var sum: UInt32 = 0
            var index = 0
            let bytes = [UInt8](data)
            while index < bytes.count {
                var word: UInt32 = 0
                for offset in 0..<4 { word = word << 8 | UInt32(index + offset < bytes.count ? bytes[index + offset] : 0) }
                sum = sum &+ word
                index += 4
            }
            return sum
        }
        let count = tables.count
        var power = 1
        var log2 = 0
        while power * 2 <= count { power *= 2; log2 += 1 }
        u32(isCFF ? fourCC("OTTO") : 0x0001_0000)
        u16(count)
        u16(power * 16)
        u16(log2)
        u16(count * 16 - power * 16)
        let offset = 12 + 16 * count
        var headOffset: Int?
        var body = Data()
        for var table in tables {
            if table.tag == fourCC("head"), table.data.count >= 12 {
                // checkSumAdjustment is zero while checksums are computed.
                table.data.replaceSubrange(8..<12, with: [0, 0, 0, 0])
                headOffset = offset + body.count
            }
            u32(table.tag)
            u32(checksum(table.data))
            u32(UInt32(offset + body.count))
            u32(UInt32(table.data.count))
            body.append(table.data)
            while body.count % 4 != 0 { body.append(0) }
        }
        out.append(body)
        if let headOffset {
            let adjustment = 0xB1B0_AFBA &- checksum(out)
            out.replaceSubrange(
                (headOffset + 8)..<(headOffset + 12),
                with: [UInt8(adjustment >> 24), UInt8(adjustment >> 16 & 0xFF), UInt8(adjustment >> 8 & 0xFF), UInt8(adjustment & 0xFF)])
        }
        return out
    }

    // MARK: Video

    /// An H.264 MP4 of `seconds` seconds, `fps` frames a second, each second
    /// a different grey, with its index at the front of the file (fast
    /// start), as a card's `<video>` would be.
    @concurrent nonisolated static func mp4(seconds: Int = 20, fps: Int = 10, size: CGSize = CGSize(width: 320, height: 240)) async throws -> Data {
        let url = FileManager.default.temporaryDirectory.appending(path: "spike-\(UUID().uuidString).mp4")
        defer { try? FileManager.default.removeItem(at: url) }
        let writer = try AVAssetWriter(outputURL: url, fileType: .mp4)
        writer.shouldOptimizeForNetworkUse = true
        let input = AVAssetWriterInput(
            mediaType: .video,
            outputSettings: [AVVideoCodecKey: AVVideoCodecType.h264, AVVideoWidthKey: Int(size.width), AVVideoHeightKey: Int(size.height)])
        let attributes = CVPixelBufferCreationAttributes(
            pixelFormatType: CVPixelFormatType(rawValue: kCVPixelFormatType_32BGRA), size: CVImageSize(width: Int(size.width), height: Int(size.height)))
        let receiver = writer.inputPixelBufferReceiver(for: input, pixelBufferAttributes: attributes)
        try writer.start()
        writer.startSession(atSourceTime: .zero)
        for index in 0..<(seconds * fps) {
            let frame = try CVMutablePixelBuffer(attributes)
            let grey = Int32(truncatingIfNeeded: 40 + (index / fps) * 9)
            frame.withUnsafeBuffer { buffer in
                CVPixelBufferLockBaseAddress(buffer, [])
                memset(CVPixelBufferGetBaseAddress(buffer), grey, CVPixelBufferGetDataSize(buffer))
                CVPixelBufferUnlockBaseAddress(buffer, [])
            }
            // `append` sometimes never resumed (a hung run): poll instead.
            let ready = CVReadOnlyPixelBuffer(frame)
            let time = CMTime(value: CMTimeValue(index), timescale: CMTimeScale(fps))
            let deadline = ContinuousClock.now + .seconds(10)
            while try !receiver.appendImmediately(ready, with: time) {
                guard ContinuousClock.now < deadline else { throw Spike.Failure("the MP4 writer took no frame \(index) for 10 s") }
                try await Task.sleep(for: .milliseconds(2))
            }
        }
        receiver.finish()
        await writer.finishWriting()
        guard writer.status == .completed else { throw Spike.Failure("the MP4 did not finish: \(String(describing: writer.error))") }
        return try Data(contentsOf: url)
    }
}
