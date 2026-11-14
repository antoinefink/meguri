import AppModel
import Foundation
import Observation
import SwiftUI

/// Mount directly inside the expanded Connection details disclosure. The parent
/// must remove this content when collapsed; SwiftUI may retain disclosure bodies.
/// Callbacks supply observations and the store's safe whole-app export only.
public struct SyncDiagnosticsContent: View {
    public typealias Snapshot = @MainActor () async throws -> FieldDiagnosticTimeline
    public typealias SafeExport = @MainActor () async throws -> Data
    public typealias ClipboardWrite = @MainActor (String) -> Bool

    @State private var model: SyncDiagnosticsModel
    @Environment(\.locale) private var locale

    public init(snapshot: @escaping Snapshot, safeExport: @escaping SafeExport,
                writeClipboard: @escaping ClipboardWrite) {
        _model = State(initialValue: SyncDiagnosticsModel(snapshot: snapshot, safeExport: safeExport,
                                                        writeClipboard: writeClipboard))
    }

    public var body: some View {
        let presentation = model.timeline.map { SyncDiagnosticsPresentation($0, locale: locale) }
        VStack(alignment: .leading) {
            Text(SyncDiagnosticsCopy.text("Sync Logs", locale: locale))
                .accessibilityAddTraits(.isHeader)
            Text(SyncDiagnosticsCopy.text("Up to 20 recent cloud and media observations, in recorded arrival order. Observation times use the wall clock and may move backward.", locale: locale))
            Text(SyncDiagnosticsCopy.text("These app observations are not service receipts or proof that collections match.", locale: locale))
            if model.isLoading {
                Text(SyncDiagnosticsCopy.text("Loading sync logs…", locale: locale))
            }
            if model.snapshotUnavailable {
                Text(SyncDiagnosticsCopy.text(model.timeline == nil
                    ? "Sync logs are unavailable. Try Refresh."
                    : "Refresh failed. Previously loaded log information remains below.", locale: locale))
                    .accessibilityIdentifier("settings.diagnostics.loadFeedback")
            }
            if let presentation {
                ForEach(Array(presentation.notices.enumerated()), id: \.offset) { _, notice in
                    Text(notice)
                }
                ForEach(presentation.rows) { row in
                    VStack(alignment: .leading) {
                        Text(row.summary)
                        Text(row.result)
                        Text(row.observedTime)
                        Text(row.count)
                        Text(row.duration)
                        if let failure = row.failure { Text(failure) }
                    }
                    .accessibilityElement(children: .combine)
                    .accessibilityIdentifier("settings.diagnostics.observation.\(row.id)")
                }
            }
            Button(SyncDiagnosticsCopy.text("Refresh", locale: locale)) { model.requestRefresh() }
                .accessibilityIdentifier("settings.diagnostics.refresh")
            Button(SyncDiagnosticsCopy.text("Copy Debug Info", locale: locale)) { model.requestCopy() }
                .disabled(model.isCopying)
                .accessibilityValue(Text(model.copyFeedback.text(locale: locale) ?? ""))
                .accessibilityIdentifier("settings.diagnostics.copyDebugInfo")
            if let feedback = model.copyFeedback.text(locale: locale) {
                Text(feedback)
                    .accessibilityIdentifier("settings.diagnostics.copyFeedback")
            }
        }
        .onDisappear { model.unmount() }
        .task(id: model.refreshRequest) {
            guard !Task.isCancelled else { return }
            model.mount()
            await model.refresh()
        }
        .task(id: model.copyRequest) {
            if model.copyRequest != nil { await model.copy() }
        }
    }
}

/// No tasks or storage are owned here. SwiftUI owns the two cancellable action
/// tasks; identities also reject callbacks that ignore cancellation.
@MainActor @Observable final class SyncDiagnosticsModel {
    enum CopyFeedback: Equatable {
        case idle, copying, copied, unavailable
        func text(locale: Locale) -> String? {
            switch self {
            case .idle: nil
            case .copying: SyncDiagnosticsCopy.text("Preparing debug info…", locale: locale)
            case .copied: SyncDiagnosticsCopy.text("Debug info copied.", locale: locale)
            case .unavailable: SyncDiagnosticsCopy.text("Debug info could not be copied. Try Copy Debug Info again.", locale: locale)
            }
        }
    }

    private let snapshot: SyncDiagnosticsContent.Snapshot
    private let safeExport: SyncDiagnosticsContent.SafeExport
    private let writeClipboard: SyncDiagnosticsContent.ClipboardWrite
    private var lifetime: UUID?
    private var refreshOwner: UUID?
    private var copyOwner: UUID?
    private(set) var refreshRequest = UUID()
    private(set) var copyRequest: UUID?
    private(set) var timeline: FieldDiagnosticTimeline?
    private(set) var isLoading = false
    private(set) var isCopying = false
    private(set) var snapshotUnavailable = false
    private(set) var copyFeedback = CopyFeedback.idle

    init(snapshot: @escaping SyncDiagnosticsContent.Snapshot,
         safeExport: @escaping SyncDiagnosticsContent.SafeExport,
         writeClipboard: @escaping SyncDiagnosticsContent.ClipboardWrite) {
        self.snapshot = snapshot; self.safeExport = safeExport; self.writeClipboard = writeClipboard
    }

    func mount() {
        guard lifetime == nil else { return }
        lifetime = UUID()
    }

    func unmount() {
        lifetime = nil; refreshOwner = nil
        invalidateCopy()
        isLoading = false
    }

    func requestRefresh() {
        guard lifetime != nil else { return }
        refreshOwner = nil
        invalidateCopy()
        refreshRequest = UUID()
    }

    func requestCopy() {
        guard lifetime != nil else { return }
        copyOwner = nil
        copyRequest = UUID()
    }

    private func invalidateCopy() {
        copyOwner = nil; copyRequest = nil
        isCopying = false; copyFeedback = .idle
    }

    func refresh() async {
        guard let lifetime, !Task.isCancelled else { return }
        let owner = UUID()
        refreshOwner = owner
        invalidateCopy()
        isLoading = true
        defer {
            if self.lifetime == lifetime, refreshOwner == owner { isLoading = false }
        }
        do {
            let value = try await snapshot()
            guard self.lifetime == lifetime, refreshOwner == owner, !Task.isCancelled else { return }
            timeline = value
            snapshotUnavailable = false
        } catch {
            guard self.lifetime == lifetime, refreshOwner == owner, !Task.isCancelled else { return }
            snapshotUnavailable = true
        }
    }

    func copy() async {
        guard let lifetime, copyRequest != nil, !Task.isCancelled else { return }
        let owner = UUID()
        copyOwner = owner
        isCopying = true; copyFeedback = .copying
        defer {
            if self.lifetime == lifetime, copyOwner == owner {
                isCopying = false
                if Task.isCancelled { copyFeedback = .idle }
            }
        }
        do {
            let bytes = try await safeExport()
            guard self.lifetime == lifetime, copyOwner == owner, !Task.isCancelled else { return }
            guard !bytes.isEmpty, bytes.count <= FieldDiagnosticsStore.byteLimit,
                  let text = String(data: bytes, encoding: .utf8) else {
                copyFeedback = .unavailable
                return
            }
            // No suspension between the ownership check and the injected write.
            let copied = writeClipboard(text)
            guard self.lifetime == lifetime, copyOwner == owner, !Task.isCancelled else { return }
            copyFeedback = copied ? .copied : .unavailable
        } catch {
            guard self.lifetime == lifetime, copyOwner == owner, !Task.isCancelled else { return }
            copyFeedback = .unavailable
        }
    }
}

/// Presentation only: retains the store's FIFO order and never interprets an
/// observation as a receipt, current sync status, or a historical last-sync fact.
struct SyncDiagnosticsPresentation {
    struct Row: Identifiable {
        let id: Int
        let summary: String
        let result: String
        let observedTime: String
        let count: String
        let duration: String
        let failure: String?
    }
    let rows: [Row]
    let notices: [String]

    init(_ timeline: FieldDiagnosticTimeline, locale: Locale) {
        func text(_ key: String.LocalizationValue) -> String { SyncDiagnosticsCopy.text(key, locale: locale) }
        guard timeline.schemaVersion == 1 else {
            rows = []
            notices = [text("This log format is unavailable in this app version.")]
            return
        }
        let events = timeline.events.enumerated().filter {
            $0.element.observation.component == .cloud || $0.element.observation.component == .media
        }
        rows = events.suffix(20).map { index, event in
            let observation = event.observation
            let component = text(observation.component == .cloud ? "Cloud" : "Media")
            let operation = text(SyncDiagnosticsCopy.operation(observation.operation))
            let stage = text(SyncDiagnosticsCopy.stage(observation.stage))
            let outcome = text(SyncDiagnosticsCopy.outcome(observation.outcome))
            let time: String
            if (-62_135_596_800_000...253_402_300_799_999).contains(event.observedAtUnixMilliseconds) {
                let formatter = ISO8601DateFormatter()
                formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
                formatter.timeZone = TimeZone(secondsFromGMT: 0)
                let instant = formatter.string(from: Date(timeIntervalSince1970: Double(event.observedAtUnixMilliseconds) / 1_000))
                time = text("Observed at \(instant)")
            } else {
                time = text("Observation time: Not recorded")
            }
            return Row(id: index, summary: text("\(component): \(operation)"),
                       result: text("\(stage) · \(outcome)"), observedTime: time,
                       count: observation.count.map { text("Recorded count: \($0)") } ?? text("Recorded count: Not recorded"),
                       duration: observation.durationMilliseconds.map { text("Recorded duration: \($0) ms") } ?? text("Recorded duration: Not recorded"),
                       failure: observation.failure.map { text(SyncDiagnosticsCopy.failure($0)) })
        }
        var notes: [String] = []
        if timeline.storage != .available { notes.append(text(SyncDiagnosticsCopy.storage(timeline.storage))) }
        if timeline.priorHistoryUnavailable || timeline.priorHistoryUnavailableReason != nil {
            notes.append(text("Some earlier history is unavailable. The number of missing observations is unknown."))
            if let reason = timeline.priorHistoryUnavailableReason, reason != .available, reason != timeline.storage {
                notes.append(text(SyncDiagnosticsCopy.storage(reason)))
            }
        }
        if timeline.droppedCount > 0 { notes.append(text("Known dropped observations, across the app: \(timeline.droppedCount)")) }
        if timeline.expiredCount > 0 { notes.append(text("Expired observations, across the app: \(timeline.expiredCount)")) }
        if timeline.truncatedCount > 0 { notes.append(text("Older observations removed, across the app: \(timeline.truncatedCount)")) }
        if timeline.invalidCount > 0 { notes.append(text("Invalid observations omitted, across the app: \(timeline.invalidCount)")) }
        if events.count > rows.count { notes.append(text("Only the latest 20 cloud and media observations are shown.")) }
        if rows.isEmpty { notes.append(text("No recent cloud or media observations are available.")) }
        notices = notes
    }
}

private enum SyncDiagnosticsCopy {
    static func text(_ key: String.LocalizationValue, locale: Locale) -> String {
        String(localized: key, table: "Diagnostics", bundle: .module, locale: locale)
    }

    static func operation(_ value: FieldDiagnosticObservation.Operation) -> String.LocalizationValue {
        switch value {
        case .open: "Open"
        case .start: "Start"
        case .answer: "Answer"
        case .save: "Save"
        case .inspect: "Inspect"
        case .activate: "Activate"
        case .recover: "Recover"
        case .prepare: "Prepare"
        case .publish: "Publish"
        case .cleanup: "Clean up"
        case .status: "Status"
        case .exchange: "Exchange"
        case .transfer: "Transfer"
        }
    }

    static func stage(_ value: FieldDiagnosticObservation.Stage) -> String.LocalizationValue {
        switch value {
        case .requested: "Request observed"
        case .admission: "Admission observed"
        case .coreAccepted: "Core acceptance observed"
        case .persistence: "Persistence observed"
        case .presentation: "Presentation observed"
        case .inspection: "Inspection observed"
        case .activation: "Activation observed"
        case .preparation: "Preparation observed"
        case .publication: "Publication observed"
        case .cleanup: "Cleanup observed"
        case .statusObserved: "Status observed"
        case .exchangeCompleted: "Exchange completion observed"
        case .convergenceObserved: "Convergence reported"
        }
    }

    static func outcome(_ value: FieldDiagnosticObservation.Outcome) -> String.LocalizationValue {
        switch value {
        case .observed: "Observed result"
        case .succeeded: "Reported success for this stage"
        case .failed: "Failed"
        case .waiting: "Pending"
        case .cancelled: "Cancelled"
        case .refused: "Refused"
        case .stale: "Stale observation"
        case .incomplete: "Incomplete"
        case .unknown: "Outcome not recorded"
        case .cleanupPending: "Cleanup pending"
        }
    }

    static func failure(_ value: FieldDiagnosticObservation.Failure) -> String.LocalizationValue {
        switch value {
        case .other: "Failure reason: Not recorded"
        case .storageUnavailable: "Failure reason: Local storage unavailable"
        case .unavailable: "Failure reason: Unavailable"
        case .invalidState: "Failure reason: Invalid state"
        case .queueRejected: "Failure reason: Queue rejected the request"
        case .transport: "Failure reason: Transport error"
        case .noSpace: "Failure reason: Not enough storage space"
        case .permission: "Failure reason: Permission denied"
        case .notAuthenticated: "Failure reason: Authentication required"
        case .rateLimited: "Failure reason: Rate limited"
        case .serviceUnavailable: "Failure reason: Service unavailable"
        }
    }

    static func storage(_ value: FieldDiagnosticTimeline.Storage) -> String.LocalizationValue {
        switch value {
        case .available: "Saved history is available."
        case .unavailable: "Saved history is unavailable. Available observations are shown."
        case .corrupt: "Saved history could not be read. Available observations are shown."
        case .oversized: "Saved history exceeded the size limit. Available observations are shown."
        case .unsupportedSchema: "Saved history uses an unsupported format. Available observations are shown."
        }
    }
}
