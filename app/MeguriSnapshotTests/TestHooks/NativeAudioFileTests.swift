#if os(macOS) && MEGURI_TEST_HOOKS
import AVFoundation
import Darwin
import Foundation
import Testing
@testable import CardRendering

@MainActor @Suite struct NativeAudioFileTests {
    @Test func retainedDescriptorPreparation() async {
        var fileURL = Observation.notRun
        var descriptorURL = Observation.notRun
        var descriptorWAVHint = Observation.notRun
        var backend = Observation.notRun
        var fileURLMs: Int?
        var descriptorURLMs: Int?
        var descriptorWAVHintMs: Int?
        var backendPrepareMs: Int?
        var clipStopMs: Int?
        var mediaCloseMs: Int?
        let preparationTimings = PreparationTimings()
        var setupError = "null"
        do {
            let manager = FileManager.default
            let directory = manager.temporaryDirectory.appending(path: "native-audio-\(UUID().uuidString)")
            try manager.createDirectory(at: directory, withIntermediateDirectories: false,
                                        attributes: [.posixPermissions: 0o700])
            defer { try? manager.removeItem(at: directory) }
            let url = directory.appending(path: "silence.wav")
            let wav = Self.silentWAV()
            try wav.write(to: url, options: .withoutOverwriting)
            try manager.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
            let descriptor = Darwin.open(url.path, O_RDONLY | O_NOFOLLOW | O_CLOEXEC)
            guard descriptor >= 0 else { throw Self.posixError() }
            defer { Darwin.close(descriptor) }

            let fileURLStart = ContinuousClock.now
            fileURL = Self.observe(url)
            fileURLMs = Self.elapsedMilliseconds(since: fileURLStart)
            let retainedURL = URL(fileURLWithPath: "/dev/fd/\(descriptor)")
            try Self.rewind(descriptor)
            let descriptorURLStart = ContinuousClock.now
            descriptorURL = Self.observe(retainedURL)
            descriptorURLMs = Self.elapsedMilliseconds(since: descriptorURLStart)
            try Self.rewind(descriptor)
            let descriptorWAVHintStart = ContinuousClock.now
            descriptorWAVHint = Self.observe(retainedURL, hint: AVFileType.wav.rawValue)
            descriptorWAVHintMs = Self.elapsedMilliseconds(since: descriptorWAVHintStart)

            try Self.rewind(descriptor)
            let duplicate = Darwin.dup(descriptor)
            guard duplicate >= 0 else { throw Self.posixError() }
            var opening = stat()
            guard fstat(duplicate, &opening) == 0 else {
                Darwin.close(duplicate)
                throw Self.posixError()
            }
            // Ownership transfers to MediaFile; close it even if preparation throws.
            let media = MediaFile(descriptor: duplicate, name: "silence.wav", opening: opening)
            let backendPrepareStart = ContinuousClock.now
            let playback = AVQueuePlayback()
            playback.onNativePreparationInterval = { phase, elapsedMs in
                preparationTimings.record(phase, elapsedMs: elapsedMs)
            }
            do {
                let clip = try await playback.prepare(media)
                backendPrepareMs = Self.elapsedMilliseconds(since: backendPrepareStart)
                backend = .success(duration: clip.duration)
                let clipStopStart = ContinuousClock.now
                await clip.stop()
                clipStopMs = Self.elapsedMilliseconds(since: clipStopStart)
            } catch {
                backendPrepareMs = Self.elapsedMilliseconds(since: backendPrepareStart)
                backend = .failure(error)
            }
            let mediaCloseStart = ContinuousClock.now
            await media.close()
            mediaCloseMs = Self.elapsedMilliseconds(since: mediaCloseStart)
        } catch {
            setupError = Self.errorJSON(error)
        }

        // Print before assertions: a production failure must retain all controls.
        print("{\"nativeAudioFile\":1,\"setupError\":\(setupError),\"fileURL\":\(fileURL.json),"
              + "\"descriptorURL\":\(descriptorURL.json),\"descriptorWAVHint\":\(descriptorWAVHint.json),"
              + "\"backend\":\(backend.json)}")
        print("{\"nativeAudioFileTiming\":1,"
              + "\"fileURL\":\(Self.timingJSON(fileURLMs, succeeded: fileURL.succeeded)),"
              + "\"descriptorURL\":\(Self.timingJSON(descriptorURLMs, succeeded: descriptorURL.succeeded)),"
              + "\"descriptorWAVHint\":\(Self.timingJSON(descriptorWAVHintMs, succeeded: descriptorWAVHint.succeeded)),"
              + "\"backendPrepare\":\(Self.timingJSON(backendPrepareMs, succeeded: backend.succeeded)),"
              + "\"clipStop\":\(Self.timingJSON(clipStopMs)),"
              + "\"mediaClose\":\(Self.timingJSON(mediaCloseMs))}")
        print(preparationTimings.json)
        #expect(fileURL.succeeded && fileURL.duration == 2)
        // Descriptor controls are diagnostic facts, separate from this verdict.
        #expect(backend.succeeded && backend.duration == 2)
    }

    private nonisolated final class PreparationTimings: @unchecked Sendable {
        private let lock = NSLock()
        private var beginNativePlayback: UInt64?
        private var playerConstructor: UInt64?
        private var prepareToPlay: UInt64?
        private var metadata: UInt64?
        private var requireNativePlaybackOpen: UInt64?

        func record(_ phase: AVQueuePlayback.NativePreparationPhase, elapsedMs: UInt64) {
            lock.lock(); defer { lock.unlock() }
            switch phase {
            case .beginNativePlayback: beginNativePlayback = elapsedMs
            case .playerConstructor: playerConstructor = elapsedMs
            case .prepareToPlay: prepareToPlay = elapsedMs
            case .metadata: metadata = elapsedMs
            case .requireNativePlaybackOpen: requireNativePlaybackOpen = elapsedMs
            }
        }

        var json: String {
            lock.lock(); defer { lock.unlock() }
            func number(_ value: UInt64?) -> String { value.map { String($0) } ?? "null" }
            return "{\"nativeAudioPreparationTiming\":1,"
                + "\"beginNativePlaybackMs\":\(number(beginNativePlayback)),"
                + "\"playerConstructorMs\":\(number(playerConstructor)),"
                + "\"prepareToPlayMs\":\(number(prepareToPlay)),"
                + "\"metadataMs\":\(number(metadata)),"
                + "\"requireNativePlaybackOpenMs\":\(number(requireNativePlaybackOpen))}"
        }
    }

    private static func elapsedMilliseconds(since start: ContinuousClock.Instant) -> Int {
        let elapsed = start.duration(to: .now)
        guard elapsed > .zero else { return 0 }
        let components = elapsed.components
        guard components.seconds < 600 else { return 600_000 }
        let milliseconds = components.seconds * 1_000 + components.attoseconds / 1_000_000_000_000_000
        return Int(max(0, min(600_000, milliseconds)))
    }

    private static func timingJSON(_ elapsed: Int?, succeeded: Bool = true) -> String {
        guard let elapsed else { return #"{"outcome":"notRun","elapsedMs":null}"# }
        let outcome = succeeded ? "success" : "failure"
        return "{\"outcome\":\"\(outcome)\",\"elapsedMs\":\(elapsed)}"
    }

    @MainActor private struct Observation {
        let succeeded: Bool
        let duration: Double?
        let json: String

        static let notRun = Observation(succeeded: false, duration: nil,
            json: #"{"outcome":"notRun","duration":null,"format":null,"error":null}"#)

        static func success(duration: Double?, format: String = "null") -> Self {
            Self(succeeded: true, duration: duration,
                 json: "{\"outcome\":\"success\",\"duration\":\(NativeAudioFileTests.number(duration)),"
                    + "\"format\":\(format),\"error\":null}")
        }

        static func failure(_ error: any Error) -> Self {
            Self(succeeded: false, duration: nil,
                 json: "{\"outcome\":\"failure\",\"duration\":null,\"format\":null,"
                    + "\"error\":\(NativeAudioFileTests.errorJSON(error))}")
        }
    }

    private static func observe(_ url: URL, hint: String? = nil) -> Observation {
        do {
            // Constructor-only controls: never prepare hardware or play.
            let player: AVAudioPlayer
            if let hint {
                player = try AVAudioPlayer(contentsOf: url, fileTypeHint: hint)
            } else {
                player = try AVAudioPlayer(contentsOf: url)
            }
            let format = player.format.streamDescription.pointee
            return .success(duration: player.duration,
                format: "{\"sampleRate\":\(number(format.mSampleRate)),\"channels\":\(format.mChannelsPerFrame),"
                    + "\"formatID\":\(format.mFormatID),\"bitsPerChannel\":\(format.mBitsPerChannel)}")
        } catch {
            return .failure(error)
        }
    }

    private enum ErrorCategory: String {
        case osStatus = "NSOSStatusErrorDomain"
        case avFoundation = "AVFoundationErrorDomain"
        case cocoa = "NSCocoaErrorDomain"
        case posix = "NSPOSIXErrorDomain"
        case other
    }

    private static func errorJSON(_ error: any Error) -> String {
        let error = error as NSError
        let category: ErrorCategory
        switch error.domain {
        case NSOSStatusErrorDomain: category = .osStatus
        case AVFoundationErrorDomain: category = .avFoundation
        case NSCocoaErrorDomain: category = .cocoa
        case NSPOSIXErrorDomain: category = .posix
        default: category = .other
        }
        return "{\"category\":\"\(category.rawValue)\",\"code\":\(error.code)}"
    }

    private static func number(_ value: Double?) -> String {
        guard let value, value.isFinite else { return "null" }
        return String(value)
    }

    private static func posixError() -> NSError {
        NSError(domain: NSPOSIXErrorDomain, code: Int(errno))
    }

    private static func rewind(_ descriptor: Int32) throws {
        guard Darwin.lseek(descriptor, 0, SEEK_SET) == 0 else { throw posixError() }
    }

    private static func silentWAV() -> Data {
        var data = Data()
        func append<T: FixedWidthInteger>(_ value: T) {
            withUnsafeBytes(of: value.littleEndian) { data.append(contentsOf: $0) }
        }
        data.append(contentsOf: "RIFF".utf8)
        append(UInt32(32_036))
        data.append(contentsOf: "WAVEfmt ".utf8)
        append(UInt32(16))
        append(UInt16(1)) // PCM
        append(UInt16(1)) // Mono
        append(UInt32(8_000))
        append(UInt32(16_000)) // Bytes per second
        append(UInt16(2)) // Block alignment
        append(UInt16(16))
        data.append(contentsOf: "data".utf8)
        append(UInt32(32_000))
        data.append(Data(count: 32_000))
        return data
    }
}
#endif
