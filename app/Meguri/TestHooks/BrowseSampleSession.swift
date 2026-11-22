#if MEGURI_TEST_HOOKS
@_spi(MeguriTestHooks) import AppModel
import CoreBridge
import Foundation
import Synchronization
#if os(macOS)
import AppKit
#endif

extension TestHooks {
    /// The Browse routes' search session (U270a): a made-up collection of
    /// `cards` cards (two per note) held in memory, every value computed
    /// from the card's number. It reads the sample backend's run text
    /// (decks and states) and answers at once; it writes nothing.
    nonisolated final class SampleSearchSession: BrowseSearching {
        let cards: Int
        private let fails: Bool
        let events: AsyncStream<BrowseSearchEvent>
        private let continuation: AsyncStream<BrowseSearchEvent>.Continuation
        private let now = Date()

        private struct Handle: Sendable {
            /// Card (or note) numbers in ascending order; nil for every one.
            let numbers: [Int32]?
            let count: Int
            let mode: BrowseMode
            let descending: Bool
            let text: String
        }

        private struct State: Sendable {
            var seq: UInt64 = 0
            var handles: [UInt64: Handle] = [:]
        }

        private let state = Mutex(State())

        init(cards: Int, fails: Bool) {
            self.cards = cards
            self.fails = fails
            (events, continuation) = AsyncStream.makeStream(of: BrowseSearchEvent.self)
        }

        // MARK: The sample collection

        private static let decks: [Int64] = [3, 3, 2, 4, 4, 5, 7, 9, 9, 6]
        private static let deckNames: [Int64: String] = [
            2: "Languages::Español", 3: "Languages::Español::Vocabulary", 4: "Languages::日本語", 5: "Music Theory",
            6: "Quick Review", 7: "Reading", 9: "Science::Chemistry",
        ]
        private static let words: [Int64: [String]] = [
            2: ["plimb", "vorran", "teskel", "mardo", "quisp"],
            3: ["zindro", "brelt", "olvany", "frenna", "tusket", "gamrel"],
            4: ["korrim", "wendle", "spaith", "drovel", "ulmish"],
            5: ["Harmel step", "Velt chord", "Orrin scale", "Trask interval"],
            6: ["Nemmit", "Sorvel"],
            7: ["Yorrel", "Pimbek"],
            9: ["Kelbin", "Sorvel", "Lumrit charge", "Dask weight", "Nemmit"],
        ]
        private static let notetypes = ["Basic", "Vocabulary", "Basic (and reversed card)"]
        private static let tags = ["brask", "orrin", "spindle", "", "quill"]

        private static func deck(_ card: Int) -> Int64 { decks[card % decks.count] }
        private static func suspended(_ card: Int) -> Bool { card % 23 == 5 }
        private static func buried(_ card: Int) -> Bool { !suspended(card) && card % 31 == 7 }
        private static func isNew(_ card: Int) -> Bool { card % 9 == 0 }
        private static func flag(_ card: Int) -> UInt8 { card % 11 == 3 ? UInt8((card / 11) % 7 + 1) : 0 }

        private func matches(_ card: Int, decks: Set<Int64>, states: [String]) -> Bool {
            if !decks.isEmpty, !decks.contains(Self.deck(card)) { return false }
            return states.allSatisfy { state in
                switch state {
                case "suspended": Self.suspended(card)
                case "buried": Self.buried(card)
                case "new": Self.isNew(card)
                case "review": !Self.isNew(card)
                default: false
                }
            }
        }

        private static func parse(_ text: String) -> (decks: Set<Int64>, states: [String]) {
            var decks = Set<Int64>(), states: [String] = []
            for part in text.split(separator: " ") {
                if part.hasPrefix("decks=") { decks = Set(part.dropFirst(6).split(separator: ",").compactMap { Int64($0) }) }
                if part.hasPrefix("states=") { states = part.dropFirst(7).split(separator: ",").map(String.init) }
            }
            return (decks, states)
        }

        // MARK: Requests

        func startSearch(_ text: String, mode: BrowseMode, sort: BrowseSort) async throws -> UInt64 {
            if fails { throw BridgeError.unavailable(reason: "sample") }
            return begin(text: text, mode: mode, descending: sort.direction == .descending)
        }

        func resort(_ handle: BrowseHandle, sort: BrowseSort) async throws -> UInt64 {
            guard let old = state.withLock({ $0.handles[handle.rawValue] }) else { throw BridgeError.browse(.unknownHandle) }
            return begin(text: old.text, mode: old.mode, descending: sort.direction == .descending)
        }

        func toggle(_ handle: BrowseHandle, sort: BrowseSort) async throws -> UInt64 {
            guard let old = state.withLock({ $0.handles[handle.rawValue] }) else { throw BridgeError.browse(.unknownHandle) }
            return begin(text: old.text, mode: old.mode == .cards ? .notes : .cards, descending: sort.direction == .descending)
        }

        private func begin(text: String, mode: BrowseMode, descending: Bool) -> UInt64 {
            let (decks, states) = Self.parse(text)
            var numbers: [Int32]?
            if !decks.isEmpty || !states.isEmpty {
                let found = (0..<cards).filter { matches($0, decks: decks, states: states) }
                numbers = mode == .cards ? found.map { Int32($0) } : Array(Set(found.map { Int32($0 / 2) })).sorted()
            }
            let count = numbers?.count ?? (mode == .cards ? cards : (cards + 1) / 2)
            let entry = Handle(numbers: numbers, count: count, mode: mode, descending: descending, text: text)
            let seq = state.withLock { state in
                state.seq += 1
                state.handles[state.seq] = entry
                return state.seq
            }
            let handle = BrowseHandle(rawValue: seq)
            let first = (0..<min(count, 200)).map { id(of: number(at: $0, in: entry), mode: mode) }
            continuation.yield(.firstPage(seq: seq, handle: handle, ids: first))
            continuation.yield(.complete(seq: seq, handle: handle, count: UInt64(count), revision: 0))
            return seq
        }

        private func number(at index: Int, in handle: Handle) -> Int {
            let position = handle.descending ? handle.count - 1 - index : index
            return handle.numbers.map { Int($0[position]) } ?? position
        }

        private func id(of number: Int, mode: BrowseMode) -> Int64 {
            (mode == .cards ? 1_700_000_100_000 : 1_700_000_000_000) + Int64(number)
        }

        // MARK: Rows

        func rows(_ handle: BrowseHandle, revision: UInt64, range: Range<UInt64>, columns: [String]) async throws
            -> [BrowseRowResult] {
            guard let entry = state.withLock({ $0.handles[handle.rawValue] }) else { throw BridgeError.browse(.unknownHandle) }
            return range.map { index in
                let number = self.number(at: Int(index), in: entry)
                return entry.mode == .cards ? card(number, columns: columns) : note(number, columns: columns)
            }
        }

        private func card(_ card: Int, columns: [String]) -> BrowseRowResult {
            let deck = Self.deck(card)
            let filtered = deck == 6
            let state = BrowseRowState(flag: Self.flag(card), marked: false, leech: false, suspended: Self.suspended(card),
                                       buried: Self.buried(card), filtered: filtered)
            let cells: [BrowseCell] = columns.map { key in
                switch key {
                case "noteFld": .text(sortField(card / 2 * 2))
                case "template": .text(card % 2 == 0 ? "Card 1" : "Card 2")
                case "deck": .text(filtered ? "Quick Review (Science::Chemistry)" : Self.deckNames[deck] ?? "Default")
                case "cardDue":
                    filtered ? .filtered(wrapped: false)
                        : Self.isNew(card) ? .newPosition(Int64(card / 9 + 1), wrapped: false)
                        : .instant(seconds: seconds(days: (card * 37) % 60 - 5), wrapped: false)
                case "cardMod": .dateTime(seconds: seconds(days: -((card * 13) % 400)))
                case "note": .text(Self.notetypes[(card / 2) % Self.notetypes.count])
                case "noteTags": .text(Self.tags[(card / 2) % Self.tags.count])
                case "noteCrt": .dateTime(seconds: seconds(days: -((card / 2 * 7) % 700)))
                default: .empty
                }
            }
            return .row(BrowseRow(id: id(of: card, mode: .cards), noteID: id(of: card / 2, mode: .notes), state: state,
                                  tint: .none, font: BrowseFont(name: nil, size: nil), sortFieldRightToLeft: false, cells: cells))
        }

        private func note(_ note: Int, columns: [String]) -> BrowseRowResult {
            let first = note * 2
            let cells: [BrowseCell] = columns.map { key in
                switch key {
                case "noteFld": .text(sortField(first))
                case "note": .text(Self.notetypes[note % Self.notetypes.count])
                case "template": .count(first + 1 < cards ? 2 : 1)
                case "noteTags": .text(Self.tags[note % Self.tags.count])
                case "noteCrt": .dateTime(seconds: seconds(days: -((note * 7) % 700)))
                case "deck": .text(Self.deckNames[Self.deck(first)] ?? "Default")
                default: .empty
                }
            }
            let state = BrowseRowState(flag: 0, marked: false, leech: false, suspended: false, buried: false, filtered: false)
            return .row(BrowseRow(id: id(of: note, mode: .notes), noteID: id(of: note, mode: .notes), state: state, tint: .none,
                                  font: BrowseFont(name: nil, size: nil), sortFieldRightToLeft: false, cells: cells))
        }

        private func sortField(_ card: Int) -> String {
            let words = Self.words[Self.deck(card)] ?? ["Sample"]
            let word = words[(card / Self.decks.count) % words.count]
            return card < words.count * Self.decks.count ? word : "\(word) \(card / (words.count * Self.decks.count) + 1)"
        }

        private func seconds(days: Int) -> Int64 {
            Int64(now.timeIntervalSince1970) + Int64(days) * 86_400
        }

        func qaCells(_ handle: BrowseHandle, revision: UInt64, range: Range<UInt64>, which: BrowseQAWhich) async throws
            -> [BrowseQAResult] {
            range.map { _ in .cells(question: nil, answer: nil) }
        }

        func invalidatedRows(_ handle: BrowseHandle, revision: UInt64, changes: [SequencedChangeSet]) async throws
            -> BrowseInvalidation {
            let count = state.withLock { $0.handles[handle.rawValue]?.count ?? 0 }
            return BrowseInvalidation(removed: [], refetch: .rows([]), revision: revision, isComplete: true, count: UInt64(count))
        }

        func mapSelection(from source: BrowseMapSource, selected: BrowseSelected, current: Int64?, to target: BrowseHandle,
                          targetRevision: UInt64) async throws -> BrowseMappedSelection {
            BrowseMappedSelection(selected: [], current: nil)
        }
    }

    /// Scrolls Browse's table for the performance tests (U270a), with no
    /// synthesized input: `<bundle id>.testhooks.browseScroll` jumps the
    /// table `rows` rows a frame for `seconds`, the way a fast scroll
    /// brings rows in, then answers `<bundle id>.testhooks.browseScrolled`.
    @MainActor final class BrowseScrollDriver: NSObject {
        static let shared = BrowseScrollDriver()
        /// The Browse whose table scrolls: a route's sample, or the real collection's.
        var model: BrowseModel?
        var collection: (() -> BrowseModel?)?
        private var listening = false
        private var running: Task<Void, Never>?

        static let rowsPerFrame = 40
        static let frames = 240

        func listen() {
            guard !listening else { return }
            listening = true
            #if os(macOS)
            DistributedNotificationCenter.default().addObserver(
                self, selector: #selector(received), name: Notification.Name(Self.name("browseScroll")), object: nil,
                suspensionBehavior: .deliverImmediately)
            #endif
        }

        static func name(_ suffix: String) -> String { "\(Bundle.main.bundleIdentifier ?? "").testhooks.\(suffix)" }

        @objc private func received() {
            guard running == nil, let model = model ?? collection?() else { return }
            running = Task { @MainActor in
                let results = model.results
                var index = 0
                for _ in 0..<Self.frames {
                    // The list as it is now: a streaming one grows while it scrolls.
                    index = (index + Self.rowsPerFrame) % max(results.list?.count ?? 0, 1)
                    results.scroll(to: index)
                    try? await Task.sleep(for: .milliseconds(16))
                }
                results.scroll(to: 0)
                running = nil
                #if os(macOS)
                DistributedNotificationCenter.default().postNotificationName(
                    Notification.Name(Self.name("browseScrolled")), object: nil, userInfo: nil, deliverImmediately: true)
                #endif
            }
        }
    }
}
#endif
