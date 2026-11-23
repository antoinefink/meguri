import CoreBridge
import Darwin
import Foundation

/// Home's last finished read, kept on the device so a launch draws the deck list and
/// Today at once (native-interface-contract N27): the deck tree with its counts, Today's
/// figures and the previous session, exactly as the views showed them. It belongs to one
/// collection file (`CollectionFileIdentity`) and is never shown for another one. Written
/// after each read, in the app's caches, never synced and excluded from backups.
public nonisolated struct HomeSnapshot: Codable, Equatable, Sendable {
    public static let currentVersion = 1

    public nonisolated struct Deck: Codable, Equatable, Sendable {
        public var id: Int64
        public var name: String
        public var filtered: Bool
        public var studyCollapsed: Bool
        public var browserCollapsed: Bool
        public var new: UInt32
        public var learning: UInt32
        public var review: UInt32
        public var children: [Deck]
    }

    /// `stats/studied-today` T12's numbers.
    public nonisolated struct Studied: Codable, Equatable, Sendable {
        public var answers: UInt64
        public var cards: UInt64
        public var timeMilliseconds: Int64
        public var recentAnswers: UInt64
        public var recentTimeMilliseconds: Int64
    }

    /// Today's Previous Session line (N24.2), as it was drawn.
    public nonisolated struct Previous: Codable, Equatable, Sendable {
        public var deckNames: [String]
        public var walk: Bool
        public var cards: Int
        public var endedAt: Date
        public init(deckNames: [String], walk: Bool, cards: Int, endedAt: Date) {
            self.deckNames = deckNames; self.walk = walk; self.cards = cards; self.endedAt = endedAt
        }
    }

    public var version: Int
    public var collection: CollectionFileIdentity
    /// When the study day these counts belong to ends (the core's `next_day_at`, Unix
    /// seconds); nil when it was not known. Counts are shown only before it.
    public var dayEndsAt: Int64?
    public var decks: [Deck]
    public var studied: Studied?
    public var nextLearningReadyAt: Date?
    public var previous: Previous?
    /// The tab layout's list: the previous session's answers.
    public var previousAnswers: Int?

    public init(collection: CollectionFileIdentity, dayEndsAt: Int64?, decks: [CountedDeckNode],
                studied: StudiedToday?, nextLearningReadyAt: Date?, previous: Previous?, previousAnswers: Int?) {
        version = Self.currentVersion
        self.collection = collection; self.dayEndsAt = dayEndsAt
        self.decks = decks.map(Deck.init)
        self.studied = studied.map(Studied.init)
        self.nextLearningReadyAt = nextLearningReadyAt; self.previous = previous
        self.previousAnswers = previousAnswers
    }

    /// Whether the counts are this study day's: before the day they were read in ends.
    public func countsCurrent(now: Date) -> Bool {
        guard let dayEndsAt else { return false }
        return now.timeIntervalSince1970 < Double(dayEndsAt)
    }

    /// The tree as Home draws it. Counts from an earlier study day are held back: zero,
    /// which no row draws, and the views read `ReviewCollection.homeCountsHeld` for the
    /// places where zero would say something (Today, the deck page, spoken values).
    public func nodes(countsCurrent: Bool) -> [CountedDeckNode] {
        decks.map { $0.node(countsCurrent: countsCurrent) }
    }
}

nonisolated extension HomeSnapshot.Deck {
    init(_ node: CountedDeckNode) {
        self.init(id: node.id, name: node.name, filtered: node.filtered, studyCollapsed: node.studyCollapsed,
                  browserCollapsed: node.browserCollapsed, new: node.counts.new, learning: node.counts.learning,
                  review: node.counts.review, children: node.children.map(Self.init))
    }

    func node(countsCurrent: Bool) -> CountedDeckNode {
        CountedDeckNode(id: id, name: name, filtered: filtered, studyCollapsed: studyCollapsed,
            browserCollapsed: browserCollapsed,
            counts: countsCurrent ? ReviewCounts(new: new, learning: learning, review: review)
                : ReviewCounts(new: 0, learning: 0, review: 0),
            children: children.map { $0.node(countsCurrent: countsCurrent) })
    }
}

nonisolated extension HomeSnapshot.Studied {
    init(_ value: StudiedToday) {
        self.init(answers: value.answers, cards: value.cards, timeMilliseconds: value.timeMilliseconds,
                  recentAnswers: value.recentAnswers, recentTimeMilliseconds: value.recentTimeMilliseconds)
    }

    var value: StudiedToday {
        StudiedToday(answers: answers, cards: cards, timeMilliseconds: timeMilliseconds,
                     recentAnswers: recentAnswers, recentTimeMilliseconds: recentTimeMilliseconds)
    }
}

/// Which collection file a snapshot belongs to: its path, file number and creation time.
/// A replaced collection (an import, a download, a restored backup, a fixture) is a new
/// file with another number and creation time, so it never matches an older snapshot.
public nonisolated struct CollectionFileIdentity: Codable, Equatable, Sendable {
    public var path: String
    public var fileNumber: UInt64
    public var createdSeconds: Int64
    public var createdNanoseconds: Int64

    /// One `stat` of the file; nil when it does not exist or cannot be read.
    public static func of(_ file: URL) -> CollectionFileIdentity? {
        var info = Darwin.stat()
        let path = file.path(percentEncoded: false)
        guard Darwin.stat(path, &info) == 0, (info.st_mode & S_IFMT) == S_IFREG else { return nil }
        return CollectionFileIdentity(path: path, fileNumber: UInt64(info.st_ino),
            createdSeconds: Int64(info.st_birthtimespec.tv_sec), createdNanoseconds: Int64(info.st_birthtimespec.tv_nsec))
    }
}

/// Reads and writes the one snapshot file. Reads happen once, at launch, on the main actor
/// (a few kilobytes, before the first frame); writes go to a serial background queue,
/// newest wins, and each is atomic.
public nonisolated final class HomeSnapshotStore: Sendable {
    public let file: URL
    private let queue = DispatchQueue(label: "com.antoinefink.meguri.homeSnapshot", qos: .utility)

    public init(file: URL) { self.file = file }

    /// `<Caches>/Meguri/home-snapshot.json` in the app's container.
    public static var standardFile: URL {
        URL.cachesDirectory.appending(path: "Meguri", directoryHint: .isDirectory)
            .appending(path: "home-snapshot.json", directoryHint: .notDirectory)
    }

    /// The snapshot kept for `collection`, if it is that file's; any other is deleted.
    public func load(for collection: URL) -> HomeSnapshot? {
        guard let data = try? Data(contentsOf: file) else { return nil }
        guard let snapshot = try? JSONDecoder().decode(HomeSnapshot.self, from: data),
              snapshot.version == HomeSnapshot.currentVersion,
              let identity = CollectionFileIdentity.of(collection), snapshot.collection == identity else {
            remove()
            return nil
        }
        return snapshot
    }

    public func save(_ snapshot: HomeSnapshot) {
        let file = file
        queue.async {
            guard let data = try? JSONEncoder().encode(snapshot) else { return }
            let directory = file.deletingLastPathComponent()
            try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            guard (try? data.write(to: file, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])) != nil else { return }
            var url = file
            var values = URLResourceValues()
            values.isExcludedFromBackup = true
            try? url.setResourceValues(values)
        }
    }

    public func remove() {
        let file = file
        queue.async { try? FileManager.default.removeItem(at: file) }
    }

    /// Waits for the writes and removals queued so far (tests).
    public func flush() { queue.sync {} }
}
