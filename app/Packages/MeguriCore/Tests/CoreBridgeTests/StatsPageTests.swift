import Foundation
import Testing
@testable import CoreBridge
import MeguriCoreBindings

// The generated bindings use the same names: these are CoreBridge's.
private typealias StatsPart = CoreBridge.StatsPart
private typealias StatsPageMessage = CoreBridge.StatsPageMessage
private typealias StatsPageHeader = CoreBridge.StatsPageHeader
private typealias StatsUnreadable = CoreBridge.StatsUnreadable
private typealias StatsPageEnd = CoreBridge.StatsPageEnd
private typealias StatsPartOutcome = CoreBridge.StatsPartOutcome
private typealias StatsPartFailure = CoreBridge.StatsPartFailure
private typealias StatsProgress = CoreBridge.StatsProgress
private typealias StatsContext = CoreBridge.StatsContext
private typealias StatsPreferences = CoreBridge.StatsPreferences
private typealias StatsBrowseScope = CoreBridge.StatsBrowseScope
private typealias StatsOldestDay = CoreBridge.StatsOldestDay
private typealias StatsScopeError = CoreBridge.StatsScopeError
private typealias StatsHistory = CoreBridge.StatsHistory
private typealias StatsPageRequest = CoreBridge.StatsPageRequest
private typealias StatsDayCards = CoreBridge.StatsDayCards
private typealias StatsDayKind = CoreBridge.StatsDayKind

// The Statistics page's request through CoreBridge (U103, U292): G6's
// decoding of every part's value, the router's header, failure, progress
// and end rules with the cancel rule, and one request on a copy of the
// schema-18 fixture. Names and values are made up.

/// G6's writer, as the core's `Encoder` writes: the decoder's tests build their bytes with it.
private struct G6Writer {
    var bytes: [UInt8] = []

    mutating func unsigned(_ value: UInt64) {
        var value = value
        repeat {
            let byte = UInt8(value & 0x7f)
            value >>= 7
            bytes.append(value == 0 ? byte : byte | 0x80)
        } while value != 0
    }

    mutating func integer(_ value: Int) {
        let value = Int64(value)
        unsigned(UInt64(bitPattern: (value << 1) ^ (value >> 63)))
    }

    mutating func float32(_ value: Float) {
        withUnsafeBytes(of: value.bitPattern.littleEndian) { bytes.append(contentsOf: $0) }
    }

    mutating func float64(_ value: Double) {
        withUnsafeBytes(of: value.bitPattern.littleEndian) { bytes.append(contentsOf: $0) }
    }

    mutating func text(_ value: String) {
        unsigned(UInt64(value.utf8.count))
        bytes.append(contentsOf: value.utf8)
    }

    mutating func list<T>(_ items: [T], _ item: (inout G6Writer, T) -> Void) {
        integer(items.count)
        for value in items { item(&self, value) }
    }

    /// A sparse map: `keys` ascending, one row of member values per key.
    mutating func map(_ keys: [Int], _ rows: [[Int]]) {
        var runs: [Range<Int>] = []
        for (index, key) in keys.enumerated() {
            if let last = runs.last, keys[last.upperBound - 1] + 1 == key {
                runs[runs.count - 1] = last.lowerBound..<(index + 1)
            } else {
                runs.append(index..<(index + 1))
            }
        }
        integer(runs.count)
        var previous: Int?
        let members = rows.first?.count ?? 0
        for run in runs {
            integer(previous.map { keys[run.lowerBound] - $0 } ?? keys[run.lowerBound])
            integer(run.count)
            for member in 0..<members {
                for at in run { integer(rows[at][member]) }
            }
            previous = keys[run.upperBound - 1]
        }
    }

    var data: Data { Data(bytes) }
}

private func decoded(_ part: StatsPart, _ write: (inout G6Writer) -> Void) throws -> StatsPartValue {
    var writer = G6Writer()
    write(&writer)
    return try StatsPartValue.decode(part, writer.data)
}

private func header(refresh: UInt64 = 4) -> MeguriCoreBindings.StatsPageHeader {
    .init(refresh: refresh, history: .allHistory,
          context: .init(nextDayAt: 1_894_773_600, today: 1_400, rolloverHour: 4, nowMs: 1_894_701_600_000,
                         utcOffsetSecs: 3_600, zone: "Europe/Paris", generation: 2, seq: 31),
          preferences: .init(firstDayOfWeek: 1, cardCountsSeparateInactive: true, futureDueShowBacklog: false, fsrs: true),
          browseScope: .text(text: "deck:Lang"), oldestDay: .day(day: -900))
}

/// The router's output for `messages`, with `cancelAfter` messages let through before the cancel.
private func routed(_ messages: [MeguriCoreBindings.StatsPageMessage], cancelAfter: Int? = nil) async -> [StatsPageMessage] {
    await routed(events: messages.map { .core($0) }, cancelAfter: cancelAfter)
}

private func routed(events: [StatsPageEvent], cancelAfter: Int? = nil) async -> [StatsPageMessage] {
    let (raw, input) = AsyncStream.makeStream(of: StatsPageEvent.self)
    let (output, continuation) = AsyncStream.makeStream(of: StatsPageMessage.self)
    let state = StatsPageCancel()
    let router = Task { await StatsPageStream.route(raw, state: state, to: continuation) }
    var received: [StatsPageMessage] = []
    var iterator = output.makeAsyncIterator()
    for (index, event) in events.enumerated() {
        if index == cancelAfter { state.cancelled.withLock { $0 = true } }
        input.yield(event)
        // Let the router take each message before the next, so the cancel falls between two.
        if case .core(.header) = event { continue }
        if cancelAfter == nil || index < cancelAfter! {
            if let next = await iterator.next() { received.append(next) }
        }
    }
    input.finish()
    await router.value
    while let next = await iterator.next() { received.append(next) }
    return received
}

@Suite struct StatsDecodingTests {
    @Test func integersAndMapsReadAsTheCoreWritesThem() throws {
        var writer = G6Writer()
        for value in [0, 1, -1, 63, -64, 64, 8_191, -8_192, 1 << 40, Int.max, Int.min] { writer.integer(value) }
        writer.map([-366, -365, -364, -10, 0, 1, 5], [[1, 10], [2, 20], [3, 30], [4, 40], [5, 50], [6, 60], [7, 70]])
        writer.float32(-0.5)
        writer.text("prop:rated=-7304")
        var bytes = StatsBytes(writer.data)
        for value in [0, 1, -1, 63, -64, 64, 8_191, -8_192, 1 << 40, Int.max, Int.min] {
            #expect(try bytes.integer() == value)
        }
        let map = try bytes.map(members: 2) { $0 }
        #expect(map.keys == [-366, -365, -364, -10, 0, 1, 5])
        #expect(map.values.map { $0[1] } == [10, 20, 30, 40, 50, 60, 70])
        #expect(map[-10] == [4, 40] && map[-11] == nil)
        #expect(try bytes.float32() == -0.5)
        #expect(try bytes.text() == "prop:rated=-7304")
        try bytes.finish()

        // Truncated bytes, a varint over 64 bits and a trailing byte never decode.
        var truncated = StatsBytes([0x80])
        #expect(throws: StatsBytes.Failure.self) { try truncated.unsigned() }
        var over = StatsBytes([UInt8](repeating: 0x80, count: 9) + [0x02])
        #expect(throws: StatsBytes.Failure.self) { try over.unsigned() }
        var max = StatsBytes([UInt8](repeating: 0xff, count: 9) + [0x01])
        #expect(try max.unsigned() == .max)
        #expect(throws: StatsBytes.Failure.self) { try StatsBytes([0x00]).finish() }
    }

    @Test func everyAnkiRecordDecodesInG6sMemberOrder() throws {
        let cardCounts = try decoded(.cardCounts) { w in
            for value in [1, 2, 3, 4, 5, 6, 7, 10, 20, 30, 40, 50, 0, 0] { w.integer(value) }
            w.integer(9)
            w.list([(0, "is:new"), (5, "is:suspended")]) { w, label in w.integer(label.0); w.text(label.1) }
            w.list([(3, "")]) { w, label in w.integer(label.0); w.text(label.1) }
            for value in [6, 0, 0, 1, 2] { w.integer(value) }
        }
        #expect(cardCounts == .cardCounts(StatsCardCounts(
            excludingInactive: .init(new: 1, learning: 2, relearning: 3, young: 4, mature: 5, suspended: 6, buried: 7),
            includingInactive: .init(new: 10, learning: 20, relearning: 30, young: 40, mature: 50, suspended: 0, buried: 0),
            notes: 9,
            labelsSeparate: [.init(label: .new, itemText: "is:new"), .init(label: .suspended, itemText: "is:suspended")],
            labelsTogether: [.init(label: .young, itemText: "")],
            suspendedByState: .init(new: 6, learning: 0, relearning: 0, young: 1, mature: 2))))

        let futureDue = try decoded(.futureDue) { w in
            w.map([-3, 0, 1, 40], [[2], [5], [7], [1]]); w.integer(1); w.integer(12); w.integer(7); w.map([0, 1], [[4], [6]])
        }
        #expect(futureDue == .futureDue(StatsFutureDue(
            due: StatsSparseMap(keys: [-3, 0, 1, 40], values: [2, 5, 7, 1]), haveBacklog: true, dailyLoad: 12,
            dueTomorrow: 7, dueYoung: StatsSparseMap(keys: [0, 1], values: [4, 6]))))

        #expect(try decoded(.intervals) { $0.map([1, 2, 4_294_967_295], [[3], [1], [1]]) }
            == .intervals(StatsSparseMap(keys: [1, 2, 4_294_967_295], values: [3, 1, 1])))
        #expect(try decoded(.stability) { w in w.map([0, 5], [[1], [2]]); w.integer(4) }
            == .stability(StatsStability(byDays: StatsSparseMap(keys: [0, 5], values: [1, 2]), leftOut: 4)))
        #expect(try decoded(.difficulty) { w in w.map([30], [[2]]); w.float32(30.5); w.integer(1) }
            == .difficulty(StatsDifficulty(byPercent: StatsSparseMap(keys: [30], values: [2]), median: 30.5, leftOut: 1)))
        #expect(try decoded(.retrievability) { w in
            w.map([90, 99], [[1], [1]]); w.float32(0.94); w.float32(1.88); w.float32(1.5); w.integer(0)
        } == .retrievability(StatsRetrievability(byPercent: StatsSparseMap(keys: [90, 99], values: [1, 1]),
                                                 average: 0.94, sumByCard: 1.88, sumByNote: 1.5, leftOut: 0)))
        #expect(try decoded(.added) { $0.map([-2, 0], [[3], [1]]) } == .added(StatsSparseMap(keys: [-2, 0], values: [3, 1])))

        // `reviews`: ten members, Anki's only (By Answer is its own part).
        let reviews = try decoded(.reviews) { w in
            w.map([-1, 0], [[1, 2, 3, 4, 5, 10, 20, 30, 40, 50], [0, 0, 1, 0, 0, 0, 0, 900, 0, 0]])
        }
        #expect(reviews == .reviews(StatsReviews(
            count: StatsSparseMap(keys: [-1, 0], values: [.init(learn: 1, relearn: 2, young: 3, mature: 4, filtered: 5),
                                                          .init(learn: 0, relearn: 0, young: 1, mature: 0, filtered: 0)]),
            time: StatsSparseMap(keys: [-1, 0], values: [.init(learn: 10, relearn: 20, young: 30, mature: 40, filtered: 50),
                                                         .init(learn: 0, relearn: 0, young: 900, mature: 0, filtered: 0)]))))

        let counts = { (base: Int) in StatsRetentionCounts(youngPassed: base, youngFailed: base + 1, maturePassed: base + 2,
                                                           matureFailed: base + 3) }
        for periods in [5, 6] {
            let value = try decoded(.retention) { w in
                w.list(Array(0..<periods)) { w, period in for at in 0..<4 { w.integer(period * 10 + at) } }
            }
            #expect(value == .retention(StatsTrueRetention(today: counts(0), yesterday: counts(10), week: counts(20),
                                                           month: counts(30), year: counts(40),
                                                           allTime: periods == 6 ? counts(50) : nil)))
        }
        #expect(throws: StatsBytes.Failure.self) {
            try decoded(.retention) { w in w.list(Array(0..<4)) { w, _ in for _ in 0..<4 { w.integer(0) } } }
        }

        // `hours`: Anki's ranges, then H7's six months, then retention by hour in time order.
        let day = { (total: Int) in (0..<24).map { StatsHour(total: total, correct: $0) } }
        let passFail = { (passed: Int) in (0..<24).map { StatsHourRetention(passed: passed, failed: $0) } }
        let hours = try decoded(.hours) { w in
            w.list([1, 3, 12, 99]) { w, total in w.list(Array(0..<24)) { w, hour in w.integer(total); w.integer(hour) } }
            w.list(Array(0..<24)) { w, hour in w.integer(6); w.integer(hour) }
            w.list([1, 3, 6, 12, 99]) { w, passed in w.list(Array(0..<24)) { w, hour in w.integer(passed); w.integer(hour) } }
        }
        #expect(hours == .hours(StatsHours(
            hours: StatsRanges(oneMonth: day(1), threeMonths: day(3), sixMonths: day(6), oneYear: day(12), allTime: day(99)),
            retentionByHour: StatsRanges(oneMonth: passFail(1), threeMonths: passFail(3), sixMonths: passFail(6),
                                         oneYear: passFail(12), allTime: passFail(99)))))

        let presses = { (base: Int) in StatsButtonPresses(learning: [base, 0, 0, 1], young: [0, base, 0, 0], mature: [0, 0, base, 2]) }
        let buttons = try decoded(.buttons) { w in
            let range = { (w: inout G6Writer, base: Int) in
                for row in [[base, 0, 0, 1], [0, base, 0, 0], [0, 0, base, 2]] { w.list(row) { w, v in w.integer(v) } }
            }
            w.list([1, 3, 12]) { w, base in range(&w, base) }
            range(&w, 6)
        }
        #expect(buttons == .buttons(StatsButtons(ranges: StatsRanges(oneMonth: presses(1), threeMonths: presses(3),
                                                                     sixMonths: presses(6), oneYear: presses(12), allTime: nil))))
    }

    @Test func thePagesOwnPartsDecodeInTheBridgesMemberOrder() throws {
        let byAnswer = try decoded(.byAnswer) { w in
            w.map([-3, -2], [[1, 2, 3, 4, 100, 200, 300, 400], [0, 0, 5, 0, 0, 0, 500, 0]]); w.integer(2)
        }
        #expect(byAnswer == .byAnswer(StatsByAnswer(
            count: StatsSparseMap(keys: [-3, -2], values: [.init(again: 1, hard: 2, good: 3, easy: 4), .init(again: 0, hard: 0, good: 5, easy: 0)]),
            time: StatsSparseMap(keys: [-3, -2], values: [.init(again: 100, hard: 200, good: 300, easy: 400),
                                                         .init(again: 0, hard: 0, good: 500, easy: 0)]),
            unrated: 2)))

        let states = try decoded(.cardStatesOverTime) { w in
            for value in [5, 1, 0, 2, 3] { w.integer(value) }
            w.map([-10, -9, 0], [[-1, 1, 0, 0, 0], [0, -1, 0, 1, 0], [0, 0, 0, -1, 1]])
            w.integer(4)
            w.list([-400]) { w, day in w.integer(day) }
        }
        guard case .cardStatesOverTime(let read) = states else { Issue.record("\(states)"); return }
        #expect(read.start == StatsStateCounts(new: 5, learning: 1, relearning: 0, young: 2, mature: 3))
        #expect(read.leftOut == 4 && read.firstDay == -400)
        #expect(read.counts(on: -11) == read.start)
        #expect(read.counts(on: -9) == StatsStateCounts(new: 4, learning: 1, relearning: 0, young: 3, mature: 3))
        #expect(read.counts(on: 0) == StatsStateCounts(new: 4, learning: 1, relearning: 0, young: 2, mature: 4))

        #expect(try decoded(.started) { w in w.map([-5, -4], [[1], [2]]); w.list([Int]()) { w, d in w.integer(d) } }
            == .started(StatsStarted(days: StatsSparseMap(keys: [-5, -4], values: [1, 2]), firstDay: nil)))
        // An optional value of two items is refused.
        #expect(throws: StatsBytes.Failure.self) {
            try decoded(.started) { w in w.map([], []); w.list([1, 2]) { w, d in w.integer(d) } }
        }
        let knowledge = try decoded(.knowledgeOverTime) { w in
            w.list([(-1, 1_894_600_799_000, 1.75, 2), (0, 1_894_701_600_000, 1.5, 2)]) { w, sample in
                w.integer(sample.0); w.integer(sample.1); w.float64(sample.2); w.integer(sample.3)
            }
            w.integer(1); w.integer(3)
        }
        #expect(knowledge == .knowledgeOverTime(StatsKnowledgeOverTime(samples: [
            StatsKnowledgeSample(day: -1, instantMilliseconds: 1_894_600_799_000, knowledge: 1.75, studied: 2),
            StatsKnowledgeSample(day: 0, instantMilliseconds: 1_894_701_600_000, knowledge: 1.5, studied: 2),
        ], failed: 1, leftOut: 3)))

        #expect(try decoded(.studiedToday) { w in w.integer(12); w.integer(7); w.integer(65_000) }
            == .studiedToday(StatsStudiedToday(answers: 12, cards: 7, timeMilliseconds: 65_000)))
        // Truncated bytes are refused, never taken as data.
        #expect(throws: StatsBytes.Failure.self) { try decoded(.studiedToday) { w in w.integer(12) } }

        let retention = try decoded(.retentionByDay) { w in
            w.map([-2, 0], [[3, 1, 4, 0, 8], [0, 0, 2, 2, 4]])
            w.list([7.2, 3.6]) { w, sum in w.float64(sum) }
            w.list([Float(0.9), 0.95]) { w, r in w.float32(r) }
            w.integer(1)
        }
        #expect(retention == .retentionByDay(StatsRetentionByDay(
            days: StatsSparseMap(keys: [-2, 0], values: [
                StatsRetentionDay(youngPassed: 3, youngFailed: 1, maturePassed: 4, matureFailed: 0, desiredSum: 7.2, desiredRows: 8),
                StatsRetentionDay(youngPassed: 0, youngFailed: 0, maturePassed: 2, matureFailed: 2, desiredSum: 3.6, desiredRows: 4),
            ]),
            retentions: [0.9, 0.95], several: true)))
        // One desired sum per day, no more and no less.
        #expect(throws: StatsBytes.Failure.self) {
            try decoded(.retentionByDay) { w in
                w.map([0], [[1, 0, 0, 0, 1]]); w.list([Double]()) { w, sum in w.float64(sum) }
                w.list([Float]()) { w, r in w.float32(r) }; w.integer(0)
            }
        }

        let values = { (base: Int) in StatsDeckValues(reviews: base, timeMilliseconds: base * 1_000, passed: base - 1, failed: 1) }
        let byDeck = try decoded(.byDeck) { w in
            w.list([(1_788_249_600_212, 1, 30, true), (1_788_249_600_214, 0, 5, false)]) { w, row in
                w.integer(row.0); w.integer(row.1); w.integer(row.2)
                for base in [2, 3, 6, 12] { w.integer(base); w.integer(base * 1_000); w.integer(base - 1); w.integer(1) }
                w.list(row.3 ? [99] : []) { w, base in w.integer(base); w.integer(base * 1_000); w.integer(base - 1); w.integer(1) }
            }
        }
        #expect(byDeck == .byDeck(StatsByDeck(rows: [
            StatsDeckRow(deckID: 1_788_249_600_212, ownCards: true, cards: 30,
                         windows: StatsRanges(oneMonth: values(2), threeMonths: values(3), sixMonths: values(6),
                                              oneYear: values(12), allTime: values(99))),
            StatsDeckRow(deckID: 1_788_249_600_214, ownCards: false, cards: 5,
                         windows: StatsRanges(oneMonth: values(2), threeMonths: values(3), sixMonths: values(6),
                                              oneYear: values(12), allTime: nil)),
        ])))

        #expect(try decoded(.activityDays) { $0.map([-800, -799, 0], [[4], [1], [9]]) }
            == .activityDays(StatsSparseMap(keys: [-800, -799, 0], values: [4, 1, 9])))
    }

    @Test func periodsReadTheirHistoryAndOnlyAllNeedsANewRequest() {
        #expect(StatsPagePeriod.allCases.filter { $0.history == .allHistory } == [.all])
        #expect(!StatsPagePeriod.oneYear.needsNewRequest(after: .oneMonth))
        #expect(!StatsPagePeriod.threeMonths.needsNewRequest(after: .sixMonths))
        #expect(StatsPagePeriod.all.needsNewRequest(after: .oneYear))
        #expect(StatsPagePeriod.oneMonth.needsNewRequest(after: .all))
        #expect(StatsPart.allCases.map(\.tier) == [.present, .present, .present, .present, .present, .present, .present]
            + Array(repeating: StatsTier.reviewLog, count: 11) + [.knowledge])
    }

    @Test func staleEventsAreG10s() {
        let changes = CoreBridge.ChangeSet(MeguriCoreBindings.ChangeSet(
            wholeCollection: false, source: .userUndoable, cards: [], notes: [], notetypes: [], decks: [], deckConfigs: [],
            configKeys: [], tags: [], revlogAdded: [], revlogRemoved: [], mediaAdded: [], mediaRemoved: [],
            skippedIncomingCards: [], skippedIncomingNotes: [], skippedIncomingDecks: [], collectionRow: false))
        #expect(StatsStaleness(.changes(changes)) == .graphsStale)
        #expect(StatsStaleness(.rolledOver(oldDay: 1, newDay: 2)) == .dayChanged)
        #expect(StatsStaleness(.nextDayAtChanged(nextDayAt: 5)) == .dayChanged)
        #expect(StatsStaleness(.timeZoneChanged(zoneID: "UTC")) == .dayChanged)
        #expect(StatsStaleness(.collectionReplaced(generation: 3)) == .collectionReplaced)
        #expect(StatsStaleness(.available) == nil)
    }
}

@Suite struct StatsRouterTests {
    @Test func theHeaderComesWithEveryPartAndEveryKindCrosses() async {
        var bytes = G6Writer()
        bytes.map([0], [[3]])
        let out = await routed([
            .progress(part: .started, progress: .fraction(fraction: 0.25)),
            .header(header: header()),
            .part(part: .added, outcome: .data(bytes: bytes.data)),
            .part(part: .cardCounts, outcome: .failed(failure: .unsupportedScope)),
            .part(part: .started, outcome: .failed(failure: .unreadableRow(row: .init(table: "revlog", column: "ease", key: 5)))),
            .part(part: .byDeck, outcome: .failed(failure: .notBuilt)),
            .part(part: .hours, outcome: .failed(failure: .failed(message: "disk full"))),
            // Bytes that do not account for themselves are never data.
            .part(part: .intervals, outcome: .data(bytes: bytes.data + Data([0]))),
            .progress(part: .knowledgeOverTime, progress: .indeterminate),
            .header(header: header(refresh: 5)),
            .part(part: .stability, outcome: .failed(failure: .notBuilt)),
            .end(end: .contextChanged),
        ])
        let first = StatsPageHeader(header())
        #expect(first.context.zoneIdentifier == "Europe/Paris" && first.context.utcOffsetSeconds == 3_600)
        #expect(first.oldestDay == .day(-900) && first.browseScope == .text("deck:Lang") && first.preferences.fsrs)
        let part = { (part: StatsPart, header: StatsPageHeader, outcome: StatsPartOutcome) in
            StatsPageMessage.part(StatsPartResult(part: part, header: header, outcome: outcome))
        }
        #expect(out == [
            .progress(.started, .fraction(0.25)),
            part(.added, first, .data(.added(StatsSparseMap(keys: [0], values: [3])))),
            part(.cardCounts, first, .failed(.unsupportedScope)),
            part(.started, first, .failed(.unreadableRow(StatsUnreadable(.init(table: "revlog", column: "ease", key: 5))))),
            part(.byDeck, first, .failed(.notBuilt)),
            part(.hours, first, .failed(.failed("disk full"))),
            part(.intervals, first, .failed(.undecodable("1 bytes left"))),
            .progress(.knowledgeOverTime, .indeterminate),
            part(.stability, StatsPageHeader(header(refresh: 5)), .failed(.notBuilt)),
            .end(.contextChanged),
        ])

        for (raw, end) in [(MeguriCoreBindings.StatsPageEnd.complete, StatsPageEnd.complete),
                           (.scopeError(error: .unknownDeck), .scopeError(.unknownDeck)),
                           (.unreadableRow(row: .init(table: "cards", column: "type", key: 9)),
                            .unreadableRow(StatsUnreadable(.init(table: "cards", column: "type", key: 9)))),
                           (.cancelled, .cancelled),
                           (.failed(error: .Busy(runningOp: "check")), .failed(.busy(runningOperation: "check")))] {
            #expect(await routed([.end(end: raw)]) == [.end(end)])
        }
    }

    @Test func aJobThatEndsWithoutItsEndEndsTheStreamWithItsOutcome() async {
        #expect(await routed(events: [.jobEnded(.cancelled)]) == [.end(.cancelled)])
        #expect(await routed(events: [.jobEnded(.failed(.internal("panicked")))]) == [.end(.failed(.internal("panicked")))])
        #expect(await routed(events: [.jobEnded(.succeeded)])
            == [.end(.failed(.internal("the page request ended without its end")))])
        // After a real end the job's outcome changes nothing.
        #expect(await routed(events: [.core(.end(end: .complete)), .jobEnded(.failed(.internal("late")))]) == [.end(.complete)])
        // A part before any header is never dropped quietly: the request fails.
        #expect(await routed([.part(part: .added, outcome: .failed(failure: .notBuilt))])
            == [.end(.failed(.internal("a part arrived before its header")))])
    }

    @Test func theIteratorSkipsWhatWasBufferedBeforeACancel() async {
        let state = StatsPageCancel()
        let (stream, input) = AsyncStream.makeStream(of: StatsPageMessage.self)
        let result = StatsPartResult(part: .cardCounts, header: StatsPageHeader(header()), outcome: .failed(.notBuilt))
        input.yield(.part(result))
        input.yield(.progress(.started, .indeterminate))
        input.yield(.end(.complete))
        input.finish()
        var iterator = StatsPageIterator(base: stream.makeAsyncIterator(), state: state)
        #expect(await iterator.next() == .part(result))
        state.cancelled.withLock { $0 = true }
        #expect(await iterator.next() == .end(.cancelled))
        #expect(await iterator.next() == nil)

        // A stream that stops without its end (the consuming task was cancelled) ends cancelled.
        let (cut, cutInput) = AsyncStream.makeStream(of: StatsPageMessage.self)
        cutInput.yield(.part(result))
        cutInput.finish()
        var cutIterator = StatsPageIterator(base: cut.makeAsyncIterator(), state: StatsPageCancel())
        #expect(await cutIterator.next() == .part(result))
        #expect(await cutIterator.next() == .end(.cancelled))
        #expect(await cutIterator.next() == nil)
    }

    @Test func nothingButACancelledEndFollowsACancel() async {
        let out = await routed([
            .header(header: header()),
            .part(part: .cardCounts, outcome: .failed(failure: .notBuilt)),
            .part(part: .futureDue, outcome: .failed(failure: .notBuilt)),
            .progress(part: .started, progress: .indeterminate),
            .end(end: .complete),
        ], cancelAfter: 2)
        #expect(out == [
            .part(StatsPartResult(part: .cardCounts, header: StatsPageHeader(header()), outcome: .failed(.notBuilt))),
            .end(.cancelled),
        ])
    }
}

private final class StatsClock: CollectionClock {
    func nowMilliseconds() -> Int64 { 1_894_701_600_000 }
    func utcOffsetSeconds(atMilliseconds: Int64) -> Int32 { 0 }
    func zoneIdentifier() -> String { "UTC" }
}

private final class StatsDiskCapacity: DiskSpaceProviding {
    func availableBytes(at directory: URL) throws -> UInt64 { .max }
}

extension EventTests {
    /// One page request on a copy of the schema-18 fixture (11 cards, 6 review-log rows), all
    /// history: one header, each part once by tier with its value, the end, and no relay kept
    /// once the job has ended; the cards behind a day; then a cancelled request.
    @Test(.timeLimit(.minutes(2))) func aPageRequestStreamsItsPartsOnAFixtureCollection() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let directory = root.appending(path: "collection")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: root.appending(path: "backups"), withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let fixture = URL(filePath: #filePath).deletingLastPathComponent().appending(
            path: "../../../../../conformance/fixtures/storage/schema-18.anki2")
        try FileManager.default.copyItem(at: fixture, to: directory.appending(path: "collection.anki2"))
        let location = CoreBridge.CollectionLocation(collectionDirectory: directory,
            backupsDirectory: root.appending(path: "backups"), appGroupSnapshot: root.appending(path: "widget.snapshot"),
            manualTransferDirectory: root.appending(path: "ManualTransfers"))
        let service = CoreCollectionService(clock: StatsClock(), diskSpace: StatsDiskCapacity())
        let stats = CoreStatsService()
        do {
            try await service.open(location, randomSeed: 11)
            let before = StatsPageRelay.live.withLock { $0 }
            let stream = try await stats.page(StatsPageRequest(scope: .allDecks, period: .all, refresh: 5))
            var results: [StatsPartResult] = []
            var ends: [StatsPageEnd] = []
            for await message in stream {
                switch message {
                case .part(let result): results.append(result)
                case .progress: break
                case .end(let end): ends.append(end)
                }
            }
            #expect(ends == [.complete])
            #expect(await stream.job.outcome() == .succeeded)
            #expect(StatsPageRelay.live.withLock { $0 } == before, "the core released the request's relay")

            #expect(Set(results.map(\.part)) == Set(StatsPart.allCases))
            #expect(results.count == StatsPart.allCases.count, "each part once")
            #expect(results.map(\.part.tier) == results.map(\.part.tier).sorted(), "by tier")
            let header = try #require(results.first?.header)
            #expect(results.allSatisfy { $0.header == header })
            #expect(header.refresh == 5 && header.history == .allHistory && header.browseScope == .none)
            #expect(header.context.nowMilliseconds == 1_894_701_600_000 && header.context.zoneIdentifier == "UTC")

            let outcome = { (part: StatsPart) in results.first { $0.part == part }?.outcome }
            guard case .day = header.oldestDay else { Issue.record("oldest day \(header.oldestDay)"); return }
            for part in StatsPart.allCases {
                guard case .data = outcome(part) else { Issue.record("\(part): \(String(describing: outcome(part)))"); continue }
            }
            guard case .data(.cardCounts(let counts)) = outcome(.cardCounts),
                  case .data(.reviews(let reviews)) = outcome(.reviews),
                  case .data(.byAnswer(let byAnswer)) = outcome(.byAnswer),
                  case .data(.cardStatesOverTime(let states)) = outcome(.cardStatesOverTime) else {
                Issue.record("values"); return
            }
            let total = { (c: StatsCardStateCounts) in c.new + c.learning + c.relearning + c.young + c.mature + c.suspended + c.buried }
            #expect(total(counts.includingInactive) == 11 && total(counts.excludingInactive) == 11)
            // By Answer is the rows Reviews counts that have an answer: never more.
            #expect(byAnswer.count.values.map(\.total).reduce(0, +) + byAnswer.unrated
                == reviews.count.values.map(\.total).reduce(0, +))
            // The last day of Cards Over Time is Card Counts without the suspended cards (H2.2).
            if case .data(.knowledgeOverTime(let knowledge)) = outcome(.knowledgeOverTime) {
                #expect(knowledge.samples.allSatisfy { $0.knowledge <= Double($0.studied) + 1e-9 })
                #expect(knowledge.samples.last.map { $0.day == 0 } ?? true, "today's point last")
            }
            // H13 on the day of the oldest row, and on a day after today.
            if case .day(let oldest) = header.oldestDay {
                let behind = try await stats.dayCards(scope: .allDecks, context: header.context, day: oldest, kind: .reviewed,
                                                      cancel: nil)
                if case .cards(let ids) = behind { #expect(!ids.isEmpty && ids == ids.sorted()) } else { #expect(behind == .noCards) }
            }
            #expect(try await stats.dayCards(scope: .allDecks, context: header.context, day: 5, kind: .answered,
                                             cancel: nil) == .noCards)
            #expect(try await stats.dayCards(scope: .search("deck:*"), context: header.context, day: 0, kind: .forgotten,
                                             cancel: nil) == .unsupportedScope)
            let today = states.counts(on: 0)
            #expect(today.total + states.leftOut == total(counts.includingInactive))

            // A request cancelled at once ends cancelled, and its relay goes too.
            let cancelled = try await stats.page(StatsPageRequest(scope: .deck(1_788_249_600_212), period: .oneYear,
                                                                  refresh: 6))
            await cancelled.cancel()
            var last: StatsPageMessage?
            for await message in cancelled { last = message }
            // Once cancelled here, the end is `.cancelled` whatever the core returned; the core's own
            // stop within 50 ms is `stats_page_ffi`'s H16 test.
            #expect(last == .end(.cancelled))
            #expect([.cancelled, .succeeded].contains(await cancelled.job.outcome()))
            #expect(StatsPageRelay.live.withLock { $0 } == before)

            try await stats.releaseCaches()  // a job: it waits for its JobFinished
            try await stats.homeIdle()
            try await service.close()
        } catch {
            try? await service.close()
            throw error
        }
    }
}
