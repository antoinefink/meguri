import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

/// N23.2: the status symbol, the same in the toolbar, the panel and the Sync
/// section: one system symbol, drawn by the system at its place's size and
/// weight. Only needs you has a colour.
struct SyncStatusSymbol: View {
    let status: SyncMainStatus
    /// Only the toolbar button's symbol turns (N23.1).
    var turning = false
    /// The panel and the Sync section draw it in the secondary colour, the
    /// toolbar in its own, Sync Off excepted; needs you is orange everywhere (N23.2).
    var secondary = false
    /// N23.6: the toolbar's check, for a moment after a sync the person started
    /// from the button succeeds: the accent colour, then the default again.
    var accented = false
    var body: some View {
        let glyph = Image(systemName: SyncSymbol.glyph(status.symbol))
            // Only the moving symbol ever turns, whatever drives `turning`.
            .symbolEffect(.rotate, options: .repeating, isActive: turning && status.symbol == SyncSymbol.moving)
        Group {
            if status.needsYou { glyph.foregroundStyle(.orange) }
            else if accented, status.symbol == SyncSymbol.upToDate { glyph.foregroundStyle(.tint) }
            else if secondary || status.symbol == SyncSymbol.off { glyph.foregroundStyle(.secondary) }
            else { glyph }
        }
        // The title beside it, or the button's own label, says the state.
        .accessibilityHidden(true)
    }
}

/// N23: the panel the sync status button opens. One status line, one
/// sentence, a bar while something moves, then Sync Settings and the
/// state's one action, trailing. The status is the Sync section's.
struct SyncStatusPanel: View {
    let collection: ReviewCollection
    let showSettings: (_ keepCollection: Bool) -> Void
    @Environment(\.appClock) private var clock
    #if !os(macOS)
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    #endif
    private var sync: SyncCoordinator { collection.sync }
    private var slot: SyncPanel.Slot { Self.slot(collection) }
    /// N23.3's action place, which the toolbar button's tap also reads (N23.6).
    @MainActor static func slot(_ collection: ReviewCollection) -> SyncPanel.Slot {
        let sync = collection.sync
        let status = sync.mainStatus
        let offered = status.action.map {
            SyncStatusActionButton.offered($0, collection: collection, inPanel: true)
        } ?? false
        return SyncPanel.slot(status, serviceSelected: sync.panelServiceSelected, offered: offered)
    }
    var body: some View {
        let status = sync.mainStatus
        VStack(alignment: .leading, spacing: Spacing.medium) {
            HStack(alignment: .firstTextBaseline, spacing: Spacing.small) {
                SyncStatusSymbol(status: status, secondary: true)
                    .font(.title2)
                VStack(alignment: .leading, spacing: Spacing.xSmall) {
                    Text(SyncText.value(status.titleKey)).font(.headline)
                    if let detail = SyncStatusCopy.detail(sync, now: clock.now()) {
                        Text(detail).font(.callout).foregroundStyle(.secondary)
                    }
                    if let issues = issuesLine(status) {
                        Text(issues).font(.footnote).foregroundStyle(.secondary)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .fixedSize(horizontal: false, vertical: true)
            }
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("sync.status.panel.status")
            if let progress = sync.panelProgress {
                VStack(alignment: .leading, spacing: Spacing.xSmall) {
                    Group {
                        if let fraction = progress.fraction {
                            ProgressView(value: fraction, total: 1)
                        } else {
                            ProgressView()
                        }
                    }
                    .progressViewStyle(.linear)
                    .accessibilityLabel(SyncText.value(status.titleKey))
                    .accessibilityIdentifier("sync.status.panel.progress")

                }
            }
            buttons
        }
    }
    /// N23.1: one trailing row; stacked, trailing, the action first, at the
    /// accessibility text sizes. One copy of each button, whatever the layout.
    private var buttons: some View {
        let stacked = stacks
        let layout = stacked ? AnyLayout(VStackLayout(alignment: .trailing, spacing: Spacing.xSmall))
            : AnyLayout(HStackLayout(spacing: Spacing.small))
        return layout {
            if stacked {
                slotButton
                settingsButton
            } else {
                Spacer(minLength: 0)
                settingsButton
                slotButton
            }
        }
        .frame(maxWidth: .infinity, alignment: .trailing)
        .controlSize(.regular)
    }
    private var stacks: Bool {
        #if os(macOS)
        false
        #else
        dynamicTypeSize.isAccessibilitySize
        #endif
    }
    private var settingsButton: some View {
        Button(SyncText.value("sync.status.openSettings")) { showSettings(false) }
            .modifier(PanelDefault(isDefault: SyncPanel.settingsIsDefault(slot)))
            .accessibilityIdentifier("sync.status.settings")
    }
    @ViewBuilder private var slotButton: some View {
        let slot = slot
        switch slot {
        case .none, .nothingToRun:
            EmptyView()
        case .syncNow:
            SyncNowButton(collection: collection, showsUnavailableReason: false, disabledWhileMoving: true)
                .modifier(PanelDefault(isDefault: true))
        case .action(let action):
            SyncStatusActionButton(collection: collection, action: action, host: .main,
                showInSettings: showSettings, identifier: "sync.status.action", inPanel: true)
                .modifier(PanelDefault(isDefault: SyncPanel.slotIsDefault(slot)))
        }
    }
    private func issuesLine(_ status: SyncMainStatus) -> String? {
        guard SyncStatusCopy.showsIssues(sync) else { return nil }
        // As the Sync section's issues row reads (N15.4).
        if let key = status.issuesRowKey, let count = MeguriAffectedCount.value(sync) {
            return SyncText.format(key, Int64(clamping: count))
        }
        return SyncText.format("sync.issues.row", Int64(status.issues.count))
    }
}

/// N23.3: the default button is the one Return runs on Mac; on touch it is
/// the prominent one. The other buttons keep the system's bordered style.
private struct PanelDefault: ViewModifier {
    let isDefault: Bool
    func body(content: Content) -> some View {
        if isDefault {
            #if os(macOS)
            content.keyboardShortcut(.defaultAction)
            #else
            content.buttonStyle(.borderedProminent)
            #endif
        } else {
            #if os(macOS)
            content
            #else
            content.buttonStyle(.bordered)
            #endif
        }
    }
}

/// The files that did not transfer, for Meguri Sync's media-blocked row (T19.5).
enum MeguriAffectedCount {
    @MainActor static func value(_ sync: SyncCoordinator) -> UInt64? { sync.meguriAffectedFileCount }
}

/// The status's one action (T19.4, T19.5), in the Sync section's status row
/// and in the panel. Shown only when it can run now (N15.2).
struct SyncStatusActionButton: View {
    let collection: ReviewCollection
    let action: SyncStatusAction
    var host: AppNavigation.SheetHost = .main
    /// The compact status sheet hands its destination over after it dismisses.
    /// Set outside Settings (the panel): Meguri's setup sheets belong to the
    /// Sync section, so its actions open that section, asking for N15.3's sheet or not.
    var showInSettings: ((_ keepCollection: Bool) -> Void)? = nil
    var identifier = "settings.sync.statusAction"
    /// The panel offers Sign In…, which opens the Sync section's fields (N23.3).
    var inPanel = false
    @State private var recovering = false
    private var sync: SyncCoordinator { collection.sync }
    private var meguri: Bool { sync.providerState?.selection == .meguri }

    /// Whether the button shows: the action exists here and can run now.
    @MainActor static func offered(_ action: SyncStatusAction, collection: ReviewCollection, inPanel: Bool) -> Bool {
        let sync = collection.sync
        switch action {
        case .chooseCollection:
            return sync.providerState?.selection != .meguri || (sync.meguriCanChoose && !sync.meguriBusy)
        case .serviceRecovery, .meguriConnectionRetry: return true
        case .copyDebugInfo:
            // Copy Debug Info requires configured diagnostics callbacks.
            return collection.configuration.fieldDiagnostics != nil
        case .meguriCancel: return sync.meguriBusy
        case .meguriTryAgain: return sync.meguriCanSettle || sync.meguriRoutine == .acceptanceUnknown || sync.meguriCanChoose
        case .meguriFinishSetup: return sync.meguriCanFinish
        // The Sync section shows the sign-in fields themselves.
        case .meguriSignIn: return inPanel
        // A paused or stopped setup media step (resume fix): the status row's own action.
        case .meguriRetryMedia: return sync.meguriCanRetryMedia || (sync.meguriRoutine == .mediaBlocked && !sync.meguriBusy)
        // MP7 step 5: shown while the upload waits, disabled while something else runs.
        case .meguriUploadStructure: return sync.meguriStructureUploadPending
        }
    }

    var body: some View {
        if Self.offered(action, collection: collection, inPanel: inPanel) { button }
    }

    @ViewBuilder private var button: some View {
        let title = SyncText.value(action.titleKey)
        let id = identifier
        #if os(macOS)
        // N23.3: on Mac a transient popover can close when its alert takes the
        // keys, so an action with a confirmation opens the Sync section, whose row asks.
        if inPanel, action == .serviceRecovery {
            Button(title) { showInSettings?(false) }
                .disabled(collection.syncPresentation.serviceChanging)
                .accessibilityIdentifier(id)
        } else { actionButton(title: title, id: id) }
        #else
        actionButton(title: title, id: id)
        #endif
    }

    @ViewBuilder private func actionButton(title: String, id: String) -> some View {
        switch action {
        case .chooseCollection:
            Button(title) { askKeepCollection() }
                .accessibilityLabel(SyncText.value("sync.decision.action.label"))
                .accessibilityIdentifier(id)
        case .copyDebugInfo:
            if let callbacks = collection.configuration.fieldDiagnostics {
                SyncCopyDebugInfoButton(callbacks: callbacks, sync: sync, identifier: id)
            }
        case .serviceRecovery:
            Button(title) { recovering = true }
                .disabled(collection.syncPresentation.serviceChanging)
                .accessibilityIdentifier(id)
                .alert(SyncText.value("sync.service.recovery.title"), isPresented: $recovering) {
                    Button(SyncText.value("sync.service.recovery.confirm"), role: .destructive) {
                        collection.syncPresentation.serviceChanging = true
                        Task {
                            collection.syncPresentation.serviceChangeFailed = !(await sync.recoverProviderHandoffToLocalOnly())
                            collection.syncPresentation.serviceChanging = false
                        }
                    }
                    Button(SyncText.value("sync.service.cancel"), role: .cancel) {}
                } message: { Text(SyncText.value("sync.service.recovery.message")) }
        case .meguriConnectionRetry:
            Button(title) { sync.loadMeguriConnection() }
                .disabled(sync.meguriBusy).accessibilityIdentifier(id)
        case .meguriCancel:
            // T20.1: in the panel a plain Cancel would read as closing it.
            Button(inPanel ? SyncText.value("sync.status.cancelSetup") : title, role: .cancel) {
                sync.cancelMeguriPreparation()
            }.accessibilityIdentifier(id)
        case .meguriTryAgain:
            // An unknown acceptance runs MP6.9's settling step; any other
            // failure runs setup again from preparation (T19.5). The sheet that
            // setup may open belongs to the Sync section, so outside Settings
            // that section opens too.
            if sync.meguriCanSettle {
                Button(title) { sync.settleMeguriSetup() }.accessibilityIdentifier(id)
            } else if sync.meguriRoutine == .acceptanceUnknown {
                Button(title) { sync.settleMeguriRound() }
                    .disabled(sync.meguriBusy).accessibilityIdentifier(id)
            } else {
                // A sheet opened from a status action keeps the service on Cancel (V27.4).
                Button(title) { sync.startMeguriSetup(keepingService: true); showInSettings?(false) }.accessibilityIdentifier(id)
            }
        case .meguriFinishSetup:
            Button(title) { sync.finishMeguriSetup() }.accessibilityIdentifier(id)
        case .meguriSignIn:
            // T20.1: Sign In… opens the Sync section at the sign-in fields.
            Button(SyncText.value("sync.status.signIn")) { showInSettings?(false) }
                .accessibilityIdentifier(id)
        case .meguriRetryMedia:
            // Runs at once, from the panel too (N23.3): the media step continues.
            Button(title) {
                if sync.meguriRoutine == .mediaBlocked { sync.retryMeguriRoundMedia() } else { sync.retryMeguriMedia() }
            }
            .disabled(sync.meguriBusy).accessibilityIdentifier(id)
        case .meguriUploadStructure:
            // Runs at once, from the panel too (N23.3), and is never the default:
            // it replaces the collection on Meguri Sync, as the Save sheet agreed.
            Button(title) { sync.uploadNotetypeStructure() }
                .disabled(!sync.meguriCanUploadStructure).accessibilityIdentifier(id)
        }
    }
    private func askKeepCollection() {
        if let showInSettings { showInSettings(true) }
        else { collection.syncPresentation.keepCollectionRequested = true }
    }
}
