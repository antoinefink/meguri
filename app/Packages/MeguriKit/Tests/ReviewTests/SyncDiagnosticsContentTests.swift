import AppModel
import Foundation
import Synchronization
import Testing
@testable import Settings

@MainActor @Suite struct SyncDiagnosticsContentTests {
    private let locale = Locale(identifier: "en")
    private let instant = Date(timeIntervalSince1970: 1_800_000_000)

    @Test func routineDetailsKeepLastWholeExchangeWithKnownAndUnknownMediaWork() throws {
        for fixture in [SyncCoordinator.RenderState.meguriSyncing, .meguriApplying, .meguriCheckingMedia,
                        .meguriSyncingMedia, .meguriSyncingMediaUnknown] {
            let sync = SyncCoordinator.renderFixture(fixture, clock: .fixed(instant))
            let at = try #require(sync.meguriLastExchange)
            let last = SyncText.format("sync.status.upToDate.detail", SyncText.instant(at, now: instant))
            let detail = try #require(SyncStatusCopy.detail(sync, now: instant))
            #expect(detail.contains(last))
            if fixture == .meguriSyncingMedia { #expect(detail.contains("42")) }
            if fixture == .meguriSyncingMediaUnknown { #expect(sync.mainStatus.detailCount == nil) }
        }
    }

    @Test func roundLogNamesTheObservedBoundaryAndCorrelation() async throws {
        let id = UUID()
        let value = await snapshot([.init(component: .cloud, operation: .exchange, stage: .statusObserved,
            outcome: .observed, durationMilliseconds: 12, roundID: id, roundMilestone: .serverAcknowledged)])
        let presentation = SyncDiagnosticsPresentation(value, locale: locale)
        let row = try #require(presentation.rows.first)
        #expect(row.result.contains("Server finish acknowledged"))
        #expect(row.observedTime.contains(id.uuidString))
        #expect(row.duration?.contains("12") == true)
    }

    @Test func sharedAdmissionSurvivesCancelledConsumersAndRemountUntilProducersReturn() async throws {
        // Owned scalar fixture, no diagnostics store or system clipboard.
        let value = try JSONDecoder().decode(FieldDiagnosticTimeline.self, from: Data("""
        {"schemaVersion":1,"events":[],"droppedCount":0,"expiredCount":0,"truncatedCount":0,"invalidCount":0,"priorHistoryUnavailable":false,"storage":"available"}
        """.utf8))
        let pendingRead = SuspendedDiagnosticResult<FieldDiagnosticTimeline>()
        let pendingExport = SuspendedDiagnosticResult<Data>()
        var reads = 0, exports = 0
        var writes: [String] = []
        let callbacks = FieldDiagnosticsCallbacks(snapshot: {
            reads += 1
            if reads == 1 { return try await pendingRead.read() }
            return value
        }, safeExport: {
            exports += 1
            if exports == 1 { return try await pendingExport.read() }
            return Data("current safe report".utf8)
        }, writeClipboard: { writes.append($0); return true })
        func consumer() -> SyncDiagnosticsModel {
            SyncDiagnosticsModel(snapshot: { try await callbacks.snapshot() },
                safeExport: { try await callbacks.safeExport() }, writeClipboard: callbacks.writeClipboard)
        }
        let first = consumer()
        first.mount()
        let read = Task { await first.refresh() }
        await pendingRead.waitUntilRequested()
        first.requestCopy()
        let copy = Task { await first.copy() }
        await pendingExport.waitUntilRequested()
        first.unmount(); read.cancel(); copy.cancel()
        let second = consumer()
        second.mount(); await second.refresh()
        second.requestCopy(); await second.copy()
        #expect(second.snapshotUnavailable && second.copyFeedback == .unavailable)
        second.unmount()
        let remade = consumer()
        remade.mount(); await remade.refresh()
        remade.requestCopy(); await remade.copy()
        #expect(reads == 1 && exports == 1 && writes.isEmpty)
        pendingRead.finish(value)
        await read.value
        await remade.refresh()
        remade.requestCopy(); await remade.copy()
        #expect(reads == 2 && exports == 1 && writes.isEmpty)
        pendingExport.finish(Data("stale safe report".utf8))
        await copy.value
        #expect(first.timeline == nil && writes.isEmpty)
        remade.requestCopy(); await remade.copy()
        #expect(exports == 2 && writes == ["current safe report"])
        #expect(remade.copyFeedback == .copied && remade.timeline != nil)
    }

    /// Each fixture owns an isolated, resolved temporary directory. Never use
    /// the standard store or the user's Application Support directory.
    private func directory() -> URL {
        FileManager.default.temporaryDirectory.resolvingSymlinksInPath()
            .appending(path: "SyncDiagnosticsPresentation-\(UUID().uuidString)", directoryHint: .isDirectory)
    }

    private func snapshot(_ observations: [FieldDiagnosticObservation] = []) async -> FieldDiagnosticTimeline {
        let root = directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = FieldDiagnosticsStore(directory: root, clock: .fixed(instant))
        for observation in observations { store.enqueue(observation) }
        return await store.timeline()
    }

    private func observation(component: FieldDiagnosticObservation.Component = .cloud,
                             stage: FieldDiagnosticObservation.Stage = .requested,
                             outcome: FieldDiagnosticObservation.Outcome = .observed,
                             count: UInt64? = nil) -> FieldDiagnosticObservation {
        .init(component: component, operation: .exchange, stage: stage, outcome: outcome, count: count)
    }

    @Test func emptyAndLongTimelinesShowOnlyTheLatest20CloudMediaObservationsInFIFOOrder() async {
        let empty = SyncDiagnosticsPresentation(await snapshot(), locale: locale)
        #expect(empty.rows.isEmpty)
        #expect(empty.notices == ["No recent cloud or media observations are available."])
        var observations: [FieldDiagnosticObservation] = []
        for index in 0..<30 {
            observations.append(observation(component: index.isMultiple(of: 2) ? .cloud : .media, count: UInt64(index)))
            observations.append(.init(component: .review, operation: .answer, stage: .persistence,
                                      outcome: .failed, failure: .transport, count: 9_999))
        }
        let presentation = SyncDiagnosticsPresentation(await snapshot(observations), locale: locale)
        #expect(presentation.rows.count == 20)
        #expect(presentation.rows.compactMap(\.count) == (10..<30).map { "Recorded count: \($0)" })
        #expect(presentation.rows.first?.summary == "Cloud: Sync data")
        #expect(presentation.rows.last?.summary == "Media: Sync data")
        #expect(presentation.rows.allSatisfy { $0.failure == nil })
        #expect(presentation.notices.contains("Only the latest 20 cloud and media observations are shown."))
        #expect(!presentation.rows.compactMap(\.count).joined().contains("9999"))
    }

    @Test func stageOutcomeAndUnknownMeasurementsDoNotInventSuccessOrZero() async {
        let timeline = await snapshot([
            observation(),
            observation(stage: .exchangeCompleted, outcome: .incomplete, count: 0),
            observation(stage: .statusObserved, outcome: .unknown),
            observation(stage: .publication, outcome: .waiting),
            observation(stage: .admission, outcome: .refused),
            observation(stage: .cleanup, outcome: .cancelled),
            .init(component: .media, operation: .transfer, stage: .publication, outcome: .failed,
                  failure: .permission, count: .max, durationMilliseconds: 42),
            observation(stage: .persistence, outcome: .succeeded),
        ])
        let rows = SyncDiagnosticsPresentation(timeline, locale: locale).rows
        #expect(rows.map(\.result) == [
            "Requested · Observed", "Data exchange finished · Incomplete",
            "Status checked · Outcome not recorded", "Publishing · Pending",
            "Request checked · Refused", "Cleaning up · Cancelled",
            "Publishing · Failed", "Saving · Step succeeded",
        ])
        #expect(rows[0].count == nil)
        #expect(rows[0].duration == nil)
        #expect(rows[1].count == "Recorded count: 0")
        #expect(rows[6].duration == "Recorded duration: 42 ms")
        #expect(rows[6].failure == "Failure reason: Permission denied")
        #expect(rows[6].count == "Recorded count: 18,446,744,073,709,551,615")
    }

    @Test func rollbackTimesAreDisplayedUnadjustedWithoutReordering() async {
        let root = directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let clock = Mutex(instant)
        let store = FieldDiagnosticsStore(directory: root, clock: AppClock { clock.withLock { $0 } })
        store.enqueue(observation(count: 1))
        _ = await store.timeline()
        clock.withLock { $0 = instant.addingTimeInterval(-1) }
        store.enqueue(observation(stage: .exchangeCompleted, outcome: .incomplete, count: 2))
        let timeline = await store.timeline()
        #expect(timeline.events.map(\.observedAtUnixMilliseconds) == [1_800_000_000_000, 1_799_999_999_000])
        let utc = TimeZone(secondsFromGMT: 0)!
        for displayLocale in [locale, Locale(identifier: "de_DE")] {
            let rows = SyncDiagnosticsPresentation(timeline, locale: displayLocale, timeZone: utc).rows
            #expect(rows.compactMap(\.count).count == 2)
            let formatter = DateFormatter()
            formatter.locale = displayLocale
            formatter.timeZone = utc
            formatter.dateStyle = .medium
            formatter.timeStyle = .medium
            #expect(rows.map(\.observedTime) == [instant, instant.addingTimeInterval(-1)].map { "Observed at \(formatter.string(from: $0))" })
        }
        #expect(SyncDiagnosticsPresentation(timeline, locale: locale).rows.compactMap(\.count) == ["Recorded count: 1", "Recorded count: 2"])
    }

    @Test func corruptHistoryHasUnknownLossWhileAvailableRowsRemainVisible() async throws {
        let root = directory()
        defer { try? FileManager.default.removeItem(at: root) }
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        try Data("{\"events\":".utf8).write(to: root.appending(path: "timeline.json"))
        let store = FieldDiagnosticsStore(directory: root, clock: .fixed(instant))
        store.enqueue(observation())
        let presentation = SyncDiagnosticsPresentation(await store.timeline(), locale: locale)
        #expect(presentation.rows.count == 1)
        #expect(presentation.notices.contains("Some earlier history is unavailable. The number of missing observations is unknown."))
        #expect(presentation.notices.contains("Saved history could not be read. Available observations are shown."))
        #expect(!presentation.notices.contains(where: { $0.contains("Known dropped") }))
    }

    @Test func knownLossCountersStaySeparateFromUnknownHistoryAndVisibleRowLimit() async throws {
        let root = directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = FieldDiagnosticsStore(directory: root, clock: .fixed(instant))
        let empty = await store.timeline()
        // A persisted public-schema fixture, read by the real store. No internal
        // AppModel initializer or production-only test hook is needed.
        var json = try #require(JSONSerialization.jsonObject(with: JSONEncoder().encode(empty)) as? [String: Any])
        json["droppedCount"] = 3; json["expiredCount"] = 4
        json["truncatedCount"] = 5; json["invalidCount"] = 6
        json["priorHistoryUnavailable"] = true
        json["priorHistoryUnavailableReason"] = "unavailable"
        try JSONSerialization.data(withJSONObject: json).write(to: root.appending(path: "timeline.json"))
        let reopened = FieldDiagnosticsStore(directory: root, clock: .fixed(instant))
        let presentation = SyncDiagnosticsPresentation(await reopened.timeline(), locale: locale)
        #expect(presentation.notices.contains("Known dropped observations, across the app: 3"))
        #expect(presentation.notices.contains("Expired observations, across the app: 4"))
        #expect(presentation.notices.contains("Older observations removed, across the app: 5"))
        #expect(presentation.notices.contains("Invalid observations omitted, across the app: 6"))
        #expect(presentation.notices.contains("Some earlier history is unavailable. The number of missing observations is unknown."))
    }

    @Test func refreshFailurePreservesPriorDataWithoutDescribingTheError() async {
        let value = await snapshot([observation(count: 7)])
        let failure = DiagnosticRefreshFailureFixture()
        var reads = 0
        var exports = 0
        var writes = 0
        let model = SyncDiagnosticsModel(snapshot: {
            reads += 1
            if failure.fails { throw UndescribableDiagnosticError() }
            return value
        }, safeExport: { exports += 1; throw UndescribableDiagnosticError() }, writeClipboard: { _ in
            writes += 1
            Issue.record("Refresh must never write the clipboard")
            return false
        })
        // Unmounted callbacks are never invoked.
        await model.refresh()
        model.requestCopy()
        await model.copy()
        #expect(model.timeline == nil && reads == 0 && exports == 0 && writes == 0)
        model.mount()
        await model.refresh()
        failure.fails = true
        model.requestRefresh()
        await model.refresh()
        #expect(model.snapshotUnavailable)
        #expect(model.timeline?.events == value.events)
        #expect(!model.isLoading)
        failure.fails = false
        await model.refresh()
        #expect(!model.snapshotUnavailable)
    }

    @Test func busyRefreshAdmitsOneNoncooperativeReadAndCanRefreshAgainAfterCompletion() async {
        let old = SuspendedDiagnosticResult<FieldDiagnosticTimeline>()
        let oldValue = await snapshot([observation(count: 1)])
        let newValue = await snapshot([observation(count: 2)])
        var reads = 0
        let model = SyncDiagnosticsModel(snapshot: {
            reads += 1
            if reads == 1 { return try await old.read() }
            return newValue
        }, safeExport: { Data() }, writeClipboard: { _ in false })
        model.mount()
        let first = Task { await model.refresh() }
        await old.waitUntilRequested()
        let request = model.refreshRequest
        for _ in 0..<20 {
            model.requestRefresh()
            await model.refresh()
        }
        #expect(reads == 1 && model.refreshRequest == request && model.refreshInFlight)
        old.finish(oldValue)
        await first.value
        #expect(model.timeline?.events == oldValue.events)
        #expect(!model.refreshInFlight)
        model.requestRefresh()
        await model.refresh()
        #expect(model.timeline?.events == newValue.events)
        #expect(reads == 2 && !model.isLoading && !model.snapshotUnavailable)
    }

    @Test(arguments: [false, true])
    func cancelledOrUnmountedRefreshCannotPublishLateData(unmount: Bool) async {
        let read = SuspendedDiagnosticResult<FieldDiagnosticTimeline>()
        let value = await snapshot([observation()])
        var reads = 0
        let model = SyncDiagnosticsModel(snapshot: {
            reads += 1
            if reads == 1 { return try await read.read() }
            return value
        }, safeExport: { Data() }, writeClipboard: { _ in false })
        model.mount()
        let task = Task { await model.refresh() }
        await read.waitUntilRequested()
        if unmount { model.unmount(); model.mount() } else { task.cancel() }
        model.requestRefresh()
        await model.refresh()
        #expect(reads == 1 && model.refreshInFlight)
        read.finish(value)
        await task.value
        #expect(model.timeline == nil && !model.isLoading && !model.snapshotUnavailable && !model.refreshInFlight)
    }

    @Test func replacingCallbackOwnerAtMountBoundaryRejectsOldWorkAndUsesOnlyNewCallbacks() async {
        let oldRead = SuspendedDiagnosticResult<FieldDiagnosticTimeline>()
        let oldExport = SuspendedDiagnosticResult<Data>()
        let oldValue = await snapshot([observation(count: 1)])
        let newValue = await snapshot([observation(count: 2)])
        var readsA = 0, readsB = 0, exportsA = 0, exportsB = 0
        var writesA: [String] = [], writesB: [String] = []
        let ownerA = SyncDiagnosticsModel(snapshot: {
            readsA += 1; return try await oldRead.read()
        }, safeExport: {
            exportsA += 1; return try await oldExport.read()
        }, writeClipboard: { writesA.append($0); return true })
        ownerA.mount()
        let readA = Task { await ownerA.refresh() }
        await oldRead.waitUntilRequested()
        ownerA.requestCopy()
        let copyA = Task { await ownerA.copy() }
        await oldExport.waitUntilRequested()
        // The model lifecycle at the caller's `.id(ownerID)` boundary: A is
        // removed and B gets a new model with B's immutable callbacks.
        ownerA.unmount()
        let ownerB = SyncDiagnosticsModel(snapshot: {
            readsB += 1; return newValue
        }, safeExport: {
            exportsB += 1; return Data("owner B safe report".utf8)
        }, writeClipboard: { writesB.append($0); return true })
        ownerB.mount()
        await ownerB.refresh()
        ownerB.requestCopy()
        await ownerB.copy()
        oldRead.finish(oldValue)
        oldExport.finish(Data("owner A stale report".utf8))
        await readA.value; await copyA.value
        // Calls made while A is unmounted cannot invoke any of its callbacks.
        ownerA.requestRefresh(); await ownerA.refresh()
        ownerA.requestCopy(); await ownerA.copy()
        #expect(readsA == 1 && exportsA == 1 && writesA.isEmpty)
        #expect(ownerA.timeline == nil && ownerA.copyFeedback == .idle)
        #expect(readsB == 1 && exportsB == 1 && writesB == ["owner B safe report"])
        #expect(ownerB.timeline?.events == newValue.events && ownerB.copyFeedback == .copied)
    }

    @Test(arguments: [false, true])
    func safeWholeAppExportIsCopiedUnchangedAndClipboardRefusalIsVisible(accepts: Bool) async throws {
        let value = await snapshot([observation(component: .review)])
        let bytes = Data("{\"safeWholeAppReport\":\"review and cloud\"}".utf8)
        var writes: [String] = []
        let model = SyncDiagnosticsModel(snapshot: { value }, safeExport: { bytes }, writeClipboard: {
            writes.append($0)
            return accepts
        })
        model.mount()
        await model.refresh()
        #expect(SyncDiagnosticsPresentation(try #require(model.timeline), locale: locale).rows.isEmpty)
        model.requestCopy()
        await model.copy()
        #expect(writes == [String(decoding: bytes, as: UTF8.self)])
        #expect(model.copyFeedback == (accepts ? .copied : .unavailable))
        #expect(model.copyFeedback.text(locale: locale) == (accepts ? "Debug info copied." : "Debug info could not be copied. Try Copy Debug Info again."))
        #expect(!model.isCopying)
    }

    @Test func clipboardCallbackCannotPublishFeedbackAfterUnmounting() async {
        let value = await snapshot()
        var model: SyncDiagnosticsModel?
        model = SyncDiagnosticsModel(snapshot: { value }, safeExport: { Data("safe report".utf8) }, writeClipboard: { _ in
            model?.unmount()
            return true
        })
        guard let current = model else { return }
        current.mount(); current.requestCopy()
        await current.copy()
        #expect(current.copyFeedback == .idle && !current.isCopying)
        // Release the test's intentionally reentrant closure capture.
        model = nil
    }

    @Test(arguments: [Data([0xFF]), Data(repeating: 65, count: FieldDiagnosticsStore.byteLimit + 1), Data()])
    func invalidExportsNeverReachTheClipboard(bytes: Data) async {
        let value = await snapshot()
        let model = SyncDiagnosticsModel(snapshot: { value }, safeExport: { bytes }, writeClipboard: { _ in
            Issue.record("Invalid export reached clipboard")
            return true
        })
        model.mount(); model.requestCopy()
        await model.copy()
        #expect(model.copyFeedback == .unavailable && !model.isCopying)
    }

    @Test func byteCapIsInclusiveForValidUTF8() async {
        let value = await snapshot()
        let bytes = Data(repeating: 65, count: FieldDiagnosticsStore.byteLimit)
        var writtenCount = 0
        let model = SyncDiagnosticsModel(snapshot: { value }, safeExport: { bytes }, writeClipboard: {
            writtenCount = $0.utf8.count
            return true
        })
        model.mount(); model.requestCopy()
        await model.copy()
        #expect(writtenCount == FieldDiagnosticsStore.byteLimit && model.copyFeedback == .copied)
    }

    @Test func exportFailureIsFixedAndNeverDescribesOrBridgesErrors() async {
        let value = await snapshot()
        let model = SyncDiagnosticsModel(snapshot: { value }, safeExport: { throw UndescribableDiagnosticError() }, writeClipboard: { _ in
            Issue.record("Failed export reached clipboard")
            return true
        })
        model.mount(); model.requestCopy()
        await model.copy()
        #expect(model.copyFeedback == .unavailable)
        #expect(model.copyFeedback.text(locale: locale) == "Debug info could not be copied. Try Copy Debug Info again.")
    }

    @Test(arguments: ["cancel", "unmount", "refresh", "newCopy"])
    func staleOrCancelledCopyNeverWritesOrReplacesCurrentFeedback(reason: String) async {
        let pending = SuspendedDiagnosticResult<Data>()
        let value = await snapshot()
        var exports = 0
        var writes: [String] = []
        let model = SyncDiagnosticsModel(snapshot: { value }, safeExport: {
            exports += 1
            if exports == 1 { return try await pending.read() }
            return Data("current safe report".utf8)
        }, writeClipboard: { writes.append($0); return true })
        model.mount(); model.requestCopy()
        let first = Task { await model.copy() }
        await pending.waitUntilRequested()
        switch reason {
        case "cancel": first.cancel()
        case "unmount": model.unmount(); model.mount()
        case "refresh": model.requestRefresh(); await model.refresh()
        default:
            // Refresh invalidates the old export. Even a replacement Copy
            // must wait until its noncooperative producer has returned.
            model.requestRefresh(); await model.refresh()
            for _ in 0..<20 { model.requestCopy(); await model.copy() }
            #expect(exports == 1 && model.copyInFlight)
        }
        pending.finish(Data("stale safe report".utf8))
        await first.value
        #expect(writes.isEmpty && model.copyFeedback == .idle)
        if reason == "newCopy" { model.requestCopy(); await model.copy() }
        #expect(writes == (reason == "newCopy" ? ["current safe report"] : []))
        #expect(model.copyFeedback == (reason == "newCopy" ? .copied : .idle))
        #expect(!model.isCopying)
    }
}

@MainActor private final class DiagnosticRefreshFailureFixture {
    var fails = false
}

/// Deliberately ignores cancellation so ownership is tested independently of a
/// cooperative producer. Tests resume every continuation without timer polling.
@MainActor private final class SuspendedDiagnosticResult<Value: Sendable> {
    private var requested = false
    private var arrival: CheckedContinuation<Void, Never>?
    private var result: CheckedContinuation<Value, any Error>?

    func read() async throws -> Value {
        try await withCheckedThrowingContinuation { continuation in
            result = continuation
            requested = true
            arrival?.resume(); arrival = nil
        }
    }
    func waitUntilRequested() async {
        if requested { return }
        await withCheckedContinuation { arrival = $0 }
    }
    func finish(_ value: Value) {
        precondition(result != nil)
        result?.resume(returning: value); result = nil
    }
}

private nonisolated struct UndescribableDiagnosticError: Error, LocalizedError, CustomStringConvertible, CustomDebugStringConvertible, CustomNSError {
    static var errorDomain: String { fatalError("Must not bridge diagnostic errors") }
    var errorCode: Int { fatalError("Must not bridge diagnostic errors") }
    var errorUserInfo: [String: Any] { fatalError("Must not inspect diagnostic error metadata") }
    var errorDescription: String? { fatalError("Must not describe diagnostic errors") }
    var description: String { fatalError("Must not describe diagnostic errors") }
    var debugDescription: String { fatalError("Must not debug-describe diagnostic errors") }
}
