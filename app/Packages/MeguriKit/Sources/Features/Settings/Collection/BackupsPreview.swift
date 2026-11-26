import AppModel
import CoreBridge
import DesignSystem
import Foundation
import SwiftUI

/// Synthetic values use the same native Form, List and confirmation as production.
/// No collection, host, service, task or file is constructed by these fixtures.
public enum BackupsPreview: PreviewStates {
    public static let previewID = "settings.backups"
    public static var previewFixtures: [PreviewFixture] {
        [PreviewFixture(.empty) { restore(.empty) },
         PreviewFixture(.typical) { settings(.typical) },
         PreviewFixture(.overflow) { restore(.overflow) },
         PreviewFixture(.error) { restore(.error) },
         PreviewFixture(.loading) { settings(.loading) }]
    }

    @MainActor public static func settings(_ state: PreviewState) -> some View {
        NavigationStack {
            BackupsContent(presentation: settingsPresentation(state), actions: .init(), folderFailed: false)
        }
        // Even the production browser Link stays inside the inert fixture.
        .environment(\.openURL, OpenURLAction { _ in .handled })
    }

    @MainActor public static func restore(_ state: PreviewState, cancel: @escaping () -> Void = {}) -> some View {
        RestoreFixture(state: state, cancel: cancel)
    }

    private struct RestoreFixture: View {
        let state: PreviewState
        let cancel: () -> Void
        @State private var selected: String?
        @State private var retriedError = false
        init(state: PreviewState, cancel: @escaping () -> Void) {
            self.state = state; self.cancel = cancel
            _selected = State(initialValue: selectedName(state))
        }
        var body: some View {
            BackupRestoreContent(presentation: restorePresentation(retriedError ? .empty : state), selectedName: $selected,
                                 refresh: {
                                     #if os(iOS)
                                     if state == .error { retriedError = true }
                                     #endif
                                 }, cancel: cancel, restore: { _ in })
        }
    }

    private static let limits = BackupLimits(daily: 12, weekly: 10, monthly: 9, minimumIntervalMinutes: 30)
    private static let instant: Int64 = 1_795_627_500_000
    private static let serverURL = URL(string: "https://sync.example.invalid/collection")

    private static func settingsPresentation(_ state: PreviewState) -> BackupsPresentation {
        switch state {
        case .empty:
            .init(limits: limits)
        case .typical:
            .init(limits: limits, lastBackupMilliseconds: instant,
                  created: .init(name: "backup-2026-11-25-17.25.00.colpkg", sizeBytes: 28_400_000),
                  provider: .meguri, serverBackupsURL: serverURL)
        case .overflow:
            .init(limits: .init(daily: .max, weekly: 10, monthly: 0, minimumIntervalMinutes: .max),
                  lastBackupMilliseconds: instant, provider: .meguri, serverBackupsURL: serverURL)
        case .error:
            .init(lastBackupUnavailable: true, failure: .collectionUnavailable,
                  canCreate: false, canEditLimits: false, canExport: false)
        case .loading:
            .init(limits: limits, lastBackupMilliseconds: instant, work: .create,
                  canCreate: false, canOpenRestore: false, canEditLimits: false, canExport: false, canCancel: true)
        }
    }

    // Newest first, using the exact production filename grammar and local wall time.
    private static let entries: [BackupEntry] = [
        .init(name: "backup-2026-11-25-17.25.00.colpkg", kind: .backup,
              wallTime: .init(year: 2026, month: 11, day: 25, hour: 17, minute: 25, second: 0), sizeBytes: 28_400_000),
        .init(name: "server-copy-2026-11-24-09.10.00.colpkg", kind: .serverCopy,
              wallTime: .init(year: 2026, month: 11, day: 24, hour: 9, minute: 10, second: 0), sizeBytes: 27_900_000),
        .init(name: "server-copy-2026-11-23-09.10.00.unverified.anki2", kind: .serverCopyUnverified,
              wallTime: .init(year: 2026, month: 11, day: 23, hour: 9, minute: 10, second: 0), sizeBytes: 27_800_000),
        .init(name: "backup-2026-11-22-08.00.00.colpkg", kind: .backup,
              wallTime: .init(year: 2026, month: 11, day: 22, hour: 8, minute: 0, second: 0), sizeBytes: 27_600_000),
    ]

    private static var overflowEntries: [BackupEntry] {
        let sameSecond = BackupEntry(name: "server-copy-2026-11-25-17.25.00.colpkg", kind: .serverCopy,
            wallTime: entries[0].wallTime, sizeBytes: 28_300_000)
        let older = (1...21).reversed().map { day in
            BackupEntry(name: String(format: "backup-2026-11-%02d-08.00.00.colpkg", day), kind: .backup,
                wallTime: .init(year: 2026, month: 11, day: UInt8(day), hour: 8, minute: 0, second: 0),
                sizeBytes: 27_500_000)
        }
        return [entries[0], sameSecond] + Array(entries.dropFirst()) + older
    }

    private static func selectedName(_ state: PreviewState) -> String? {
        switch state {
        case .typical: entries[0].name
        case .overflow: entries[1].name // An older verified server copy is selected.
        default: nil
        }
    }

    private static func restorePresentation(_ state: PreviewState) -> BackupsPresentation {
        switch state {
        case .empty: .init()
        case .typical: .init(entries: entries, provider: .localOnly)
        case .overflow:
            .init(entries: overflowEntries, rejectedNames: [entries[3].name], provider: .meguri)
        case .error: .init(listFailure: .io("Synthetic unreadable backup folder"))
        case .loading: .init(loading: true)
        }
    }
}

#Preview("Empty") { BackupsPreview.preview(.empty) }
#Preview("Typical") { BackupsPreview.preview(.typical) }
#Preview("Overflow") { BackupsPreview.preview(.overflow) }
#Preview("Error") { BackupsPreview.preview(.error) }
#Preview("Loading") { BackupsPreview.preview(.loading) }
