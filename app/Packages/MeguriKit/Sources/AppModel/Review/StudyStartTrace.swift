import Foundation
import Instrumentation

/// One Study's way from the click to its first card (R1813,
/// `review.startToFirstCard`), as uptime marks in the order the work runs.
/// The collection starts it, the session and its pages mark their parts,
/// and the first presentation's commit ends it. The same parts are
/// signposts for Instruments. Nothing here changes what runs.
public final class StudyStartTrace {
    public enum Mark: String, CaseIterable, Sendable {
        /// The deck selection was saved.
        case selected
        /// The media binding was ready.
        case media
        /// The session had its pages, warm or new.
        case pages
        /// The collection's study preparation (`sched/app-coordination` F5) returned.
        case prepared
        /// The core's start returned the first head.
        case core
        /// Both card pages had their documents.
        case loaded
        /// The first head's cards were rendered.
        case rendered
        /// The first card was ready in the idle page.
        case card
        /// The swap to the first card was made.
        case presented
        /// The review screen was asked for.
        case revealed
        /// The first card's page reported its presentation (`onCommitted`).
        case committed
    }

    /// Offsets from the click, in milliseconds.
    public struct Timings: Sendable, Equatable {
        public let deckID: Int64
        public let offsets: [Mark: Double]
        /// The pages were warm when the session took them.
        public let warmPages: Bool
        /// The first card was the one prepared before the click.
        public let preloaded: Bool
        /// One line for the test hooks' probe: each mark's offset, in the order above.
        public var summary: String {
            let marks = Mark.allCases.compactMap { mark in
                offsets[mark].map { "\(mark.rawValue)@\(Self.format($0))" }
            }
            return (["deck=\(deckID)", "warm=\(warmPages)", "preloaded=\(preloaded)"] + marks).joined(separator: " ")
        }
        private static func format(_ value: Double) -> String { String(format: "%.1f", value) }
    }

    public let deckID: Int64
    private let uptime: () -> TimeInterval
    private let began: TimeInterval
    private var marks: [Mark: TimeInterval] = [:]
    private(set) var warmPages = false
    private(set) var preloaded = false
    private var total: IntervalToken?
    private var pages: IntervalToken?
    private var card: IntervalToken?
    /// Called once, at the first presentation's commit.
    var finished: ((Timings) -> Void)?

    init(deckID: Int64, uptime: @escaping () -> TimeInterval = { ProcessInfo.processInfo.systemUptime }) {
        self.deckID = deckID
        self.uptime = uptime
        began = uptime()
        total = Signposts.begin(IntervalNames.reviewStartToFirstCard)
        pages = Signposts.begin(IntervalNames.reviewStartPagesReady)
    }

    /// The first time a mark is reached counts; later ones are ignored.
    func mark(_ mark: Mark) {
        guard marks[mark] == nil, total != nil else { return }
        marks[mark] = uptime()
        switch mark {
        case .loaded: if let pages { Signposts.end(pages); self.pages = nil }
        case .core: card = Signposts.begin(IntervalNames.reviewStartCardReady)
        case .card: if let card { Signposts.end(card); self.card = nil }
        case .committed: finish()
        default: break
        }
    }

    func pagesWereWarm() { warmPages = true }
    func cardWasPreloaded() { preloaded = true }

    var timings: Timings {
        Timings(deckID: deckID, offsets: marks.mapValues { ($0 - began) * 1_000 },
                warmPages: warmPages, preloaded: preloaded)
    }

    /// Ends the open intervals without a result: the session never showed a card.
    func cancel() {
        for token in [pages, card, total].compactMap({ $0 }) { Signposts.end(token) }
        pages = nil; card = nil; total = nil
        finished = nil
    }

    private func finish() {
        let timings = timings
        let done = finished
        cancel()
        done?(timings)
    }
}
