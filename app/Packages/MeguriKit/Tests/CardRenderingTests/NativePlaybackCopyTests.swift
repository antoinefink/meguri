@testable import CardRendering
import Darwin
import Foundation
import Testing

/// Real owned files only; no page, WebKit host or audible playback.
@Suite struct NativePlaybackCopyTests {
    @Test func exactBytesControlledSuffixReadOnlyAndClose() async throws {
        let fixture = try CopyFixture()
        let bytes = Data((0..<190_003).map { UInt8($0 % 251) })
        try bytes.write(to: fixture.source)
        let (resolver, file) = try await fixture.open()
        let url = try await file.playbackURL(in: fixture.area)
        #expect(url.isFileURL && url.pathExtension == "wav")
        #expect(url.lastPathComponent == "payload.wav")
        #expect(try Data(contentsOf: url) == bytes)
        let mode = try FileManager.default.attributesOfItem(atPath: url.path)[.posixPermissions] as? NSNumber
        #expect(mode?.intValue == 0o400)
        let directoryMode = try FileManager.default.attributesOfItem(atPath: url.deletingLastPathComponent().path)[.posixPermissions] as? NSNumber
        #expect(directoryMode?.intValue == 0o700)
        #expect(try await file.playbackURL(in: fixture.area) == url)
        #expect(try await file.read(offset: 17, count: 10) == bytes.subdata(in: 17..<27))
        await file.close()
        #expect(!FileManager.default.fileExists(atPath: url.deletingLastPathComponent().path))
        #expect(try Data(contentsOf: fixture.source) == bytes)
        await #expect(throws: (any Error).self) { try await file.playbackURL(in: fixture.area) }
        await #expect(throws: (any Error).self) { try await file.read(offset: 0, count: 1) }
        await file.close() // Idempotent; cannot recreate the copy.
        await resolver.close()
    }

    @Test func increasingFilesTransferWithOneBoundedChunk() async throws {
        for length in [1_048_579, 25_165_841] {
            let fixture = try CopyFixture()
            try fixture.writePattern(length: length)
            let (resolver, file) = try await fixture.open()
            let probe = CopyProbe()
            let url = try await file.playbackURL(in: fixture.area, hooks: .init(didRead: { probe.read($0) }))
            #expect(probe.maximumRead <= 65_536)
            #expect(probe.requestedBytes == length)
            #expect(probe.reads > 1)
            #expect(!probe.readOnMainThread)
            // Compare incrementally too: the test never allocates input-sized Data.
            try fixture.comparePattern(url, length: length)
            await file.close()
            await resolver.close()
        }
    }

    @Test func completesShortWritesAndRejectsIOFailureOrZeroProgress() async throws {
        for outcome in 0...2 {
            let fixture = try CopyFixture()
            let bytes = Data(repeating: 37, count: 131_079)
            try bytes.write(to: fixture.source)
            let (resolver, file) = try await fixture.open()
            let hooks = NativePlaybackCopy.Hooks(write: { fd, data, offset in
                if outcome == 1 { errno = EIO; return -1 }
                if outcome == 2 { return 0 }
                return data.withUnsafeBytes { Darwin.write(fd, $0.baseAddress!.advanced(by: offset), min(997, data.count - offset)) }
            })
            if outcome == 0 {
                let url = try await file.playbackURL(in: fixture.area, hooks: hooks)
                #expect(try Data(contentsOf: url) == bytes)
            } else {
                await #expect(throws: (any Error).self) { try await file.playbackURL(in: fixture.area, hooks: hooks) }
                #expect(try fixture.entries().isEmpty)
            }
            await file.close()
            await resolver.close()
            #expect(try fixture.entries().isEmpty)
        }
    }

    @Test func sharedAreaSiblingChangesPermitPreparationAndDisposal() async throws {
        let targets: [(NativePlaybackCopy.AreaObservation, Bool)] = [(.opening, false), (.validating, false), (.validating, true)]
        for (target, duringDisposal) in targets {
            for removeSibling in [false, true] {
                let fixture = try CopyFixture()
                try Data([1, 2, 3]).write(to: fixture.source)
                try FileManager.default.createDirectory(at: fixture.area, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
                let sibling = fixture.area.appending(path: "clip-" + UUID().uuidString.lowercased())
                if removeSibling { #expect(mkdir(sibling.path, 0o700) == 0) }
                let (resolver, file) = try await fixture.open()
                let probe = CopyProbe()
                let hooks = NativePlaybackCopy.Hooks(barrier: { stage, _ in
                    if stage == .disposing { probe.boundary(stage, survived: true) }
                }, didObserveArea: { phase, area in
                    guard phase == target, probe.stages.contains(.disposing) == duringDisposal, probe.first() else { return }
                    var before = stat()
                    #expect(lstat(area.path, &before) == 0)
                    if removeSibling { #expect(rmdir(sibling.path) == 0) }
                    else { #expect(mkdir(sibling.path, 0o700) == 0) }
                    var after = stat()
                    #expect(lstat(area.path, &after) == 0)
                    #expect(MediaFileIdentity(before) != MediaFileIdentity(after))
                })
                let url = try await file.playbackURL(in: fixture.area, hooks: hooks)
                #expect(try Data(contentsOf: url) == Data([1, 2, 3]))
                await file.close()
                #expect(probe.calls > 0)
                #expect(!FileManager.default.fileExists(atPath: url.deletingLastPathComponent().path))
                let expected: Set<String> = removeSibling ? [] : [sibling.lastPathComponent]
                #expect(Set(try fixture.entries()) == expected)
                #expect(try Data(contentsOf: fixture.source) == Data([1, 2, 3]))
                await resolver.close()
            }
        }
    }

    @Test func sharedAreaReplacementAndModeChangesRefusePreparationOrRemoval() async throws {
        let checkpoints: [(NativePlaybackCopy.AreaObservation, Bool)] = [(.opening, false), (.validating, false), (.validating, true)]
        for (checkpoint, onClose) in checkpoints {
            for exchangeDirectory in [false, true] {
                let sandbox = try CopyFixture()
                try Data([1, 2, 3]).write(to: sandbox.source)
                try FileManager.default.createDirectory(at: sandbox.area, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
                let incomingDirectory = sandbox.root.appending(path: "area-slot-b")
                let parkedDirectory = sandbox.root.appending(path: "area-slot-a")
                try FileManager.default.createDirectory(at: incomingDirectory, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
                try Data([17]).write(to: sandbox.area.appending(path: "sentinel.dat"))
                try Data([91]).write(to: incomingDirectory.appending(path: "sentinel.dat"))
                let (reader, media) = try await sandbox.open()
                let observer = CopyProbe()
                let interposition = NativePlaybackCopy.Hooks(barrier: { boundary, _ in
                    if boundary == .disposing { observer.boundary(boundary, survived: true) }
                }, didObserveArea: { moment, inspectedDirectory in
                    guard moment == checkpoint, observer.stages.contains(.disposing) == onClose, observer.first() else { return }
                    if exchangeDirectory {
                        #expect(rename(inspectedDirectory.path, parkedDirectory.path) == 0)
                        #expect(rename(incomingDirectory.path, inspectedDirectory.path) == 0)
                    } else {
                        #expect(chmod(inspectedDirectory.path, 0o755) == 0)
                    }
                })
                if onClose {
                    let preparedURL = try await media.playbackURL(in: sandbox.area, hooks: interposition)
                    await media.close()
                    let preservedURL = exchangeDirectory
                        ? parkedDirectory.appending(path: preparedURL.deletingLastPathComponent().lastPathComponent).appending(path: preparedURL.lastPathComponent)
                        : preparedURL
                    #expect(try Data(contentsOf: preservedURL) == Data([1, 2, 3]))
                } else {
                    await #expect(throws: NativePlaybackCopy.Failure.self) {
                        try await media.playbackURL(in: sandbox.area, hooks: interposition)
                    }
                    await media.close()
                }
                #expect(observer.calls > 0)
                let firstContainer = exchangeDirectory ? parkedDirectory : sandbox.area
                let secondContainer = exchangeDirectory ? sandbox.area : incomingDirectory
                #expect(try Data(contentsOf: firstContainer.appending(path: "sentinel.dat")) == Data([17]))
                #expect(try Data(contentsOf: secondContainer.appending(path: "sentinel.dat")) == Data([91]))
                #expect(try Data(contentsOf: sandbox.source) == Data([1, 2, 3]))
                await reader.close()
            }
        }
    }

    @Test func sharedAreaFirstObservationMustAlsoHavePrivateMode() async throws {
        for target in [NativePlaybackCopy.AreaObservation.opening, .validating] {
            let fixture = try CopyFixture()
            try Data([1, 2, 3]).write(to: fixture.source)
            try FileManager.default.createDirectory(at: fixture.area, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
            if target == .opening { #expect(chmod(fixture.area.path, 0o755) == 0) }
            let (resolver, file) = try await fixture.open()
            let probe = CopyProbe()
            let hooks = NativePlaybackCopy.Hooks(barrier: { stage, _ in
                if stage == .beforeLease, target == .validating { #expect(chmod(fixture.area.path, 0o755) == 0) }
            }, didObserveArea: { phase, area in
                guard phase == target, probe.first() else { return }
                #expect(chmod(area.path, 0o700) == 0)
            })
            await #expect(throws: NativePlaybackCopy.Failure.self) {
                try await file.playbackURL(in: fixture.area, hooks: hooks)
            }
            #expect(probe.calls > 0)
            #expect(try fixture.entries().isEmpty)
            #expect(try Data(contentsOf: fixture.source) == Data([1, 2, 3]))
            await file.close()
            await resolver.close()
        }
    }

    @Test func mutationTruncationAndEarlyEOFRefused() async throws {
        for truncate in [false, true] {
            let fixture = try CopyFixture()
            try fixture.writePattern(length: 196_613)
            let (resolver, file) = try await fixture.open()
            let once = CopyProbe()
            let hooks = NativePlaybackCopy.Hooks(barrier: { stage, _ in
                guard stage == .copying, once.first() else { return }
                let fd = Darwin.open(fixture.source.path, O_WRONLY | O_NOFOLLOW | O_CLOEXEC)
                guard fd >= 0 else { return }
                defer { Darwin.close(fd) }
                if truncate { _ = ftruncate(fd, 10) }
                else {
                    var byte: UInt8 = 231
                    _ = pwrite(fd, &byte, 1, 0)
                }
            })
            await #expect(throws: (any Error).self) { try await file.playbackURL(in: fixture.area, hooks: hooks) }
            #expect(once.calls > 0)
            #expect(try fixture.entries().isEmpty)
            await file.close()
            await resolver.close()
        }
        // Timestamp-only changes made after resolver open are also refused.
        let fixture = try CopyFixture()
        try Data([1, 2, 3]).write(to: fixture.source)
        let (resolver, file) = try await fixture.open()
        try FileManager.default.setAttributes([.modificationDate: Date(timeIntervalSince1970: 1)], ofItemAtPath: fixture.source.path)
        await #expect(throws: (any Error).self) { try await file.playbackURL(in: fixture.area) }
        await file.close()
        await resolver.close()
    }

    @Test func atomicReplacementNeverSelectsReplacementBytes() async throws {
        let fixture = try CopyFixture()
        let old = Data(repeating: 13, count: 70_001)
        try old.write(to: fixture.source)
        let (resolver, file) = try await fixture.open()
        try Data(repeating: 99, count: 70_001).write(to: fixture.source, options: .atomic)
        do {
            let url = try await file.playbackURL(in: fixture.area)
            #expect(try Data(contentsOf: url) == old)
        } catch {
            #expect(error is NativePlaybackCopy.Failure)
        }
        await file.close()
        await resolver.close()
        #expect(try fixture.entries().isEmpty)
    }

    @Test func cancellationAtEveryCreationBoundaryAndConcurrentClose() async throws {
        for target in [NativePlaybackCopy.Stage.beforeLease, .copying, .published] {
            let fixture = try CopyFixture()
            try fixture.writePattern(length: 131_073)
            let (resolver, file) = try await fixture.open()
            let gate = CopyGate()
            let hooks = NativePlaybackCopy.Hooks(barrier: { stage, _ in
                if stage == target { gate.pauseOnce() }
            })
            let task = Task { try await file.playbackURL(in: fixture.area, hooks: hooks) }
            defer { gate.release() }
            try await gate.waitUntilPaused()
            task.cancel()
            // This queues behind the synchronous copy, then closes exactly once.
            let close = Task { await file.close() }
            gate.release()
            await #expect(throws: CancellationError.self) { try await task.value }
            await close.value
            #expect(try fixture.entries().isEmpty)
            await #expect(throws: (any Error).self) { try await file.playbackURL(in: fixture.area) }
            await resolver.close()
        }
    }

    @Test func recoverySkipsEveryLiveBoundaryAndReclaimsOnlyCompletedAbandonment() async throws {
        let fixture = try CopyFixture()
        try fixture.writePattern(length: 131_073)
        let (resolver, file) = try await fixture.open()
        let probe = CopyProbe()
        let hooks = NativePlaybackCopy.Hooks(barrier: { stage, url in
            NativePlaybackCopy.recover(in: fixture.area)
            probe.boundary(stage, survived: FileManager.default.fileExists(atPath: url.deletingLastPathComponent().path))
        }, unlink: { _, _ in errno = EIO; return -1 })
        let url = try await file.playbackURL(in: fixture.area, hooks: hooks)
        NativePlaybackCopy.recover(in: fixture.area)
        #expect(FileManager.default.fileExists(atPath: url.path))
        await file.close() // Failed removal leaves an unused, fully published entry.
        #expect(probe.survivedAllBoundaries)
        #expect(probe.stages == Set([.beforeLease, .copying, .published, .disposing]))
        #expect(FileManager.default.fileExists(atPath: url.path))
        NativePlaybackCopy.recover(in: fixture.area)
        #expect(!FileManager.default.fileExists(atPath: url.deletingLastPathComponent().path))
        await resolver.close()
    }

    @Test func recoveryRefusesUnknownIncompleteLinkedAndMetadataChangedEntries() async throws {
        for variant in 0...6 {
            let fixture = try CopyFixture()
            try Data([4, 5, 6]).write(to: fixture.source)
            let (resolver, file) = try await fixture.open()
            let url = try await file.playbackURL(in: fixture.area, hooks: .init(unlink: { _, _ in errno = EIO; return -1 }))
            await file.close()
            let directory = url.deletingLastPathComponent()
            let marker = directory.appending(path: "complete")
            switch variant {
            case 0: try Data([7]).write(to: directory.appending(path: "unknown"))
            case 1: try FileManager.default.removeItem(at: marker)
            case 2:
                let link = fixture.root.appending(path: "linked-payload")
                try FileManager.default.linkItem(at: url, to: link)
            case 3:
                try FileManager.default.removeItem(at: marker)
                try FileManager.default.createSymbolicLink(at: marker, withDestinationURL: fixture.source)
            case 4: try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
            case 5:
                try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: directory.path)
                let lock = directory.appending(path: "lease")
                try FileManager.default.removeItem(at: lock)
                try Data().write(to: lock, options: .withoutOverwriting)
                try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: lock.path)
            default:
                try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
                let handle = try FileHandle(forWritingTo: url)
                try handle.write(contentsOf: Data([9, 9, 9]))
                try handle.close()
                try FileManager.default.setAttributes([.posixPermissions: 0o400], ofItemAtPath: url.path)
            }
            NativePlaybackCopy.recover(in: fixture.area)
            #expect(FileManager.default.fileExists(atPath: directory.path))
            #expect(try Data(contentsOf: fixture.source) == Data([4, 5, 6]))
            await resolver.close()
        }
    }

    @Test func sourceAndAreaSymlinksCannotSelectAnotherDirectory() async throws {
        let fixture = try CopyFixture()
        let peerDirectory = fixture.root.appending(path: "peer-directory")
        try FileManager.default.createDirectory(at: peerDirectory, withIntermediateDirectories: false)
        let peerSample = peerDirectory.appending(path: "sample.wav")
        try Data([42]).write(to: peerSample)
        try FileManager.default.createSymbolicLink(at: fixture.source, withDestinationURL: peerSample)
        let resolver = try await MediaPathResolver.open(root: fixture.root, trackedNames: [])
        await #expect(throws: (any Error).self) { try await resolver.openAudioName("user-name.WAV") }
        try FileManager.default.removeItem(at: fixture.source)
        try Data([3]).write(to: fixture.source)
        let file = try await resolver.openAudioName("user-name.WAV")
        try FileManager.default.createSymbolicLink(at: fixture.area, withDestinationURL: peerDirectory)
        await #expect(throws: (any Error).self) { try await file.playbackURL(in: fixture.area) }
        NativePlaybackCopy.recover(in: fixture.area)
        NativePlaybackCopy.recover(in: peerDirectory) // This basename is also ineligible.
        #expect(try Data(contentsOf: peerSample) == Data([42]))
        await file.close()
        await resolver.close()
    }

    @Test func recoveryRetainsUnpublishedDirectoriesSymlinksAndNonprivateModes() throws {
        let fixture = try CopyFixture()
        try FileManager.default.createDirectory(at: fixture.area, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        let peerDirectory = fixture.root.appending(path: "peer-directory")
        try FileManager.default.createDirectory(at: peerDirectory, withIntermediateDirectories: false)
        let peerSample = peerDirectory.appending(path: "sample")
        try Data([77]).write(to: peerSample)
        var retained: [URL] = []
        for state in 0...3 {
            let candidate = fixture.area.appending(path: "clip-" + UUID().uuidString.lowercased())
            retained.append(candidate)
            if state == 3 {
                try FileManager.default.createSymbolicLink(at: candidate, withDestinationURL: peerDirectory)
                continue
            }
            try FileManager.default.createDirectory(at: candidate, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
            if state == 0 { continue } // Empty directory before lease creation.
            try Data().write(to: candidate.appending(path: "lease"), options: .withoutOverwriting)
            try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: candidate.appending(path: "lease").path)
            if state == 2 { try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: candidate.path) }
        }
        NativePlaybackCopy.recover(in: fixture.area)
        for url in retained { #expect(FileManager.default.fileExists(atPath: url.path)) }
        #expect(try Data(contentsOf: peerSample) == Data([77]))
    }

    @Test func invalidLengthAndReadFaultNeverPublishAndOwnerDeinitDisposes() async throws {
        let fixture = try CopyFixture()
        #expect(throws: (any Error).self) {
            try NativePlaybackCopy.create(extension: "wav", length: UInt64.max, area: fixture.area,
                hooks: .init(), validate: {}, read: { _, _ in throw NativePlaybackCopy.Failure.copy })
        }
        #expect(throws: (any Error).self) {
            try NativePlaybackCopy.create(extension: "wav", length: 4, area: fixture.area,
                hooks: .init(), validate: {}, read: { _, _ in throw NativePlaybackCopy.Failure.copy })
        }
        #expect(throws: (any Error).self) {
            try NativePlaybackCopy.create(extension: "wav", length: 4, area: fixture.area,
                hooks: .init(), validate: {}, read: { _, _ in Data() })
        }
        #expect(try fixture.entries().isEmpty)
        try Data([1, 2]).write(to: fixture.source)
        let resolver = try await MediaPathResolver.open(root: fixture.root, trackedNames: [])
        var file: MediaFile? = try await resolver.openAudioName("user-name.WAV")
        let url = try await file!.playbackURL(in: fixture.area)
        file = nil
        // Isolated actor deinit is scheduled on its executor, not MainActor.
        let deadline = ContinuousClock.now + .seconds(10)
        while FileManager.default.fileExists(atPath: url.path), ContinuousClock.now < deadline { await Task.yield() }
        #expect(!FileManager.default.fileExists(atPath: url.deletingLastPathComponent().path))
        await resolver.close()
    }

    @Test func closeDuringNativeOwnershipDefersRemovalUntilNativeStopBarrier() async throws {
        let fixture = try CopyFixture()
        try Data([1, 2, 3]).write(to: fixture.source)
        let (resolver, file) = try await fixture.open()
        let url = try await file.playbackURL(in: fixture.area)
        #expect(try await file.beginNativePlayback() == url)
        await file.close()
        // Logical close forbids any more work while initialization/stop still
        // owns both physical resources. No decoder can acquire this file again.
        #expect(FileManager.default.fileExists(atPath: url.path))
        await #expect(throws: CancellationError.self) { try await file.requireNativePlaybackOpen() }
        await #expect(throws: (any Error).self) { try await file.beginNativePlayback() }
        await #expect(throws: (any Error).self) { try await file.read(offset: 0, count: 1) }
        NativePlaybackCopy.recover(in: fixture.area)
        #expect(FileManager.default.fileExists(atPath: url.path))
        await file.endNativePlayback() // Only called after native stop/revocation.
        #expect(!FileManager.default.fileExists(atPath: url.deletingLastPathComponent().path))
        await file.endNativePlayback()
        await resolver.close()
    }

    @Test func nativePreparationFailureAndCancellationReleaseBothResources() async throws {
        for cancel in [false, true] {
            let fixture = try CopyFixture()
            try Data("invalid WAV bytes".utf8).write(to: fixture.source)
            let (resolver, file) = try await fixture.open()
            let url = try await file.playbackURL(in: fixture.area)
            let backend = AVQueuePlayback()
            let task = Task<Void, Error> { @MainActor in
                let clip = try await backend.prepare(file)
                await clip.stop()
            }
            if cancel { task.cancel() }
            await #expect(throws: (any Error).self) { try await task.value }
            #expect(!FileManager.default.fileExists(atPath: url.path))
            await #expect(throws: (any Error).self) { try await file.read(offset: 0, count: 1) }
            await resolver.close()
        }
    }

    @Test func nativePreparedResourcesSurvivePauseSeekUntilStop() async throws {
        let fixture = try CopyFixture()
        try CopyFixture.silentWAV.write(to: fixture.source)
        let (resolver, file) = try await fixture.open()
        let url = try await file.playbackURL(in: fixture.area)
        let clip = try await AVQueuePlayback().prepare(file)
        #expect(clip.duration == 2)
        await clip.pause()
        try await clip.seek(to: 0.5)
        #expect(FileManager.default.fileExists(atPath: url.path))
        #expect(try await file.read(offset: 0, count: 4) == Data("RIFF".utf8))
        await clip.stop()
        #expect(!FileManager.default.fileExists(atPath: url.path))
        await #expect(throws: (any Error).self) { try await file.playbackURL(in: fixture.area) }
        await #expect(throws: (any Error).self) { try await clip.play() }
        await clip.stop()
        await resolver.close()
    }

    @Test func closeOrCancelAfterNativeInitializationCannotReturnAClip() async throws {
        for cancel in [false, true] {
            let fixture = try CopyFixture()
            try CopyFixture.silentWAV.write(to: fixture.source)
            let (resolver, file) = try await fixture.open()
            let url = try await file.playbackURL(in: fixture.area)
            let gate = CopyGate()
            defer { gate.release() }
            let backend = AVQueuePlayback()
            backend.beforeNativeTransfer = { gate.pauseOnce() }
            let preparing = Task<Void, Error> { @MainActor in
                let clip = try await backend.prepare(file)
                await clip.stop()
            }
            try await gate.waitUntilPaused()
            if cancel { preparing.cancel() }
            else { await file.close() }
            #expect(FileManager.default.fileExists(atPath: url.path))
            NativePlaybackCopy.recover(in: fixture.area)
            #expect(FileManager.default.fileExists(atPath: url.path))
            gate.release()
            await #expect(throws: CancellationError.self) { try await preparing.value }
            #expect(!FileManager.default.fileExists(atPath: url.deletingLastPathComponent().path))
            await #expect(throws: (any Error).self) { try await file.read(offset: 0, count: 1) }
            await resolver.close()
        }
    }
}

private nonisolated final class CopyFixture: Sendable {
    let root: URL
    let area: URL
    let source: URL
    init() throws {
        root = FileManager.default.temporaryDirectory.appending(path: "copy-test-" + UUID().uuidString)
        area = root.appending(path: NativePlaybackCopy.namespace)
        source = root.appending(path: "user-name.WAV")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
    }
    deinit { try? FileManager.default.removeItem(at: root) }
    func open() async throws -> (MediaPathResolver, MediaFile) {
        let resolver = try await MediaPathResolver.open(root: root, trackedNames: [])
        return (resolver, try await resolver.openAudioName("user-name.WAV"))
    }
    func entries() throws -> [String] {
        guard FileManager.default.fileExists(atPath: area.path) else { return [] }
        return try FileManager.default.contentsOfDirectory(atPath: area.path)
    }
    func writePattern(length: Int) throws {
        let handle = try FileHandle(forWritingTo: createSource())
        defer { try? handle.close() }
        let block = Data((0..<65_536).map { UInt8($0 % 251) })
        var remaining = length
        while remaining > 0 {
            try handle.write(contentsOf: block.prefix(min(remaining, block.count)))
            remaining -= min(remaining, block.count)
        }
    }
    private func createSource() throws -> URL { try Data().write(to: source, options: .withoutOverwriting); return source }
    func comparePattern(_ url: URL, length: Int) throws {
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        let block = Data((0..<65_536).map { UInt8($0 % 251) })
        var remaining = length
        while remaining > 0 {
            let count = min(remaining, block.count)
            #expect(try handle.read(upToCount: count) == block.prefix(count))
            remaining -= count
        }
        #expect(try handle.read(upToCount: 1)?.isEmpty != false)
    }
    static var silentWAV: Data {
        var data = Data()
        func append<T: FixedWidthInteger>(_ value: T) { withUnsafeBytes(of: value.littleEndian) { data.append(contentsOf: $0) } }
        data.append(contentsOf: "RIFF".utf8); append(UInt32(32_036))
        data.append(contentsOf: "WAVEfmt ".utf8); append(UInt32(16)); append(UInt16(1)); append(UInt16(1))
        append(UInt32(8_000)); append(UInt32(16_000)); append(UInt16(2)); append(UInt16(16))
        data.append(contentsOf: "data".utf8); append(UInt32(32_000)); data.append(Data(count: 32_000))
        return data
    }
}

private nonisolated final class CopyProbe: @unchecked Sendable {
    private let lock = NSLock()
    private var maxSize = 0
    private var totalSize = 0
    private var readCount = 0
    private var mainThreadRead = false
    private var count = 0
    private var boundaries: Set<NativePlaybackCopy.Stage> = []
    private var survived = true
    func read(_ size: Int) {
        lock.lock(); defer { lock.unlock() }
        maxSize = max(maxSize, size); totalSize += size; readCount += 1
        mainThreadRead = mainThreadRead || Thread.isMainThread
    }
    var maximumRead: Int { lock.lock(); defer { lock.unlock() }; return maxSize }
    var requestedBytes: Int { lock.lock(); defer { lock.unlock() }; return totalSize }
    var reads: Int { lock.lock(); defer { lock.unlock() }; return readCount }
    var readOnMainThread: Bool { lock.lock(); defer { lock.unlock() }; return mainThreadRead }
    func first() -> Bool { lock.lock(); defer { lock.unlock() }; count += 1; return count == 1 }
    var calls: Int { lock.lock(); defer { lock.unlock() }; return count }
    func boundary(_ stage: NativePlaybackCopy.Stage, survived: Bool) {
        lock.lock(); defer { lock.unlock() }; boundaries.insert(stage); self.survived = self.survived && survived
    }
    var stages: Set<NativePlaybackCopy.Stage> { lock.lock(); defer { lock.unlock() }; return boundaries }
    var survivedAllBoundaries: Bool { lock.lock(); defer { lock.unlock() }; return survived }
}

private nonisolated final class CopyGate: @unchecked Sendable {
    private let condition = NSCondition()
    private var paused = false
    private var released = false
    func pauseOnce() {
        condition.lock(); defer { condition.unlock() }
        guard !paused else { return }
        paused = true
        while !released { condition.wait() }
    }
    enum Failure: Error { case barrierNotReached }
    func waitUntilPaused() async throws {
        // Async polling never blocks MainActor, which must cancel the copy task.
        let deadline = ContinuousClock.now + .seconds(10)
        while !isPaused {
            guard ContinuousClock.now < deadline else { throw Failure.barrierNotReached }
            await Task.yield()
        }
    }
    private var isPaused: Bool { condition.lock(); defer { condition.unlock() }; return paused }
    func release() { condition.lock(); defer { condition.unlock() }; released = true; condition.broadcast() }
}
