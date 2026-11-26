import AppModel
import CoreBridge
import DesignSystem
import SwiftUI
#if os(macOS)
import AppKit
#else
import CoreText
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

struct BackupsContent: View {
    let presentation: BackupsPresentation
    let actions: BackupsActions
    let folderFailed: Bool
    var body: some View {
        Form {
            Section {
                Button(action: actions.create) { Text("Back Up Now", bundle: .module) }
                    .backupFormActionTint()
                    .disabled(!presentation.canCreate)
                    .accessibilityIdentifier("settings.backups.create")
                if presentation.work == .create { BackupProgress(work: presentation.work, canCancel: presentation.canCancel, cancel: actions.cancel) }
                if let instant = presentation.lastBackupMilliseconds {
                    LabeledContent {
                        Text(Date(timeIntervalSince1970: Double(instant) / 1_000), format: .dateTime)
                            .backupTouchSecondaryStyle()
                    } label: { Text("Last backup", bundle: .module) }
                } else if presentation.lastBackupUnavailable {
                    Text("Last backup unavailable. Try refreshing Backups.", bundle: .module)
                } else if presentation.loading { ProgressView() }
                else { Text("No successful backup recorded", bundle: .module) }
            } footer: {
                caption {
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
            }
            Section {
                Button(action: actions.restore) { Text("Restore from Backup…", bundle: .module) }
                    .backupFormActionTint()
                    .disabled(!presentation.canOpenRestore)
                    .accessibilityIdentifier("settings.backups.restore")
                Button(action: actions.revealFolder) {
                    #if os(macOS)
                    Text("Show in Finder", bundle: .module)
                    #else
                    Text("Show in Files", bundle: .module)
                    #endif
                }
                .backupFormActionTint()
                .accessibilityIdentifier("settings.backups.folder")
                Button(action: actions.export) { Text("Export Collection with Media…", bundle: .module) }
                    .backupFormActionTint()
                    .disabled(!presentation.canExport)
                    .accessibilityIdentifier("settings.backups.export")
                if folderFailed { Text("The backup folder could not be opened. Check file access and try again.", bundle: .module) }
            } footer: {
                caption {
                    Text("Device backups contain only the collection. Export it to save media too.", bundle: .module)
                        #if os(iOS)
                        .font(.footnote)
                        #endif
                        .accessibilityIdentifier("settings.backups.mediaExplanation")
                }
            }
            Section {
                if let limits = presentation.limits {
                    limit("Interval", field: .interval, value: limits.minimumIntervalMinutes)
                } else {
                    Text("Backup settings are unavailable until the collection is ready.", bundle: .module)
                }
            } header: { caption(footer: false) { Text("Automatic Backups", bundle: .module) } }
            footer: { caption { Text("Synced settings. Enter 5–9999 minutes.", bundle: .module) } }
            if let limits = presentation.limits {
                Section {
                    limit("Daily", field: .daily, value: limits.daily)
                    limit("Weekly", field: .weekly, value: limits.weekly)
                    limit("Monthly", field: .monthly, value: limits.monthly)
                } header: { caption(footer: false) { Text("Keep Backups", bundle: .module) } }
                footer: { caption { Text("Synced settings. Enter 0–9999 backups per period.", bundle: .module) } }
            }
            if let url = presentation.serverBackupsURL {
                Section {
                    Link(destination: url) { Text("Server Backups in Browser…", bundle: .module) }
                        .backupFormActionTint()
                        .accessibilityIdentifier("settings.backups.server")
                } footer: {
                    caption {
                        Text("Sign in to your Meguri Sync account in the browser. Scheduled server backups include media. Restoring there replaces the server collection.", bundle: .module)
                            .accessibilityIdentifier("settings.backups.serverExplanation")
                    }
                }
            }
        }
        .formStyle(.grouped)
        .navigationTitle(Text("Backups", bundle: .module))
        .accessibilityIdentifier("settings.backups.content")
    }

    // Mac light-mode failures were footer text; ordinary headers retain native styling.
    @ViewBuilder private func caption<Content: View>(footer: Bool = true, @ViewBuilder _ content: () -> Content) -> some View {
        #if os(iOS)
        // Footer status views also use the secondary style.
        content().foregroundStyle(ColorToken.textSecondary.color, ColorToken.textSecondary.color)
        #else
        content().backupMacLightTextStyle(when: footer)
        #endif
    }

    private func limit(_ title: LocalizedStringKey, field: BackupCoordinator.LimitField, value: UInt32) -> some View {
        BackupLimitRow(title: title, identifier: "settings.backups.limit.\(String(describing: field))", field: field, value: value, enabled: presentation.canEditLimits) { edited in
            actions.editLimit(field, edited)
        }
    }
}

private extension View {
    // Measured backup captions/status text; touch keeps its existing exception in all appearances.
    @ViewBuilder func backupTouchSecondaryStyle() -> some View {
        #if os(iOS)
        self.foregroundStyle(ColorToken.textSecondary.color, ColorToken.textSecondary.color)
        #else
        self.backupMacLightTextStyle()
        #endif
    }

    @ViewBuilder func backupMacLightTextStyle(when applies: Bool = true, native: HierarchicalShapeStyle? = nil) -> some View {
        #if os(macOS)
        modifier(BackupMacLightTextStyle(applies: applies, native: native))
        #else
        self
        #endif
    }

    // Native light iPhone action tint measured below 4.5:1.
    @ViewBuilder func backupFormActionTint() -> some View {
        #if os(iOS)
        self.tint(ColorToken.statusAccent.color)
        #else
        self
        #endif
    }
}

#if os(macOS)
/// Only measured app-owned labels opt in; dark appearance and selection keep native styles.
private struct BackupMacLightTextStyle: ViewModifier {
    let applies: Bool
    let native: HierarchicalShapeStyle?
    @Environment(\.colorScheme) private var colorScheme

    @ViewBuilder func body(content: Content) -> some View {
        if applies && colorScheme == .light {
            content.foregroundStyle(ColorToken.textSecondary.color)
        } else if let native {
            content.foregroundStyle(native)
        } else {
            content
        }
    }
}
#endif

private struct BackupLimitRow: View {
    let title: LocalizedStringKey
    let identifier: String
    let field: BackupCoordinator.LimitField
    let value: UInt32
    private var minimum: UInt32 { field == .interval ? 5 : 0 }
    private var unit: LocalizedStringKey? { field == .interval ? "min" : nil }
    let enabled: Bool
    let save: (UInt32) -> Void
    @State private var draft = BackupLimitDraft()
    #if os(iOS)
    @State private var focused = false
    @Environment(\.locale) private var locale
    #else
    @FocusState private var focused: Bool
    #endif
    @Environment(\.dynamicTypeSize) private var textSize
    var body: some View {
        Group {
            #if os(iOS)
            if textSize.isAccessibilitySize {
                VStack(alignment: .leading) {
                    fieldLabel.frame(maxWidth: .infinity, alignment: .leading)
                    input
                }
            } else { regularRow }
            #else
            regularRow
            #endif
        }
        #if os(iOS)
        // Use the existing row's vertical space for the real input, rather than
        // adding a 44-point control to the Form's ordinary text-row padding.
        .listRowInsets(.vertical, Spacing.xxSmall)
        #endif
        .onChange(of: value, initial: true) { _, value in if !focused { draft.reset(to: value) } }
        if draft.invalid { Text("Enter a whole number in the allowed range.", bundle: .module).foregroundStyle(.secondary) }
    }
    private var regularRow: some View {
        LabeledContent {
            HStack {
                // Measure all ten UInt32 digits in the current font, including AX.
                // The field takes only that space instead of proposing a full row.
                ZStack(alignment: .trailing) {
                    Text(verbatim: String(UInt32.max)).padding(.horizontal, Spacing.small).hidden().accessibilityHidden(true)
                    input.multilineTextAlignment(.trailing)
                }
                .monospacedDigit()
                .fixedSize(horizontal: true, vertical: false)
                if let unit {
                    Text(unit, bundle: .module)
                        #if os(iOS)
                        .foregroundStyle(ColorToken.textSecondary.color)
                        #else
                        .backupMacLightTextStyle(when: enabled, native: .secondary)
                        #endif
                }
            }
            .fixedSize(horizontal: true, vertical: false)
        } label: { Text(title, bundle: .module) }
        .labeledContentStyle(BackupLimitRowStyle(stacked: textSize.isAccessibilitySize))
    }
    private var fieldLabel: Text {
        field == .interval ? Text("Interval in minutes", bundle: .module) : Text(title, bundle: .module)
    }
    private var input: some View {
        #if os(iOS)
        BackupLimitTouchField(text: $draft.text, focused: $focused, identifier: identifier,
                              label: touchFieldLabel, enabled: enabled,
                              trailing: !textSize.isAccessibilitySize)
            .onChange(of: focused) { _, active in
                if active { draft.reset(to: value) } else { commit() }
            }
            .disabled(!enabled)
        #else
            TextField(text: $draft.text) { Text(title, bundle: .module) }
                .labelsHidden()
                .accessibilityLabel(fieldLabel)
                .accessibilityIdentifier(identifier)
                .focused($focused)
                .onSubmit(commit)
                .onChange(of: focused) { _, active in
                    if active { draft.reset(to: value) } else { commit() }
                }
                .disabled(!enabled)
        #endif
    }
    #if os(iOS)
    private var touchFieldLabel: String {
        switch field {
        case .interval: String(localized: "Interval in minutes", bundle: .module, locale: locale)
        case .daily: String(localized: "Daily", bundle: .module, locale: locale)
        case .weekly: String(localized: "Weekly", bundle: .module, locale: locale)
        case .monthly: String(localized: "Monthly", bundle: .module, locale: locale)
        }
    }
    #endif
    private func commit() {
        guard enabled else { return }
        if let number = draft.commit(value: value, minimum: minimum) { save(number) }
    }
}

#if os(iOS)
/// One editable native element: its UIView bounds are both its AX and hit frame.
/// SwiftUI padding around a text-sized field does not enlarge that native frame.
private struct BackupLimitTouchField: UIViewRepresentable {
    @Binding var text: String
    @Binding var focused: Bool
    let identifier: String
    let label: String
    let enabled: Bool
    let trailing: Bool
    @Environment(\.layoutDirection) private var layoutDirection
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize

    private func preferredFont(for field: UITextField) -> UIFont {
        let traits = field.traitCollection.modifyingTraits {
            $0.preferredContentSizeCategory = UIContentSizeCategory(dynamicTypeSize)
        }
        let font = UIFont.preferredFont(forTextStyle: .body, compatibleWith: traits)
        guard trailing else { return font }
        let descriptor = font.fontDescriptor.addingAttributes([
            .featureSettings: [[
                UIFontDescriptor.FeatureKey.type: kNumberSpacingType,
                UIFontDescriptor.FeatureKey.selector: kMonospacedNumbersSelector
            ]]
        ])
        return UIFont(descriptor: descriptor, size: 0)
    }

    func makeUIView(context: Context) -> UITextField {
        let field = UITextField()
        field.borderStyle = .none
        field.keyboardType = .numbersAndPunctuation
        field.delegate = context.coordinator
        field.addTarget(context.coordinator, action: #selector(Coordinator.changed(_:)), for: .editingChanged)
        field.setContentHuggingPriority(.defaultLow, for: .horizontal)
        return field
    }

    func updateUIView(_ field: UITextField, context: Context) {
        context.coordinator.parent = self
        if field.text != text { field.text = text }
        field.font = preferredFont(for: field)
        field.semanticContentAttribute = layoutDirection == .rightToLeft ? .forceRightToLeft : .forceLeftToRight
        let rightToLeft = field.effectiveUserInterfaceLayoutDirection == .rightToLeft
        field.textAlignment = trailing == rightToLeft ? .left : .right
        field.isEnabled = enabled && context.environment.isEnabled
        field.accessibilityIdentifier = identifier
        field.accessibilityLabel = label
    }

    func sizeThatFits(_ proposal: ProposedViewSize, uiView: UITextField, context: Context) -> CGSize? {
        // The regular row asks for an ideal width. Reserve the same ten digits
        // and horizontal space as its existing SwiftUI measuring text.
        let font = uiView.font ?? preferredFont(for: uiView)
        let idealWidth = trailing
            ? (String(UInt32.max) as NSString).size(withAttributes: [.font: font]).width + 2 * Spacing.small
            : uiView.intrinsicContentSize.width
        let width = proposal.width.flatMap { $0.isFinite ? $0 : nil } ?? idealWidth
        return CGSize(width: max(ControlMetrics.minimumTarget, width),
                      height: max(ControlMetrics.minimumTarget, uiView.intrinsicContentSize.height))
    }

    func makeCoordinator() -> Coordinator { Coordinator(parent: self) }

    final class Coordinator: NSObject, UITextFieldDelegate {
        var parent: BackupLimitTouchField
        init(parent: BackupLimitTouchField) { self.parent = parent }
        @objc func changed(_ field: UITextField) { parent.text = field.text ?? "" }
        func textFieldDidBeginEditing(_ field: UITextField) { parent.focused = true }
        func textFieldDidEndEditing(_ field: UITextField) { parent.focused = false }
        func textFieldShouldReturn(_ field: UITextField) -> Bool {
            // A successful resign follows the same editing-ended commit as blur.
            // Committing here too would reset the draft before an async save lands.
            return field.resignFirstResponder()
        }
    }
}
#endif

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
    private let duplicateOrdinals: [BackupWallTime: [String: Int]]
    @Environment(\.locale) private var locale
    @State private var confirmation = false
    @State private var confirmedEntry: BackupEntry?
    @Environment(\.dynamicTypeSize) private var textSize
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    // Native single-line rows plus inline navigation and grouped-list insets.
    // These are scaled point estimates, independent of the presenting viewport.
    @ScaledMetric(relativeTo: .body) private var compactRowHeight: CGFloat = 44
    @ScaledMetric(relativeTo: .body) private var compactChromeHeight: CGFloat = 100
    @State private var hasWrappedRow = false
    #endif
    init(presentation: BackupsPresentation, selectedName: Binding<String?>,
         refresh: @escaping () -> Void, cancel: @escaping () -> Void, restore: @escaping (String) -> Void) {
        self.presentation = presentation
        self._selectedName = selectedName
        self.refresh = refresh; self.cancel = cancel; self.restore = restore
        duplicateOrdinals = Dictionary(grouping: presentation.entries, by: \.wallTime)
            .filter { $0.value.count > 1 }
            .mapValues { entries in
                var ordinals: [String: Int] = [:]
                for (index, entry) in entries.enumerated() where ordinals[entry.name] == nil {
                    ordinals[entry.name] = index + 1
                }
                return ordinals
            }
    }
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
            .presentationSizing(BackupRestorePresentationSizing(
                regularWidth: horizontalSizeClass == .regular, shortHeight: shortHistoryHeight))
            .presentationDetents(horizontalSizeClass == .regular ? [.large] : restoreDetents)
            .interactiveDismissDisabled(presentation.busy || confirmation)
        #endif
    }
    #if os(iOS)
    private var restoreDetents: Set<PresentationDetent> {
        guard let height = shortHistoryHeight else { return [.large] }
        return [.height(height), .large]
    }
    private var shortHistoryHeight: CGFloat? {
        guard textSize <= .large, !hasWrappedRow, presentation.entries.count <= 5,
              presentation.work == nil, presentation.failure == nil, presentation.listFailure == nil,
              !presentation.candidateActive, !presentation.activationUncertain, presentation.safetyBackup == nil,
              presentation.entries.allSatisfy({
                  $0.kind == .backup && !unavailable($0) && duplicateIndex($0) == nil
              }) else { return nil }
        if case .uncertain = presentation.backupPublication { return nil }
        // Reserve three rows before discovery too: opening a short history must
        // not first flash a large sheet. Real errors/work use the large detent.
        let rows = presentation.entries.isEmpty ? 3 : presentation.entries.count
        // iPad needs taller native rows and more bottom room, even in compact sheets.
        let isPad = UIDevice.current.userInterfaceIdiom == .pad
        let rowHeight = compactRowHeight * (isPad ? 52.0 / 44.0 : 1)
        let chromeHeight = compactChromeHeight * (isPad ? 112.0 / 100.0 : 1)
        return rowHeight * CGFloat(rows) + chromeHeight
    }
    #endif
    private var restoreBody: some View {
        restoreLayout
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("settings.backups.restoreContent")
            .navigationTitle(restoreTitle)
            #if os(iOS)
            .accessibilityLabel(Text("Restore from Backup", bundle: .module))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { cancelButton }
                ToolbarItem(placement: .confirmationAction) { restoreButton }
            }
            #endif
            #if os(macOS)
            .confirmationDialog(confirmationTitle, isPresented: $confirmation, titleVisibility: .visible) {
                confirmationActions
            } message: { confirmationText }
            #else
            .alert(confirmationTitle, isPresented: $confirmation) {
                confirmationActions
            } message: { confirmationText }
            #endif
    }
    @ViewBuilder private var confirmationActions: some View {
        Button(role: .destructive) {
            if let entry = confirmedEntry { restore(entry.name) }
        } label: { Text("Restore", bundle: .module) }
        Button(role: .cancel) {} label: { Text("Cancel", bundle: .module) }
    }
    private var confirmationText: Text { Text(confirmationMessage, bundle: .module) }
    @ViewBuilder private var restoreLayout: some View {
        #if os(macOS)
        // Allocate the footer outside the history. Native Table drawing must
        // stop at its viewport even when more rows exist below the window.
        VStack(spacing: 0) {
            restoreContent
                .frame(maxWidth: .infinity, minHeight: 0, maxHeight: .infinity)
                .clipped()
            Divider()
            HStack { Spacer(); cancelButton; restoreButton }
                .padding()
                .background(Color(nsColor: .windowBackgroundColor))
        }
        #else
        restoreContent
        #endif
    }
    private var restoreContent: some View {
            VStack {
                if presentation.work == .restore {
                    BackupProgress(work: presentation.work, canCancel: presentation.canCancel, cancel: cancel)
                        .padding()
                } else if presentation.loading {
                    ProgressView {
                        Text("Loading Backups…", bundle: .module)
                            .backupTouchSecondaryStyle()
                    }
                        .accessibilityLabel(Text("Loading Backups…", bundle: .module))
                        .accessibilityValue(Text("In progress", bundle: .module))
                        .accessibilityIdentifier("settings.backups.loading")
                } else if showsListFailure, let error = presentation.listFailure {
                    ScrollView {
                        ContentUnavailableView {
                            Label { Text("Unable to Load Backups", bundle: .module).backupMacLightTextStyle() } icon: { Image(systemName: "exclamationmark.triangle") }
                        } description: {
                            BackupFailureText(error: error, candidateActive: false, uncertain: false, restore: false)
                                .fixedSize(horizontal: false, vertical: true)
                                .accessibilityIdentifier("settings.backups.listFailure")
                        } actions: {
                            retryButton
                        }
                        restoreNotices
                    }
                } else if presentation.entries.isEmpty {
                    ContentUnavailableView {
                        Label { Text("No Backups Yet", bundle: .module).backupMacLightTextStyle() } icon: { Image(systemName: "clock.arrow.circlepath") }
                    } description: {
                        Text("Create a backup on this device, or check the configured backup folder.", bundle: .module)
                            .backupTouchSecondaryStyle()
                    }
                    actions: { retryButton }
                } else {
                    history
                }
                if !showsListFailure { restoreNotices }
            }
    }
    private var retryButton: some View {
        Button(action: refresh) {
            Text("Try Again", bundle: .module)
                #if os(iOS)
                // Reserve for compact sheet presentation scaling.
                .frame(minWidth: ControlMetrics.minimumTarget,
                       minHeight: ControlMetrics.minimumTarget + (horizontalSizeClass == .compact ? Spacing.xxSmall : 0))
                .contentShape(Rectangle())
                #endif
        }
        .backupFormActionTint()
        .accessibilityIdentifier("settings.backups.retry")
    }
    private var restoreTitle: Text {
        #if os(iOS)
        if horizontalSizeClass == .compact { return Text("Backups", bundle: .module) }
        #endif
        return Text("Restore from Backup", bundle: .module)
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
                let isUnavailable = unavailable(row.entry)
                Text(verbatim: BackupPresentation.size(row.entry.sizeBytes))
                    .backupMacLightTextStyle(when: isUnavailable && selectedName != row.entry.name,
                                             native: isUnavailable ? .secondary : .primary)
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
        .frame(minHeight: 0, maxHeight: .infinity)
        .clipped()
        #else
        List(selection: selection) {
            ForEach(presentation.entries, id: \.name) { entry in
                Group {
                    if textSize.isAccessibilitySize {
                        VStack(alignment: .leading) {
                            entryLabel(entry)
                            Text(verbatim: BackupPresentation.size(entry.sizeBytes))
                                .foregroundStyle(unavailable(entry) ? .secondary : .primary)
                        }
                        .fixedSize(horizontal: false, vertical: true)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    } else {
                        HStack(alignment: .firstTextBaseline) {
                            entryLabel(entry)
                            Spacer()
                            Text(verbatim: BackupPresentation.size(entry.sizeBytes))
                                .foregroundStyle(unavailable(entry) ? .secondary : .primary)
                        }
                    }
                }
                .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { height in
                    // Observe the intrinsic row content, never the parent sheet.
                    // Promotion is one-way so a detent cannot feed back into sizing.
                    if height > compactRowHeight * 0.75 { hasWrappedRow = true }
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
        let isUnavailable = unavailable(entry)
        #if os(macOS)
        let isUnselected = selectedName != entry.name
        #endif
        return VStack(alignment: .leading) {
            Text(verbatim: BackupPresentation.timestamp(entry, locale: locale))
            if let index = duplicateIndex(entry) {
                Text("Archive \(index)", bundle: .module)
                    #if os(macOS)
                    .backupMacLightTextStyle(when: isUnselected, native: .secondary)
                    #else
                    .foregroundStyle(.secondary)
                    #endif
            }
            if entry.kind == .serverCopy { Text("Server copy", bundle: .module) }
            if isUnavailable { Text("Can’t be restored", bundle: .module) }
        }
        #if os(macOS)
        .backupMacLightTextStyle(when: isUnavailable && isUnselected, native: isUnavailable ? .secondary : .primary)
        #else
        .foregroundStyle(isUnavailable ? .secondary : .primary)
        #endif
        .help(entry.name)
    }
    private func duplicateIndex(_ entry: BackupEntry) -> Int? {
        duplicateOrdinals[entry.wallTime]?[entry.name]
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
        !BackupCoordinator.isRestorable(entry, rejectedNames: presentation.rejectedNames)
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

#if os(iOS)
private struct BackupRestorePresentationSizing: PresentationSizing {
    let regularWidth: Bool
    let shortHeight: CGFloat?

    func proposedSize(for root: PresentationSizingRoot, context: PresentationSizingContext) -> ProposedViewSize {
        // Keep phone detents and one NavigationStack across size-class changes.
        guard regularWidth else { return PagePresentationSizing.page.proposedSize(for: root, context: context) }
        let proposal = FormPresentationSizing.form.proposedSize(for: root, context: context)
        // Both page and form sizing have native height floors. Keep the system
        // form width, but let an eligible short history supply its own height.
        guard let shortHeight else { return proposal }
        return .init(width: proposal.width, height: shortHeight)
    }
}
#endif

private struct BackupProgress: View {
    let work: BackupCoordinator.Work?
    let canCancel: Bool
    let cancel: () -> Void
    var body: some View {
        ProgressView {
            Text(work == .restore ? "Restoring Backup…" : "Creating Backup…", bundle: .module)
                .backupTouchSecondaryStyle()
        }
        .accessibilityLabel(Text(work == .restore ? "Restoring Backup…" : "Creating Backup…", bundle: .module))
        .accessibilityValue(Text("In progress", bundle: .module))
        .accessibilityIdentifier("settings.backups.progress")
        if canCancel {
            Button(action: cancel) { Text("Cancel", bundle: .module) }
                .backupFormActionTint()
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
        #if os(iOS)
        .backupTouchSecondaryStyle()
        #else
        .backupMacLightTextStyle(native: .secondary)
        #endif
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
