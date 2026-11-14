import Foundation

public typealias FieldDiagnosticRecorder = @MainActor (FieldDiagnosticObservation) -> Void

/// Retained with launch configuration, above removable diagnostics consumers.
/// Cancellation belongs to the consumer; admission belongs to the producer and
/// is released only when that callback actually returns.
@MainActor public final class FieldDiagnosticsCallbacks {
    public enum Unavailable: Error { case busy }
    private let readSnapshot: @MainActor () async throws -> FieldDiagnosticTimeline
    private let readExport: @MainActor () async throws -> Data
    public let writeClipboard: @MainActor (String) -> Bool
    private var snapshotInFlight = false
    private var exportInFlight = false

    public init(snapshot: @escaping @MainActor () async throws -> FieldDiagnosticTimeline,
                safeExport: @escaping @MainActor () async throws -> Data,
                writeClipboard: @escaping @MainActor (String) -> Bool) {
        readSnapshot = snapshot; readExport = safeExport; self.writeClipboard = writeClipboard
    }

    public func snapshot() async throws -> FieldDiagnosticTimeline {
        guard !snapshotInFlight else { throw Unavailable.busy }
        snapshotInFlight = true
        defer { snapshotInFlight = false }
        return try await readSnapshot()
    }

    public func safeExport() async throws -> Data {
        guard !exportInFlight else { throw Unavailable.busy }
        exportInFlight = true
        defer { exportInFlight = false }
        return try await readExport()
    }
}
