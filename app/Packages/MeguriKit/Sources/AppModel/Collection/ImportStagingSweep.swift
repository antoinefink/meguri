import Foundation
import Synchronization

/// `package/file-intake` K3 step 6 at launch: the copies a past run left in
/// the app's own import staging are removed. Two places only: the drop
/// copies under `tmp/Drops` (each in a folder of its own) and the system's
/// `Documents/Inbox` (Open in from Mail or the share sheet). An Inbox item
/// newer than `recent` before launch is kept: it may be the file this launch
/// was opened for, delivered after the sweep runs. At most `limit` items go
/// per run, so a full folder never slows the start.
nonisolated public enum ImportStagingSweep {
    public static func run(drops: URL, inbox: URL?, launchedAt: Date, recent: TimeInterval = 600,
                           limit: Int = 200, files: FileManager = .default) -> Int {
        var removed = 0
        func items(_ folder: URL) -> [URL] {
            (try? files.contentsOfDirectory(at: folder, includingPropertiesForKeys: [.contentModificationDateKey],
                                            options: [.skipsHiddenFiles])) ?? []
        }
        // Drop copies are this run's only: every folder left is a past run's.
        for item in items(drops) where removed < limit {
            if (try? files.removeItem(at: item)) != nil { removed += 1 }
        }
        guard let inbox else { return removed }
        for item in items(inbox) where removed < limit {
            let modified = (try? item.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate
            guard let modified, modified < launchedAt.addingTimeInterval(-recent) else { continue }
            if (try? files.removeItem(at: item)) != nil { removed += 1 }
        }
        return removed
    }

    /// Once per process, off the main thread: the launch's sweep.
    public static func sweepAtLaunch() {
        guard !swept.withLock({ let was = $0; $0 = true; return was }) else { return }
        let launchedAt = Date()
        Task.detached(priority: .utility) { _ = run(drops: dropsFolder, inbox: inboxFolder, launchedAt: launchedAt) }
    }
    private static let swept = Mutex(false)

    /// The app's two staging places on this device.
    public static var dropsFolder: URL { FileManager.default.temporaryDirectory.appending(path: "Drops", directoryHint: .isDirectory) }
    public static var inboxFolder: URL? {
        #if os(iOS)
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first?
            .appending(path: "Inbox", directoryHint: .isDirectory)
        #else
        nil
        #endif
    }
}
