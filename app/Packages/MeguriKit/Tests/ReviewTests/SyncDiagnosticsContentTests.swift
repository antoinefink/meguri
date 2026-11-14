import AppModel
import Foundation
import Synchronization
import Testing
@testable import Settings

@MainActor @Suite struct SyncDiagnosticsContentTests {
    private let locale = Locale(identifier: "en")
    private let instant = Date(timeIntervalSince1970: 1_800_000_000)

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
        #expect(presentation.rows.map(\.count) == (10..<30).map { "Recorded count: \($0)" })
        #expect(presentation.rows.first?.summary == "Cloud: Exchange")
        #expect(presentation.rows.last?.summary == "Media: Exchange")
        #expect(presentation.rows.allSatisfy { $0.failure == nil })
        #expect(presentation.notices.contains("Only the latest 20 cloud and media observations are shown."))
        #expect(!presentation.rows.map(\.count).joined().contains("9999"))
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
            "Request observed · Observed result", "Exchange completion observed · Incomplete",
            "Status observed · Outcome not recorded", "Publication observed · Pending",
            "Admission observed · Refused", "Cleanup observed · Cancelled",
            "Publication observed · Failed", "Persistence observed · Reported success for this stage",
        ])
        #expect(rows[0].count == "Recorded count: Not recorded")
        #expect(rows[0].duration == "Recorded duration: Not recorded")
        #expect(rows[1].count == "Recorded count: 0")
        #expect(rows[6].duration == "Recorded duration: 42 ms")
        #expect(rows[6].failure == "Failure reason: Permission denied")
        #expect(!rows[6].count.contains("Not recorded"))
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
        let rows = SyncDiagnosticsPresentation(await store.timeline(), locale: locale).rows
        #expect(rows.map(\.count) == ["Recorded count: 1", "Recorded count: 2"])
        #expect(rows.map(\.observedTime) == ["Observed at 2027-01-15T08:00:00.000Z", "Observed at 2027-01-15T07:59:59.000Z"])
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
        var fails = false
        let model = SyncDiagnosticsModel(snapshot: {
            if fails { throw UndescribableDiagnosticError() }
            return value
        }, safeExport: { throw UndescribableDiagnosticError() }, writeClipboard: { _ in
            Issue.record("Refresh must never write the clipboard")
            return false
        })
        // Unmounted callbacks are never invoked.
        await model.refresh()
        #expect(model.timeline == nil)
        model.mount()
        await model.refresh()
        fails = true
        model.requestRefresh()
        await model.refresh()
        #expect(model.snapshotUnavailable)
        #expect(model.timeline?.events == value.events)
        #expect(!model.isLoading)
        fails = false
        await model.refresh()
        #expect(!model.snapshotUnavailable)
    }

    @Test func newerRefreshWinsEvenWhenTheOldReadIgnoresCancellation() async {
        let old = SuspendedDiagnosticResult<FieldDiagnosticTimeline>()
        let new = SuspendedDiagnosticResult<FieldDiagnosticTimeline>()
        let oldValue = await snapshot([observation(count: 1)])
        let newValue = await snapshot([observation(count: 2)])
        var reads = 0
        let model = SyncDiagnosticsModel(snapshot: {
            reads += 1
            return try await (reads == 1 ? old : new).read()
        }, safeExport: { Data() }, writeClipboard: { _ in false })
        model.mount()
        let first = Task { await model.refresh() }
        await old.waitUntilRequested()
        model.requestRefresh()
        let second = Task { await model.refresh() }
        await new.waitUntilRequested()
        new.finish(newValue)
        await second.value
        old.finish(oldValue)
        await first.value
        #expect(model.timeline?.events == newValue.events)
        #expect(!model.isLoading && !model.snapshotUnavailable)
    }

    @Test(arguments: [false, true])
    func cancelledOrUnmountedRefreshCannotPublishLateData(unmount: Bool) async {
        let read = SuspendedDiagnosticResult<FieldDiagnosticTimeline>()
        let value = await snapshot([observation()])
        let model = SyncDiagnosticsModel(snapshot: { try await read.read() }, safeExport: { Data() }, writeClipboard: { _ in false })
        model.mount()
        let task = Task { await model.refresh() }
        await read.waitUntilRequested()
        if unmount { model.unmount(); model.mount() } else { task.cancel() }
        read.finish(value)
        await task.value
        #expect(model.timeline == nil && !model.isLoading && !model.snapshotUnavailable)
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
        default: model.requestCopy(); await model.copy()
        }
        pending.finish(Data("stale safe report".utf8))
        await first.value
        #expect(writes == (reason == "newCopy" ? ["current safe report"] : []))
        #expect(model.copyFeedback == (reason == "newCopy" ? .copied : .idle))
        #expect(!model.isCopying)
    }
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
