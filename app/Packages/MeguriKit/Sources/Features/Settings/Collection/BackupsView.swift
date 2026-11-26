import AppModel
import CoreBridge
import DesignSystem
import SwiftUI
#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// Device operations always use the native maintenance bridge, offline too.
public struct BackupsView: View {
    let collection: ReviewCollection
    let host: AppNavigation.SheetHost
    @Environment(\.openURL) private var openURL
    @State private var folderFailed = false
    public init(collection: ReviewCollection, host: AppNavigation.SheetHost = .main) {
        self.collection = collection; self.host = host
    }
    private var model: BackupCoordinator { collection.backups }
    public var body: some View {
        Form {
            Section {
                Button { model.create() } label: { Text("Back Up Now", bundle: .module) }
                    .disabled(!model.canCreate)
                    .accessibilityIdentifier("settings.backups.create")
                if model.work == .create { BackupProgress(model: model) }
                if let backup = model.status?.lastBackup {
                    LabeledContent {
                        Text(Date(timeIntervalSince1970: Double(backup.instantMilliseconds) / 1_000), format: .dateTime)
                    } label: { Text("Last backup", bundle: .module) }
                } else if model.status?.failures.contains(where: { $0.field == .lastBackup }) == true || (model.status == nil && !model.loading) {
                    Text("Last backup unavailable. Try refreshing Backups.", bundle: .module)
                } else if model.loading { ProgressView() }
                else { Text("No successful backup recorded", bundle: .module) }
            } footer: {
                if let receipt = model.created {
                    Text("Created \(receipt.name) (\(BackupPresentation.size(receipt.sizeBytes)))", bundle: .module)
                        .accessibilityIdentifier("settings.backups.created")
                }
                if let error = model.failure { BackupFailureText(error: error, candidateActive: model.candidateActive, uncertain: model.activationUncertain, restore: false) }
                if case .uncertain = model.backupPublication {
                    Text("The backup’s publication could not be confirmed. Check the backup folder before trying again.", bundle: .module)
                }
            }
            Section {
                Button { collection.presentBackupRestore(host: host) } label: { Text("Restore from Backup…", bundle: .module) }
                    .disabled(!model.canOpenRestore)
                    .accessibilityIdentifier("settings.backups.restore")
                Button(action: revealFolder) {
                    #if os(macOS)
                    Text("Show in Finder", bundle: .module)
                    #else
                    Text("Show in Files", bundle: .module)
                    #endif
                }
                .accessibilityIdentifier("settings.backups.folder")
                Button {
                    guard collection.navigation.sheet == nil else { return }
                    collection.collectionExport.start()
                    collection.navigation.sheetHost = host
                } label: { Text("Export Collection with Media…", bundle: .module) }
                    .disabled(!collection.collectionExport.canPresent || model.busy)
                    .accessibilityIdentifier("settings.backups.export")
                if folderFailed { Text("The backup folder could not be opened. Check file access and try again.", bundle: .module) }
            } footer: { Text("Backups don’t include media files. Export the collection to save its media too.", bundle: .module) }
            Section {
                if let limits = model.limits {
                    limit("Back up every (minutes)", field: .interval, value: limits.minimumIntervalMinutes)
                    limit("Daily backups", field: .daily, value: limits.daily)
                    limit("Weekly backups", field: .weekly, value: limits.weekly)
                    limit("Monthly backups", field: .monthly, value: limits.monthly)
                } else {
                    Text("Backup settings are unavailable until the collection is ready.", bundle: .module)
                }
            } header: { Text("Automatic Backups", bundle: .module) }
            footer: { Text("Synced settings. Interval: 5–9999 minutes. Retention: 0–9999 per period. Values from other versions stay unchanged until you edit that field.", bundle: .module) }
            if collection.sync.providerState?.selection == .meguri, collection.sync.serviceConnected,
               let url = BackupCoordinator.accountBackupsURL(endpoint: collection.sync.meguriEndpoint) {
                Section {
                    Link(destination: url) { Text("Server Backups in Browser…", bundle: .module) }
                        .accessibilityIdentifier("settings.backups.server")
                } footer: {
                    Text("Opens this account’s Collection page on the configured Meguri Sync server. Sign in in your browser. Scheduled server backups include media; restoring there replaces the server collection.", bundle: .module)
                }
            }
        }
        .formStyle(.grouped)
        .navigationTitle(Text("Backups", bundle: .module))
        .task { await model.refresh() }
    }

    private func limit(_ title: LocalizedStringKey, field: BackupCoordinator.LimitField, value: UInt32) -> some View {
        BackupLimitRow(title: title, value: value, minimum: field == .interval ? 5 : 0, enabled: model.canEditLimits) { edited in
            Task { await model.editLimit(field, value: edited) }
        }
    }
    private func revealFolder() {
        let url = collection.configuration.collectionLocation.backupsDirectory
        #if os(macOS)
        // Existing files only: core alone creates and validates the configured directory.
        NSWorkspace.shared.activateFileViewerSelecting([url])
        #else
        var components = URLComponents(url: url, resolvingAgainstBaseURL: false)
        components?.scheme = "shareddocuments"
        guard let destination = components?.url else { folderFailed = true; return }
        openURL(destination) { accepted in folderFailed = !accepted }
        #endif
    }
}

private struct BackupLimitRow: View {
    let title: LocalizedStringKey
    let value: UInt32
    let minimum: UInt32
    let enabled: Bool
    let save: (UInt32) -> Void
    @State private var draft = ""
    @State private var invalid = false
    @FocusState private var focused: Bool
    var body: some View {
        LabeledContent {
            TextField(text: $draft) { Text(title, bundle: .module) }
                .multilineTextAlignment(.trailing)
                #if os(iOS)
                .keyboardType(.numbersAndPunctuation)
                #endif
                .focused($focused)
                .onSubmit(commit)
                .onChange(of: focused) { _, active in if !active { commit() } }
                .disabled(!enabled)
        } label: { Text(title, bundle: .module) }
        .onChange(of: value, initial: true) { _, value in if !focused { draft = String(value) } }
        if invalid { Text("Enter a whole number in the allowed range.", bundle: .module).foregroundStyle(.secondary) }
    }
    private func commit() {
        guard enabled else { return }
        guard let number = UInt32(draft.trimmingCharacters(in: .whitespaces)), number >= minimum, number <= 9999 else { invalid = true; return }
        invalid = false
        if number != value { save(number) }
    }
}

public struct BackupRestoreSheet: View {
    let collection: ReviewCollection
    @Environment(\.locale) private var locale
    @State private var confirmation = false
    @State private var confirmedEntry: BackupEntry?
    public init(collection: ReviewCollection) { self.collection = collection }
    private var model: BackupCoordinator { collection.backups }
    public var body: some View {
        NavigationStack {
            VStack {
                if model.work == .restore {
                    BackupProgress(model: model)
                        .padding()
                } else if model.loading {
                    ProgressView { Text("Loading Backups…", bundle: .module) }
                } else if model.entries.isEmpty {
                    ContentUnavailableView {
                        Label { Text("No Backups Yet", bundle: .module) } icon: { Image(systemName: "clock.arrow.circlepath") }
                    } description: { Text("Create a backup on this device, or check the configured backup folder.", bundle: .module) }
                    actions: { Button { Task { await model.refresh() } } label: { Text("Try Again", bundle: .module) } }
                } else {
                    List(selection: Binding(get: { model.selectedName }, set: { name in
                        guard let name, model.entries.contains(where: { $0.name == name && $0.kind != .serverCopyUnverified }),
                              !model.rejectedNames.contains(name) else { return }
                        model.selectedName = name
                    })) {
                        ForEach(model.entries, id: \.name) { entry in
                            VStack(alignment: .leading) {
                                Text(verbatim: BackupPresentation.timestamp(entry, locale: locale))
                                Text(verbatim: BackupPresentation.size(entry.sizeBytes)).foregroundStyle(.secondary)
                                Text(verbatim: entry.name).font(.caption).foregroundStyle(.secondary)
                                if entry.kind == .serverCopy { Text("Server copy", bundle: .module) }
                                if unavailable(entry) { Text("Can’t be restored", bundle: .module) }
                            }
                            .tag(entry.name)
                            .selectionDisabled(unavailable(entry))
                            .foregroundStyle(unavailable(entry) ? .secondary : .primary)
                            .accessibilityElement(children: .combine)
                            .accessibilityIdentifier("settings.backups.entry.\(entry.name)")
                        }
                    }
                    .accessibilityIdentifier("settings.backups.list")
                }
                if let error = model.failure {
                    BackupFailureText(error: error, candidateActive: model.candidateActive, uncertain: model.activationUncertain, restore: true)
                        .padding()
                        .accessibilityIdentifier("settings.backups.failure")
                }
                if let safety = model.safetyBackup {
                    Text("Safety copy: \(safety)", bundle: .module).font(.caption)
                }
                if case .uncertain = model.backupPublication {
                    Text("The safety copy’s publication could not be confirmed. Keep recovery files and reopen Meguri before continuing.", bundle: .module)
                        .padding()
                }
            }
            .navigationTitle(Text("Restore from Backup", bundle: .module))
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button {
                        if model.busy { model.cancel() }
                        else { collection.navigation.dismissSheet() }
                    } label: { Text("Cancel", bundle: .module) }
                        .keyboardShortcut(.cancelAction)
                        .disabled(model.busy && !model.canCancel)
                        .accessibilityIdentifier("settings.backups.cancel")
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button {
                        confirmedEntry = model.selectedEntry; confirmation = confirmedEntry != nil
                    } label: { Text("Restore…", bundle: .module) }
                        .disabled(!model.canOpenRestore || model.loading || model.selectedEntry == nil)
                        .accessibilityIdentifier("settings.backups.apply")
                }
            }
            .confirmationDialog(confirmationTitle, isPresented: $confirmation, titleVisibility: .visible) {
                Button(role: .destructive) {
                    if let entry = confirmedEntry { model.restoreConfirmed(name: entry.name) }
                } label: { Text("Restore", bundle: .module) }
                Button(role: .cancel) {} label: { Text("Cancel", bundle: .module) }
            } message: { Text(confirmationMessage, bundle: .module) }
        }
        .interactiveDismissDisabled(model.busy || confirmation)
        .task { await model.openedRestore() }
    }
    private func unavailable(_ entry: BackupEntry) -> Bool {
        entry.kind == .serverCopyUnverified || model.rejectedNames.contains(entry.name)
    }
    private var confirmationTitle: Text {
        let date = confirmedEntry.map { BackupPresentation.timestamp($0, locale: locale) } ?? ""
        return Text("Replace the current collection with the backup from \(date)?", bundle: .module)
    }
    private var confirmationMessage: LocalizedStringKey {
        switch collection.sync.providerState?.selection {
        case .iCloud:
            "The current collection is backed up first. The restored collection stays on this device until you choose what to do with iCloud."
        case .meguri:
            "The current collection is backed up first. Restoring does not upload this collection to Meguri Sync. Review the collection choice in Sync before publishing."
        default:
            "The current collection is backed up first. This replaces the collection on this device. Media files are kept."
        }
    }
}

private struct BackupProgress: View {
    let model: BackupCoordinator
    var body: some View {
        ProgressView {
            Text(model.work == .restore ? "Restoring Backup…" : "Creating Backup…", bundle: .module)
        }
        if model.canCancel {
            Button { model.cancel() } label: { Text("Cancel", bundle: .module) }
                .accessibilityIdentifier("settings.backups.cancelOperation")
        }
    }
}

private enum BackupPresentation {
    static func size(_ bytes: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(clamping: bytes), countStyle: .file)
    }
    static func timestamp(_ entry: BackupEntry, locale: Locale) -> String {
        // A filename contains local wall time, not a UTC instant. Format its
        // components without applying today's zone or normalizing a DST gap.
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = .gmt
        let wall = entry.wallTime
        let date = calendar.date(from: DateComponents(year: Int(wall.year), month: Int(wall.month), day: Int(wall.day),
            hour: Int(wall.hour), minute: Int(wall.minute), second: Int(wall.second)))
        guard let date else { return entry.name }
        let formatter = DateFormatter()
        formatter.locale = locale; formatter.calendar = calendar; formatter.timeZone = .gmt
        formatter.dateStyle = .medium; formatter.timeStyle = .medium
        return formatter.string(from: date)
    }
}

private struct BackupFailureText: View {
    let error: MaintenanceError
    let candidateActive: Bool
    let uncertain: Bool
    let restore: Bool
    var body: some View {
        VStack(alignment: .leading) {
            Text(cause, bundle: .module)
            if case .busy(let operation) = error {
                Text("Running operation: \(operation)", bundle: .module)
            }
            if restore, uncertain {
                Text("Meguri could not determine which collection is active. Quit and reopen Meguri before continuing. Recovery files have been kept.", bundle: .module)
            } else if restore {
                Text(candidateActive
                    ? "The selected backup is now active, but recovery did not finish. Check Sync and reopen the collection before continuing."
                    : "The backup was not restored. Try another backup or resolve the problem and try again.", bundle: .module)
            }
        }
        .foregroundStyle(.secondary)
    }
    private var cause: LocalizedStringKey {
        switch error {
        case .cancelled: "The operation was cancelled."
        case .corrupt, .invalidIDs: "This backup is damaged. Choose another backup."
        case .tooNew: "This backup needs a newer version of Meguri."
        case .schemaUnsupported, .unsupportedFormat: "This backup uses an unsupported format. Choose another backup."
        case .safetyBackupFailed: "The current collection could not be backed up, so it was not replaced. Free space or check file access, then try again."
        case .sessionActive: "Finish the active review before restoring a backup."
        case .busy: "Another collection operation is running. Wait for it to finish and try again."
        case .insufficientSpace, .diskFull: "There isn’t enough free space. Free space on this device and try again."
        case .collectionCorrupt: "The collection is damaged. Restore a known-good backup."
        case .collectionUnavailable: "The collection is unavailable. Wait for the current operation, or reopen Meguri and try again."
        case .destinationExists: "A backup with this name already exists. Try again shortly."
        case .invalidArgument: "This backup is no longer available. Refresh the list and choose another."
        case .io: "The backup could not be read or written. Check file access and free space, then try again."
        }
    }
}
