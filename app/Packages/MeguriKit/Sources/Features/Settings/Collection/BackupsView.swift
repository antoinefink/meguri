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
        BackupsContent(presentation: .init(collection), actions: .init(
            create: { model.create() }, restore: { collection.presentBackupRestore(host: host) },
            revealFolder: revealFolder, export: {
                guard collection.navigation.sheet == nil else { return }
                collection.collectionExport.start()
                collection.navigation.sheetHost = host
            }, editLimit: { field, value in Task { await model.editLimit(field, value: value) } },
            cancel: model.cancel), folderFailed: folderFailed)
            .task { await model.refresh() }
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

/// The production grouped form; immutable render values carry no host or service.
struct BackupsContent: View {
    let presentation: BackupsPresentation
    let actions: BackupsActions
    let folderFailed: Bool
    var body: some View {
        Form {
            Section {
                Button(action: actions.create) { Text("Back Up Now", bundle: .module) }
                    .disabled(!presentation.canCreate)
                    .accessibilityIdentifier("settings.backups.create")
                if presentation.work == .create { BackupProgress(work: presentation.work, canCancel: presentation.canCancel, cancel: actions.cancel) }
                if let instant = presentation.lastBackupMilliseconds {
                    LabeledContent {
                        Text(Date(timeIntervalSince1970: Double(instant) / 1_000), format: .dateTime)
                    } label: { Text("Last backup", bundle: .module) }
                } else if presentation.lastBackupUnavailable {
                    Text("Last backup unavailable. Try refreshing Backups.", bundle: .module)
                } else if presentation.loading { ProgressView() }
                else { Text("No successful backup recorded", bundle: .module) }
            } footer: {
                if let receipt = presentation.created {
                    Text("Created \(receipt.name) (\(BackupPresentation.size(receipt.sizeBytes)))", bundle: .module)
                        .accessibilityIdentifier("settings.backups.created")
                }
                if let error = presentation.failure { BackupFailureText(error: error, candidateActive: presentation.candidateActive, uncertain: presentation.activationUncertain, restore: false) }
                if let error = presentation.listFailure { BackupFailureText(error: error, candidateActive: false, uncertain: false, restore: false) }
                if case .uncertain = presentation.backupPublication {
                    Text("The backup’s publication could not be confirmed. Check the backup folder before trying again.", bundle: .module)
                }
            }
            Section {
                Button(action: actions.restore) { Text("Restore from Backup…", bundle: .module) }
                    .disabled(!presentation.canOpenRestore)
                    .accessibilityIdentifier("settings.backups.restore")
                Button(action: actions.revealFolder) {
                    #if os(macOS)
                    Text("Show in Finder", bundle: .module)
                    #else
                    Text("Show in Files", bundle: .module)
                    #endif
                }
                .accessibilityIdentifier("settings.backups.folder")
                Button(action: actions.export) { Text("Export Collection with Media…", bundle: .module) }
                    .disabled(!presentation.canExport)
                    .accessibilityIdentifier("settings.backups.export")
                if folderFailed { Text("The backup folder could not be opened. Check file access and try again.", bundle: .module) }
            } footer: { Text("Device backups contain only the collection. Export it to save media too.", bundle: .module)
                .accessibilityIdentifier("settings.backups.mediaExplanation") }
            Section {
                if let limits = presentation.limits {
                    limit("Interval", field: .interval, value: limits.minimumIntervalMinutes)
                } else {
                    Text("Backup settings are unavailable until the collection is ready.", bundle: .module)
                }
            } header: { Text("Automatic Backups", bundle: .module) }
            footer: { Text("Synced settings. Enter 5–9999 minutes.", bundle: .module) }
            if let limits = presentation.limits {
                Section {
                    limit("Daily", field: .daily, value: limits.daily)
                    limit("Weekly", field: .weekly, value: limits.weekly)
                    limit("Monthly", field: .monthly, value: limits.monthly)
                } header: { Text("Keep Backups", bundle: .module) }
                footer: { Text("Synced settings. Enter 0–9999 backups per period.", bundle: .module) }
            }
            if let url = presentation.serverBackupsURL {
                Section {
                    Link(destination: url) { Text("Server Backups in Browser…", bundle: .module) }
                        .accessibilityIdentifier("settings.backups.server")
                } footer: {
                    Text("Sign in to your Meguri Sync account in the browser. Scheduled server backups include media. Restoring there replaces the server collection.", bundle: .module)
                        .accessibilityIdentifier("settings.backups.serverExplanation")
                }
            }
        }
        .formStyle(.grouped)
        .navigationTitle(Text("Backups", bundle: .module))
        .accessibilityIdentifier("settings.backups.content")
    }

    private func limit(_ title: LocalizedStringKey, field: BackupCoordinator.LimitField, value: UInt32) -> some View {
        BackupLimitRow(title: title, identifier: "settings.backups.limit.\(String(describing: field))", value: value, minimum: field == .interval ? 5 : 0, unit: field == .interval ? "min" : nil, enabled: presentation.canEditLimits) { edited in
            actions.editLimit(field, edited)
        }
    }
}

private struct BackupLimitRow: View {
    let title: LocalizedStringKey
    let identifier: String
    let value: UInt32
    let minimum: UInt32
    let unit: LocalizedStringKey?
    let enabled: Bool
    let save: (UInt32) -> Void
    @State private var draft = BackupLimitDraft()
    @FocusState private var focused: Bool
    @Environment(\.dynamicTypeSize) private var textSize
    var body: some View {
        LabeledContent {
            HStack {
                // Measure all ten UInt32 digits in the current font, including AX.
                // The field takes only that space instead of proposing a full row.
                ZStack(alignment: .trailing) {
                    Text(verbatim: String(UInt32.max)).padding(.horizontal, Spacing.small).hidden().accessibilityHidden(true)
                    input
                }
                .monospacedDigit()
                .fixedSize(horizontal: true, vertical: false)
                if let unit { Text(unit, bundle: .module).foregroundStyle(.secondary) }
            }
            .fixedSize(horizontal: true, vertical: false)
        } label: { Text(title, bundle: .module) }
        .labeledContentStyle(BackupLimitRowStyle(stacked: textSize.isAccessibilitySize))
        .onChange(of: value, initial: true) { _, value in if !focused { draft.reset(to: value) } }
        if draft.invalid { Text("Enter a whole number in the allowed range.", bundle: .module).foregroundStyle(.secondary) }
    }
    private var input: some View {
            TextField(text: $draft.text) { Text(title, bundle: .module) }
                .labelsHidden()
                .accessibilityLabel(minimum == 5 ? Text("Interval in minutes", bundle: .module) : Text(title, bundle: .module))
                .accessibilityIdentifier(identifier)
                .multilineTextAlignment(.trailing)
                #if os(iOS)
                .keyboardType(.numbersAndPunctuation)
                #endif
                .focused($focused)
                .onSubmit(commit)
                .onChange(of: focused) { _, active in
                    if active { draft.reset(to: value) } else { commit() }
                }
                .disabled(!enabled)
    }
    private func commit() {
        guard enabled else { return }
        if let number = draft.commit(value: value, minimum: minimum) { save(number) }
    }
}

/// Standard label/value alignment; only accessibility text sizes may reflow.
private struct BackupLimitRowStyle: LabeledContentStyle {
    let stacked: Bool
    func makeBody(configuration: Configuration) -> some View {
        if stacked {
            ViewThatFits(in: .horizontal) {
                row(configuration)
                VStack(alignment: .leading) { configuration.label; configuration.content }
            }
        } else { row(configuration) }
    }
    private func row(_ configuration: Configuration) -> some View {
        HStack { configuration.label; Spacer(); configuration.content }
    }
}

public struct BackupRestoreSheet: View {
    let collection: ReviewCollection
    public init(collection: ReviewCollection) { self.collection = collection }
    private var model: BackupCoordinator { collection.backups }
    public var body: some View {
        BackupRestoreContent(presentation: .init(collection), selectedName: Binding(
            get: { model.selectedName }, set: { model.selectedName = $0 }),
            refresh: { Task { await model.refresh() } }, cancel: {
                if model.busy { model.cancel() }
                #if os(macOS)
                collection.navigation.dismissSheet()
                #else
                if !model.busy { collection.navigation.dismissSheet() }
                #endif
            }, restore: { model.restoreConfirmed(name: $0) })
            .task { await model.openedRestore() }
    }
}

struct BackupRestoreContent: View {
    let presentation: BackupsPresentation
    @Binding var selectedName: String?
    let refresh: () -> Void
    let cancel: () -> Void
    let restore: (String) -> Void
    @Environment(\.locale) private var locale
    @State private var confirmation = false
    @State private var confirmedEntry: BackupEntry?
    @Environment(\.dynamicTypeSize) private var textSize
    private var selectedEntry: BackupEntry? {
        presentation.entries.first { $0.name == selectedName && !unavailable($0) }
    }
    private var showsListFailure: Bool {
        presentation.work != .restore && !presentation.loading && presentation.entries.isEmpty && presentation.listFailure != nil
    }
    var body: some View {
        #if os(macOS)
        restoreBody
            .frame(minWidth: BackupRestoreWindowLayout.minimumSize.width,
                   minHeight: BackupRestoreWindowLayout.minimumSize.height)
        #else
        NavigationStack { restoreBody }
            .presentationSizing(.form)
            .presentationDetents(textSize.isAccessibilitySize || presentation.entries.count > 8 ? [.large] : [.medium, .large])
            .interactiveDismissDisabled(presentation.busy || confirmation)
        #endif
    }
    private var restoreBody: some View {
            VStack {
                if presentation.work == .restore {
                    BackupProgress(work: presentation.work, canCancel: presentation.canCancel, cancel: cancel)
                        .padding()
                } else if presentation.loading {
                    ProgressView { Text("Loading Backups…", bundle: .module) }
                        .accessibilityLabel(Text("Loading Backups…", bundle: .module))
                        .accessibilityValue(Text("In progress", bundle: .module))
                        .accessibilityIdentifier("settings.backups.loading")
                } else if showsListFailure, let error = presentation.listFailure {
                    ScrollView {
                        ContentUnavailableView {
                            Label { Text("Unable to Load Backups", bundle: .module) } icon: { Image(systemName: "exclamationmark.triangle") }
                        } description: {
                            BackupFailureText(error: error, candidateActive: false, uncertain: false, restore: false)
                                .fixedSize(horizontal: false, vertical: true)
                                .accessibilityIdentifier("settings.backups.listFailure")
                        } actions: {
                            Button(action: refresh) { Text("Try Again", bundle: .module) }
                                .accessibilityIdentifier("settings.backups.retry")
                        }
                        restoreNotices
                    }
                } else if presentation.entries.isEmpty {
                    ContentUnavailableView {
                        Label { Text("No Backups Yet", bundle: .module) } icon: { Image(systemName: "clock.arrow.circlepath") }
                    } description: { Text("Create a backup on this device, or check the configured backup folder.", bundle: .module) }
                    actions: { Button(action: refresh) { Text("Try Again", bundle: .module) }
                        .accessibilityIdentifier("settings.backups.retry") }
                } else {
                    history
                }
                if !showsListFailure { restoreNotices }
            }
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("settings.backups.restoreContent")
            .navigationTitle(Text("Restore from Backup", bundle: .module))
            #if os(macOS)
            .safeAreaInset(edge: .bottom, spacing: 0) {
                HStack { Spacer(); cancelButton; restoreButton }.padding()
            }
            #else
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { cancelButton }
                ToolbarItem(placement: .confirmationAction) { restoreButton }
            }
            #endif
            .confirmationDialog(confirmationTitle, isPresented: $confirmation, titleVisibility: .visible) {
                Button(role: .destructive) {
                    if let entry = confirmedEntry { restore(entry.name) }
                } label: { Text("Restore", bundle: .module) }
                Button(role: .cancel) {} label: { Text("Cancel", bundle: .module) }
            } message: { Text(confirmationMessage, bundle: .module) }
    }
    private var cancelButton: some View {
        Button(action: cancel) { Text("Cancel", bundle: .module) }
            .keyboardShortcut(.cancelAction)
            .disabled(presentation.busy && !presentation.canCancel)
            .accessibilityIdentifier("settings.backups.cancel")
    }
    private var restoreButton: some View {
        Button {
            confirmedEntry = selectedEntry; confirmation = confirmedEntry != nil
        } label: { Text("Restore…", bundle: .module) }
            .disabled(!presentation.canOpenRestore || presentation.loading || selectedEntry == nil)
            .accessibilityIdentifier("settings.backups.apply")
    }
    private var selection: Binding<String?> {
        Binding(get: { selectedName }, set: { name in
            guard let name, presentation.entries.contains(where: { $0.name == name && !unavailable($0) }) else { return }
            selectedName = name
        })
    }
    @ViewBuilder private var history: some View {
        #if os(macOS)
        Table(of: BackupHistoryRow.self, selection: selection) {
            TableColumn(Text("Backup Date", bundle: .module)) { row in
                entryLabel(row.entry)
                    .accessibilityElement(children: .combine)
                    .accessibilityLabel(Text(verbatim: entryAccessibilityLabel(row.entry)))
                    .accessibilityAddTraits(selectedName == row.entry.name ? .isSelected : [])
                    .accessibilityValue(Text(selectedName == row.entry.name ? "Selected" : "Not selected", bundle: .module))
                    .accessibilityIdentifier("settings.backups.entry.\(row.entry.name)")
            }
            TableColumn(Text("Size", bundle: .module)) { row in
                Text(verbatim: BackupPresentation.size(row.entry.sizeBytes))
                    .foregroundStyle(unavailable(row.entry) ? .secondary : .primary)
            }
            .width(min: 70, ideal: 90)
        } rows: {
            ForEach(presentation.entries.map(BackupHistoryRow.init)) { row in
                TableRow(row).selectionDisabled(unavailable(row.entry))
            }
        }
        .contextMenu(forSelectionType: String.self) { names in
            if let name = names.first { Text(verbatim: name) }
        }
        #else
        List(selection: selection) {
            ForEach(presentation.entries, id: \.name) { entry in
                HStack(alignment: .firstTextBaseline) {
                    entryLabel(entry)
                    Spacer()
                    Text(verbatim: BackupPresentation.size(entry.sizeBytes)).foregroundStyle(.secondary)
                }
                .tag(entry.name)
                .selectionDisabled(unavailable(entry))
                .accessibilityElement(children: .combine)
                .accessibilityIdentifier("settings.backups.entry.\(entry.name)")
                .accessibilityLabel(Text(verbatim: entryAccessibilityLabel(entry)))
                .accessibilityAddTraits(selectedName == entry.name ? .isSelected : [])
                .accessibilityValue(Text(selectedName == entry.name ? "Selected" : "Not selected", bundle: .module))
            }
        }
        #endif
    }
    private func entryLabel(_ entry: BackupEntry) -> some View {
        VStack(alignment: .leading) {
            Text(verbatim: BackupPresentation.timestamp(entry, locale: locale))
            // Rare identical timestamps need a visible discriminator, not a
            // repeated filename on every archive in the history.
            if let index = duplicateIndex(entry) {
                Text("Archive \(index)", bundle: .module).foregroundStyle(.secondary)
            }
            if entry.kind == .serverCopy { Text("Server copy", bundle: .module) }
            if unavailable(entry) { Text("Can’t be restored", bundle: .module) }
        }
        .foregroundStyle(unavailable(entry) ? .secondary : .primary)
        .help(entry.name)
    }
    private func duplicateIndex(_ entry: BackupEntry) -> Int? {
        let sameTime = presentation.entries.filter { $0.wallTime == entry.wallTime }
        guard sameTime.count > 1, let index = sameTime.firstIndex(where: { $0.name == entry.name }) else { return nil }
        return index + 1
    }
    private func entryAccessibilityLabel(_ entry: BackupEntry) -> String {
        var parts = [BackupPresentation.timestamp(entry, locale: locale), BackupPresentation.size(entry.sizeBytes), entry.name]
        if entry.kind == .serverCopy { parts.append(String(localized: "Server copy", bundle: .module)) }
        if unavailable(entry) { parts.append(String(localized: "Can’t be restored", bundle: .module)) }
        return parts.joined(separator: ", ")
    }
    @ViewBuilder private var restoreNotices: some View {
        if !showsListFailure, let error = presentation.listFailure {
            BackupFailureText(error: error, candidateActive: false, uncertain: false, restore: false)
                .padding()
                .accessibilityIdentifier("settings.backups.listFailure")
        }
        if let error = presentation.failure {
            BackupFailureText(error: error, candidateActive: presentation.candidateActive, uncertain: presentation.activationUncertain,
                              restore: presentation.failureWork == .restore)
                .padding()
                .accessibilityIdentifier("settings.backups.failure")
        }
        if let safety = presentation.safetyBackup {
            Text("Safety copy: \(safety)", bundle: .module).font(.caption)
                .accessibilityIdentifier("settings.backups.safetyCopy")
        }
        if case .uncertain = presentation.backupPublication {
            Text("The safety copy’s publication could not be confirmed. Keep recovery files and reopen Meguri before continuing.", bundle: .module)
                .padding()
        }
    }
    private func unavailable(_ entry: BackupEntry) -> Bool {
        entry.kind == .serverCopyUnverified || presentation.rejectedNames.contains(entry.name)
    }
    private var confirmationTitle: Text {
        let date = confirmedEntry.map { BackupPresentation.timestamp($0, locale: locale) } ?? ""
        if let entry = confirmedEntry, duplicateIndex(entry) != nil {
            return Text("Replace the current collection with \(entry.name) from \(date)?", bundle: .module)
        }
        return Text("Replace the current collection with the backup from \(date)?", bundle: .module)
    }
    private var confirmationMessage: LocalizedStringKey {
        switch presentation.provider {
        case .iCloud:
            "Meguri backs up the current collection first. Media files are kept. The restored collection stays on this device until you choose what to do with iCloud."
        case .meguri:
            "Meguri backs up the current collection first. Media files are kept. Restoring does not upload this collection to Meguri Sync. Review the collection choice in Sync before publishing."
        default:
            "Meguri backs up the current collection first. Media files are kept."
        }
    }
}

private struct BackupProgress: View {
    let work: BackupCoordinator.Work?
    let canCancel: Bool
    let cancel: () -> Void
    var body: some View {
        ProgressView {
            Text(work == .restore ? "Restoring Backup…" : "Creating Backup…", bundle: .module)
        }
        .accessibilityLabel(Text(work == .restore ? "Restoring Backup…" : "Creating Backup…", bundle: .module))
        .accessibilityValue(Text("In progress", bundle: .module))
        .accessibilityIdentifier("settings.backups.progress")
        if canCancel {
            Button(action: cancel) { Text("Cancel", bundle: .module) }
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
        formatter.setLocalizedDateFormatFromTemplate("yMMMdjmmss")
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

// Table selection and touch List selection use the same stable file identity.
private struct BackupHistoryRow: Identifiable {
    let entry: BackupEntry
    var id: String { entry.name }
}

/// A compact initial content viewport. The native window remains freely resizable.
public enum BackupRestoreWindowLayout {
    public static let id = "settings.backups.restoreWindow"
    public static let defaultSize = CGSize(width: 560, height: 240)
    public static let minimumSize = CGSize(width: 420, height: 230)
    public static var title: String { String(localized: "Restore from Backup", bundle: .module) }
}
