import Foundation
import Testing
@testable import CardRendering

/// A `MediaFile` is released by whichever task used it last, often on another
/// thread just after that task left the file's actor, at any priority. The
/// release must never destroy the actor while a job is still on it (the
/// device crash of 2026-11-22: `swift_defaultActor_destroy` aborting from the
/// file's isolated deinit, run as a job on a cooperative-pool thread).
@Suite(.timeLimit(.minutes(3))) struct MediaFileLifetimeTests {
    private static let rounds = 400
    private static let width = 24
    nonisolated private static let priorities: [TaskPriority] = [.background, .utility, .medium, .userInitiated, .high]

    private static func fixture() throws -> (root: URL, names: [String]) {
        let root = FileManager.default.temporaryDirectory.appending(path: "meguri-media-life-\(UUID())", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        var names: [String] = []
        for index in 0..<8 {
            let name = "clip-\(index).wav"
            try Data(repeating: UInt8(index), count: 4_096).write(to: root.appending(path: name))
            names.append(name)
        }
        return (root, names)
    }

    /// Read, close and drop, and read while another task closes, with tasks of
    /// every priority meeting on one file; the last reference goes wherever
    /// the scheduler puts it.
    @Test func filesReleasedAcrossThreadsAndPrioritiesNeverAbort() async throws {
        let (root, names) = try Self.fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        let resolver = try await MediaPathResolver.open(root: root, trackedNames: [])
        for round in 0..<Self.rounds {
            await withTaskGroup(of: Void.self) { group in
                for lane in 0..<Self.width {
                    let name = names[(round + lane) % names.count]
                    let priority = Self.priorities[(round + lane) % Self.priorities.count]
                    group.addTask(priority: priority) {
                        await Self.exercise(resolver, name: name, lane: lane)
                    }
                }
            }
        }
        await resolver.close()
    }

    @concurrent private static func exercise(_ resolver: MediaPathResolver, name: String, lane: Int) async {
        guard let file = try? await resolver.openAudioName(name) else { return }
        switch lane % 4 {
        case 0:
            _ = try? await file.read(offset: 0, count: 512)
            await file.close()
        case 1:
            // Readers at other priorities hold their own references while the
            // opener closes and lets go.
            let reader = Task(priority: priorities[lane % priorities.count]) { [file] in
                for offset in stride(from: UInt64(0), to: 4_096, by: 512) {
                    guard (try? await file.read(offset: offset, count: 512)) != nil else { return }
                }
            }
            await file.close()
            await reader.value
        case 2:
            // The decoder's private copy is released with the file.
            _ = try? await file.playbackURL()
            await file.close()
        default:
            // Native playback ends through its owner, which then goes away.
            if (try? await file.beginNativePlayback()) != nil {
                let owner = Owner(file: file)
                await owner.stop()
            } else {
                await file.close()
            }
        }
    }

    private actor Owner {
        private let file: MediaFile
        init(file: MediaFile) { self.file = file }
        func stop() async { await file.endNativePlayback() }
    }

    /// The last reference can also go on a thread that runs no task: a
    /// dispatch queue, WebKit's or AVFoundation's callbacks, the main run loop.
    @Test(arguments: [false, true])
    func fileReleasedOutsideAnyTaskNeverAborts(closed: Bool) async throws {
        let (root, names) = try Self.fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        let resolver = try await MediaPathResolver.open(root: root, trackedNames: [])
        for round in 0..<200 {
            var file: MediaFile? = try await resolver.openAudioName(names[round % names.count])
            _ = try await file?.read(offset: 0, count: 512)
            if round % 2 == 0 { _ = try await file?.playbackURL() }
            if closed { await file?.close() }
            let last = Holder(file!)
            file = nil
            await withCheckedContinuation { (done: CheckedContinuation<Void, Never>) in
                DispatchQueue.global(qos: round % 3 == 0 ? .background : .userInitiated).async {
                    last.release()
                    done.resume()
                }
            }
        }
        await resolver.close()
    }

    /// The last release runs the deinit there and then, on the releasing
    /// thread (here a dispatch queue): nothing is left to run on the file's
    /// actor afterwards, and every file opened is closed by the time its last
    /// reference is gone.
    @Test func lastReleaseDestroysTheFileOnTheReleasingThread() async throws {
        let (root, names) = try Self.fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        let resolver = try await MediaPathResolver.open(root: root, trackedNames: [])
        for round in 0..<50 {
            var file: MediaFile? = try await resolver.openAudioName(names[round % names.count])
            _ = try await file?.read(offset: 0, count: 512)
            let id = ObjectIdentifier(file!)
            MediaFile.watchDeinit(file!)
            let last = Holder(file!)
            file = nil
            let ranBeforeReturn = await withCheckedContinuation { (done: CheckedContinuation<Bool, Never>) in
                DispatchQueue.global().async {
                    last.release()
                    done.resume(returning: MediaFile.deinitRan(id))
                }
            }
            #expect(ranBeforeReturn, "round \(round): the deinit waited for a job on the file's actor")
            while !MediaFile.deinitRan(id) && !ranBeforeReturn { await Task.yield() }
        }
        await resolver.close()
    }

    private nonisolated final class Holder: @unchecked Sendable {
        private var file: MediaFile?
        init(_ file: MediaFile) { self.file = file }
        func release() { file = nil }
    }
}
