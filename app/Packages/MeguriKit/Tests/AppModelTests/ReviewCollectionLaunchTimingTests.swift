@testable import CoreBridge
import Foundation
import Testing
@testable import AppModel

/// How long a real open takes to Home's first deck tree, step by step (N27.5). Owns the
/// process's real collection: run it with a focused selector. Set
/// `MEGURI_LAUNCH_TIMING_FIXTURE` to a generated collection (`tools/mg fixtures 10k`) to time
/// a larger one; the default is the package's small schema-18 fixture.
extension ReviewCollectionStartupTests {
    @Test(.timeLimit(.minutes(5))) func launchTimingFirstDeckTreeIsReadBeforeTheRestOfTheHostCompletes() async throws {
        let source: URL
        if let path = ProcessInfo.processInfo.environment["MEGURI_LAUNCH_TIMING_FIXTURE"] {
            source = URL(filePath: path)
        } else {
            source = try #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2"))
        }
        var runs: [[String: Double]] = []
        for _ in 0..<5 {
            let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
            defer { try? FileManager.default.removeItem(at: directory) }
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            try FileManager.default.copyItem(at: source, to: directory.appending(path: "collection.anki2"))
            var configuration = LaunchConfiguration()
            configuration.collectionLocation = AppModel.CollectionLocation(directory: directory)
            configuration.timeZone = TimeZone(secondsFromGMT: 0)
            let sync = SyncCoordinator(directory: directory, configuration: nil, defaults: nil)
            let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync,
                providerDirectory: directory.appending(path: "synthetic-installation", directoryHint: .isDirectory))
            let clock = ContinuousClock()
            let started = clock.now
            let trace = LaunchTimingTrace()
            let elapsed: @MainActor () -> Double = {
                let duration = clock.now - started
                return Double(duration.components.seconds) * 1000 + Double(duration.components.attoseconds) / 1e15
            }
            owner.startupStep = { step in trace.marks[step] = elapsed(); trace.order.append(step) }
            await owner.start()
            trace.marks["started"] = elapsed()
            let marks = trace.marks, order = trace.order
            try #require(owner.state == .ready)
            try #require(owner.deckTreeGeneration == owner.generation)
            try #require(!owner.decks.isEmpty)
            // The tree is read before sync attaches, the media index and study preparation.
            let tree = try #require(order.firstIndex(of: "treeRead"))
            for later in ["syncAttached", "mediaRead", "studyPrepared"] {
                if let index = order.firstIndex(of: later) { #expect(tree < index, "\(later) ran before the tree") }
            }
            runs.append(marks)
            #expect(await owner.close())
        }
        let steps = ["platformPrepared", "coreOpened", "ready", "recordLoaded", "dayChecked", "treeRead",
                     "syncAttached", "mediaRead", "studyPrepared", "started"]
        var lines: [String] = []
        for step in steps {
            let values = runs.compactMap { $0[step] }.sorted()
            guard !values.isEmpty else { continue }
            lines.append("\(step): median \(String(format: "%.1f", values[values.count / 2])) ms, max \(String(format: "%.1f", values.last!)) ms")
        }
        print("LAUNCH-TIMING " + lines.joined(separator: "; "))
    }

    private final class LaunchTimingTrace {
        var marks: [String: Double] = [:]
        var order: [String] = []
    }
}
