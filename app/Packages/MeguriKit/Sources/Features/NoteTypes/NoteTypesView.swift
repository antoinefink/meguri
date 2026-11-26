import AppModel
import CoreBridge
import DesignSystem
import SwiftUI
#if os(macOS)
import AppKit
#endif

/// The Note Types section (`native-interface-contract` N22.1): the list of
/// note types beside the selected one's detail, with Fields and Cards in the
/// toolbar, then Revert and Save at its trailing edge, or while a save job
/// runs one status item in their place (`notetypes/workspace` W2,
/// `notetypes/draft-save` D1, D3, D5; N22.5). The shell shows it as the section's
/// root in the sidebar layout (Mac, iPad); the model outlives it.
public struct NoteTypesView: View {
    @Bindable private var model: NoteTypesModel
    @Environment(\.locale) private var locale
    /// The sync button (N23.6), first of the trailing group.
    @Environment(\.syncStatusItem) private var syncStatusItem
    @State private var jobShown = SaveJobShown()
    /// The window's width (Mac), for N22.1's narrow Cards tab.
    @State private var windowWidth: CGFloat?
    /// Hides the app's sidebar while the Cards tab is narrow (N22.1).
    private let navigation: AppNavigation?

    public init(model: NoteTypesModel, navigation: AppNavigation? = nil) {
        self.model = model
        self.navigation = navigation
    }

    public var body: some View {
        #if os(macOS)
        macBody
        #else
        // iPad: the list is the section's root and a note type is a navigation step (N22.7).
        NoteTypesTouchRoot(model: model)
        #endif
    }

    #if os(macOS)
    private var macBody: some View {
        split
            .navigationTitle(title)
            .navigationSubtitle(subtitle)
            .toolbar { toolbar }
            .confirmationDialog(Text("noteTypes.discard.title", bundle: .module), isPresented: $model.confirmingRevert,
                                titleVisibility: .visible) {
                Button(role: .destructive) {
                    if let id = model.selection { model.revert(id) }
                } label: { Text("noteTypes.discard.discard", bundle: .module) }
                    .accessibilityIdentifier("noteTypes.list.discard.discard")
                Button(role: .cancel) { model.confirmingRevert = false } label: { Text("noteTypes.discard.keepEditing", bundle: .module) }
            }
            .modifier(SaveOutcomeAlerts(model: model))
            .task(id: model.progress?.notetypeId) { await SaveJobShown.follow(model.progress?.notetypeId, into: $jobShown) }
            .onAppear { model.appear() }
            // N22.1: under about 1,100 points the sidebar steps aside while the Cards tab shows.
            .background { WindowWidthReader(width: $windowWidth) }
            .onChange(of: narrowCards, initial: true) { _, narrow in navigation?.setNarrowWorkspace(narrow) }
            .onDisappear { navigation?.setNarrowWorkspace(false) }
    }

    /// N22.1's width under which the Cards tab hides the app's sidebar.
    static let narrowWindow: CGFloat = 1_100

    private var narrowCards: Bool {
        guard model.tab == .cards, model.selection != nil, let windowWidth else { return false }
        return windowWidth < Self.narrowWindow
    }

    @ViewBuilder private var split: some View {
        HSplitView {
            NoteTypeListColumn(model: model)
                .frame(minWidth: Self.listMinimum, idealWidth: Self.listIdeal, maxWidth: Self.listMaximum)
            detail
                // The detail asks for all the room whatever has loaded, so the list
                // opens at its minimum on every launch; with its content's own width
                // the list opened at its minimum or its maximum depending on whether
                // the fields had loaded by the split view's first layout.
                .frame(minWidth: ContentWidth.primaryAction, idealWidth: Self.detailIdeal, maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    /// Wider than any window: the split view gives the list only its minimum at first.
    private static let detailIdeal: CGFloat = 10_000

    /// The list column fits the longest stock name with its count (N22.1).
    private static let listMinimum: CGFloat = 200
    private static let listIdeal: CGFloat = 260
    private static let listMaximum: CGFloat = 320

    @ViewBuilder private var detail: some View {
        // The note type shown stays until the newly selected one can draw,
        // then the detail swaps in one step: never an empty page. A wait of
        // more than half a second shows a progress indicator over it.
        if let id = model.shownSelection {
            NoteTypeDetailContent(model: model, id: id)
                // Shown in the new one's place for a moment: it looks the same, takes no edits.
                .allowsHitTesting(model.selection == id)
                .overlay {
                    if model.selection != id { DelayedProgress() }
                }
        } else if model.phase == .ready {
            ContentUnavailableView {
                Label { Text("noteTypes.detail.none", bundle: .module) } icon: { Image(systemName: "rectangle.stack") }
            }
        } else {
            Color.clear
        }
    }

    /// The title and its counts change together, once the selected note type's
    /// counts are read: never a title over a blank subtitle (`headerSelection`).
    private var title: Text {
        guard let id = model.headerSelection, let name = model.draft(id)?.name ?? model.item(id)?.name else {
            return Text("noteTypes.title", bundle: .module)
        }
        return Text(verbatim: name)
    }

    private var subtitle: String {
        guard model.headerSelection != nil, let counts = model.headerCounts else { return "" }
        return NoteTypesStrings.subtitle(notes: counts.notes, cards: counts.cards, locale: locale)
    }

    @ToolbarContentBuilder private var toolbar: some ToolbarContent {
        ToolbarItem(placement: .principal) {
            Picker(selection: $model.tab) {
                Text("noteTypes.tab.fields", bundle: .module).tag(NoteTypesModel.Tab.fields)
                    .accessibilityIdentifier("noteTypes.list.tabFields")
                Text("noteTypes.tab.cards", bundle: .module).tag(NoteTypesModel.Tab.cards)
                    .accessibilityIdentifier("noteTypes.list.tabCards")
            } label: {
                Text("noteTypes.tabs", bundle: .module)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .disabled(model.selection == nil)
            .accessibilityIdentifier("noteTypes.list.tabs")
        }
        // Revert and Save trail the detail's toolbar. On the Fields tab its
        // inspector's toolbar holds them with its toggle, at the window's
        // trailing end, shown or hidden (`NoteTypeFieldsTab`).
        // The sync button first, then Revert and Save: [sync] [Revert] [Save].
        if !inspectorHoldsSave {
            if let syncStatusItem { ToolbarItem(placement: .primaryAction) { syncStatusItem } }
            NoteTypeSaveToolbar(model: model, running: jobShown.running(in: model))
            // The Cards tab's preview toggle, last, as the Fields tab's inspector toggle is.
            if model.tab == .cards, model.selection != nil {
                ToolbarItem(placement: .primaryAction) { PreviewToggle(preview: model.preview) }
            }
        }
    }

    /// The Fields tab is on screen: its inspector's toolbar holds Revert, Save and the toggle.
    private var inspectorHoldsSave: Bool {
        model.tab == .fields && model.selection != nil && model.shownSelection.flatMap(model.draft) != nil
    }
    #endif
}

#if os(macOS)
/// Reports the window's width as it is resized (the detail's own width
/// changes when the sidebar hides, so it cannot decide that).
struct WindowWidthReader: NSViewRepresentable {
    @Binding var width: CGFloat?

    func makeNSView(context: Context) -> ReaderView {
        let view = ReaderView()
        view.report = { width = $0 }
        return view
    }

    func updateNSView(_ view: ReaderView, context: Context) {
        view.report = { width = $0 }
    }

    final class ReaderView: NSView {
        var report: (CGFloat) -> Void = { _ in }
        private var observer: NSObjectProtocol?

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if let observer { NotificationCenter.default.removeObserver(observer) }
            observer = nil
            guard let window else { return }
            observer = NotificationCenter.default.addObserver(forName: NSWindow.didResizeNotification, object: window, queue: .main) {
                [weak self] _ in
                MainActor.assumeIsolated { self?.publish() }
            }
            publish()
        }

        private func publish() {
            guard let width = window?.frame.width else { return }
            DispatchQueue.main.async { [weak self] in self?.report(width) }
        }

        isolated deinit { if let observer { NotificationCenter.default.removeObserver(observer) } }
    }
}
#endif

/// Revert and Save (N22.5), or while a save job runs, one status item in
/// their place; they return when it ends. A plain status, not a control,
/// so it has no glass of its own. One item either way, so the toolbar
/// keeps its other items as the job starts and ends.
struct NoteTypeSaveToolbar: ToolbarContent {
    let model: NoteTypesModel
    /// The selected note type's save job, once the toolbar shows it.
    let running: (id: Int64, progress: NoteTypesModel.Progress)?
    /// The detail's trailing edge, or (`.automatic`) the inspector's own part of the toolbar.
    var placement: ToolbarItemPlacement = .primaryAction

    var body: some ToolbarContent {
        ToolbarItem(placement: placement) {
            if let running {
                SaveStatusView(progress: running.progress, notes: model.typeCounts[running.id]?.notes)
            } else {
                NoteTypeSaveGroup(model: model)
            }
        }
        .sharedBackgroundVisibility(running == nil ? .automatic : .hidden)
    }
}

/// Revert and Save as one control group: they read as one pair in one shape.
/// Save is not prominent: it is disabled until there is a draft, and a
/// tinted disabled button reads as broken. A view of its own, so the draft's
/// changes reach it.
private struct NoteTypeSaveGroup: View {
    let model: NoteTypesModel

    var body: some View {
        ControlGroup {
            Button {
                model.confirmingRevert = true
            } label: {
                Text("noteTypes.revert", bundle: .module)
            }
            .disabled(model.selection.map { !(model.isEdited($0) && model.editable($0)) } ?? true)
            .accessibilityIdentifier("noteTypes.list.revert")
            Button {
                model.save()
            } label: {
                if let id = model.selection, model.saveAsks(id) {
                    Text("noteTypes.saveAsks", bundle: .module)
                } else {
                    Text("noteTypes.save", bundle: .module)
                }
            }
            // ⌘S is File › Save's (N4).
            .disabled(model.selection.map { !model.canSave($0) } ?? true)
            .accessibilityIdentifier("noteTypes.list.save")
        } label: {
            // The group's title where the toolbar folds it into its overflow menu.
            Text("noteTypes.save", bundle: .module)
        }
    }
}

/// The Fields tab's inspector toggle, last in the window's toolbar, as in
/// Apple's apps: above the inspector while it shows, at the toolbar's
/// trailing end while it is hidden.
struct InspectorToggle: View {
    @Bindable var model: NoteTypesModel

    var body: some View {
        let key: LocalizedStringKey = model.inspectorShown ? "noteTypes.inspector.hide" : "noteTypes.inspector.show"
        Button {
            model.inspectorShown.toggle()
        } label: {
            Label { Text(key, bundle: .module) } icon: { Image(systemName: "sidebar.trailing") }
        }
        .help(Text(key, bundle: .module))
        .accessibilityIdentifier("noteTypes.fields.inspectorToggle")
    }
}

/// When the toolbar shows a running save job (N22.5): half a second after it
/// starts (D5 step 5's "once a save has run for a moment"). A short save
/// leaves the toolbar as it was.
struct SaveJobShown: Equatable {
    private var id: Int64?

    func shown(for notetypeId: Int64) -> Bool { id == notetypeId }

    /// The selected note type's save job, once the toolbar shows it.
    @MainActor func running(in model: NoteTypesModel) -> (id: Int64, progress: NoteTypesModel.Progress)? {
        guard let id = model.selection, let progress = model.progress, progress.notetypeId == id, shown(for: id) else { return nil }
        return (id, progress)
    }

    /// The job whose status has shown already: a view that appears while it
    /// runs (the iPad page pushed again) shows the status at once.
    @MainActor private static var shownJob: Int64?

    /// Follows the job of `notetypeId` (nil: none runs) for as long as the task lives.
    @MainActor static func follow(_ notetypeId: Int64?, into state: Binding<SaveJobShown>) async {
        guard let notetypeId else {
            shownJob = nil
            state.wrappedValue = SaveJobShown()
            return
        }
        if shownJob == notetypeId {
            state.wrappedValue = SaveJobShown(id: notetypeId)
            return
        }
        state.wrappedValue = SaveJobShown()
        guard (try? await Task.sleep(for: .milliseconds(500))) != nil else { return }
        shownJob = notetypeId
        state.wrappedValue = SaveJobShown(id: notetypeId)
    }
}

/// The selected note type's detail: D1's row when it changed elsewhere, then
/// the Fields or Cards tab; a progress indicator while it reads.
struct NoteTypeDetailContent: View {
    let model: NoteTypesModel
    let id: Int64

    var body: some View {
        if model.draft(id) != nil {
            VStack(spacing: .zero) {
                if model.changedElsewhere.contains(id) {
                    ChangedElsewhereRow(model: model, id: id)
                    Divider()
                }
                switch model.tab {
                case .fields: NoteTypeFieldsTab(model: model, id: id)
                case .cards: NoteTypeCardsTab(model: model, id: id)
                }
            }
        } else if model.recordFailed.contains(id) {
            ContentUnavailableView {
                Label { Text("noteTypes.detail.failed.title", bundle: .module) } icon: { Image(systemName: "rectangle.stack") }
            } actions: {
                Button { model.reload(id) } label: { Text("noteTypes.tryAgain", bundle: .module) }
            }
        } else {
            DelayedProgress()
        }
    }
}

/// D5 step 5's status while a save job runs (N22.5): a small spinner and
/// "Updating 212 of 412 notes" ("Saving…" until the first count); with Meguri Sync, the job's stage first
/// ("Syncing…", then the count, then "Uploading…", D6). The widest text the
/// job can show reserves its width, so the toolbar does not move as the
/// stage or the count changes.
struct SaveStatusView: View {
    let progress: NoteTypesModel.Progress
    /// The note type's notes, which the count can reach before its total is known.
    let notes: UInt64?
    @Environment(\.locale) private var locale

    var body: some View {
        // Trailing, where Save was: the spinner sits just before the text.
        ZStack(alignment: .trailing) {
            ForEach(candidates, id: \.self) { text in line(text).hidden() }
            line(current)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text("noteTypes.saving", bundle: .module))
        .accessibilityValue(Text(verbatim: current))
        .accessibilityIdentifier("noteTypes.list.progress")
    }

    private func line(_ text: String) -> some View {
        HStack(spacing: Spacing.xSmall) {
            ProgressView().controlSize(.small)
            Text(verbatim: text)
                .foregroundStyle(.secondary)
                .monospacedDigit()
                .lineLimit(1)
        }
        .fixedSize()
    }

    /// Every text this job can show, so the widest reserves the width.
    private var candidates: [String] {
        [NoteTypesStrings.text("noteTypes.progress.syncing", locale: locale),
         NoteTypesStrings.text("noteTypes.progress.uploading", locale: locale),
         NoteTypesStrings.text("noteTypes.progress.saving", locale: locale), widestCount]
    }

    private var current: String { Self.text(progress, locale: locale) }

    /// The job's stage text.
    static func text(_ progress: NoteTypesModel.Progress, locale: Locale) -> String {
        switch progress.stage {
        case .syncing?: NoteTypesStrings.text("noteTypes.progress.syncing", locale: locale)
        case .uploading?: NoteTypesStrings.text("noteTypes.progress.uploading", locale: locale)
        case .saving?, nil:
            // Before the first count, which can come after the status shows.
            if progress.removing {
                NoteTypesStrings.deletionProgress(progress.completed, locale: locale)
            } else {
                progress.total.map { NoteTypesStrings.progress(completed: progress.completed, total: $0, locale: locale) }
                    ?? NoteTypesStrings.text("noteTypes.progress.saving", locale: locale)
            }
        }
    }

    /// The count at its end, with the largest total this job can have: its digits are monospaced.
    private var widestCount: String {
        let total = max(progress.total ?? 0, notes ?? 0, progress.completed)
        return progress.removing ? NoteTypesStrings.deletionProgress(total, locale: locale)
            : NoteTypesStrings.progress(completed: total, total: total, locale: locale)
    }
}

/// What a save that ended needs to tell: nothing was changed (D5 step 6;
/// with Meguri Sync, D6 step 1's stop), or the changes are saved on this
/// device but not uploaded (D6 step 4).
struct SaveOutcomeAlerts: ViewModifier {
    @Bindable var model: NoteTypesModel
    @Environment(\.locale) private var locale
    #if os(macOS)
    @State private var appVisible = !NSApplication.shared.isHidden

    // Save and deletion share these outcomes. Recheck the actual state in
    // each binding too: a failure can arrive before didHide is delivered.
    private var canPresent: Bool { appVisible && !NSApplication.shared.isHidden }
    #else
    private let canPresent = true
    #endif

    func body(content: Content) -> some View {
        content
            .alert(Text("noteTypes.failed.title", bundle: .module), isPresented: Binding(
                get: { canPresent && model.failure != nil }, set: { if !$0, canPresent { model.failure = nil } })) {
                Button(role: .cancel) { model.failure = nil } label: { Text("noteTypes.failed.ok", bundle: .module) }
            } message: {
                if model.failure == .syncStopped {
                    Text("noteTypes.failed.syncMessage", bundle: .module)
                } else if model.failure == .unconfirmed {
                    Text("noteTypes.failed.unconfirmedMessage", bundle: .module)
                } else {
                    Text("noteTypes.failed.message", bundle: .module)
                }
            }
            .alert(Text("noteTypes.notUploaded.title", bundle: .module), isPresented: Binding(
                get: { canPresent && model.notice != nil && model.failure == nil }, set: { if !$0, canPresent { model.notice = nil } })) {
                Button(role: .cancel) { model.notice = nil } label: { Text("noteTypes.failed.ok", bundle: .module) }
            } message: {
                Text(verbatim: NoteTypesStrings.notUploaded(device: NoteTypesStrings.device(locale: locale), locale: locale))
            }
            #if os(macOS)
            // Keep hidden outcomes pending, including when this view was
            // first built hidden or returns after missing an unhide event.
            .onAppear { appVisible = !NSApplication.shared.isHidden }
            .onReceive(NotificationCenter.default.publisher(for: NSApplication.didHideNotification)) { _ in
                appVisible = false
            }
            .onReceive(NotificationCenter.default.publisher(for: NSApplication.didUnhideNotification)) { _ in
                appVisible = !NSApplication.shared.isHidden
            }
            #endif
    }
}

/// A progress indicator that shows only once a read has taken about half a second.
struct DelayedProgress: View {
    @State private var visible = false
    var body: some View {
        ProgressView()
            .opacity(visible ? 1 : 0)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .task {
                try? await Task.sleep(for: .milliseconds(500))
                visible = true
            }
    }
}

/// D1: the note type changed elsewhere while its draft is open.
struct ChangedElsewhereRow: View {
    let model: NoteTypesModel
    let id: Int64

    var body: some View {
        HStack(spacing: Spacing.small) {
            Label {
                Text("noteTypes.changed.message", bundle: .module)
            } icon: {
                Image(systemName: "exclamationmark.triangle").foregroundStyle(.orange)
            }
            Spacer(minLength: Spacing.small)
            Button { model.keepEditing(id) } label: { Text("noteTypes.changed.keepEditing", bundle: .module) }
                .accessibilityIdentifier("noteTypes.list.changed.keepEditing")
            Button { model.reload(id) } label: { Text("noteTypes.changed.reload", bundle: .module) }
                .accessibilityIdentifier("noteTypes.list.changed.reload")
        }
        .padding(.horizontal, Spacing.medium)
        .padding(.vertical, Spacing.xSmall)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("noteTypes.list.changed")
    }
}

// MARK: The list (W2)

private struct NoteTypeListColumn: View {
    @Bindable var model: NoteTypesModel

    var body: some View {
        VStack(spacing: .zero) {
            FilterField(text: $model.filter, prompt: NoteTypesStrings.text("noteTypes.list.filter"))
                .padding(.horizontal, Spacing.xSmall)
                .padding(.vertical, Spacing.xSmall)
                .accessibilityIdentifier("noteTypes.list.filter")
            list
        }
    }

    private var list: some View {
        List(selection: $model.selection) {
            ForEach(model.filteredItems) { item in
                NoteTypeListRow(item: item, edited: model.isEdited(item.id))
                    .tag(item.id)
            }
        }
        .contextMenu(forSelectionType: Int64.self) { rows in
            if rows.count == 1, let id = rows.first { NoteTypeDeletionMenu(model: model, id: id) }
        }
        // A content list beside the app's sidebar (N22.1): the window's own
        // background, as the filter field above it, with the system selection.
        // Only the app's navigation sidebar uses sidebar material.
        #if os(macOS)
        .listStyle(.inset)
        #else
        .listStyle(.plain)
        #endif
        .scrollContentBackground(.hidden)
        .accessibilityIdentifier("noteTypes.list.table")
        .overlay {
            switch model.phase {
            case .loading:
                DelayedProgress()
            case .noCollection:
                // Nothing failed: there is no collection to read yet (N18.3's welcome is on Today).
                ContentUnavailableView {
                    Label { Text("noteTypes.list.noCollection.title", bundle: .module) } icon: { Image(systemName: "rectangle.stack") }
                } description: {
                    Text("noteTypes.list.noCollection.message", bundle: .module)
                }
                .accessibilityIdentifier("noteTypes.list.noCollection")
            case .failed:
                ContentUnavailableView {
                    Label { Text("noteTypes.list.failed.title", bundle: .module) } icon: { Image(systemName: "rectangle.stack") }
                } description: {
                    Text("noteTypes.list.failed.message", bundle: .module)
                } actions: {
                    Button { model.reloadList() } label: { Text("noteTypes.tryAgain", bundle: .module) }
                }
            case .ready where model.filteredItems.isEmpty && !model.items.isEmpty:
                ContentUnavailableView.search(text: model.filter)
            default:
                EmptyView()
            }
        }
    }
}

struct NoteTypeListRow: View {
    let item: NoteTypesModel.Item
    let edited: Bool

    var body: some View {
        HStack(spacing: Spacing.xSmall) {
            Text(verbatim: item.name)
                .lineLimit(1)
            if edited {
                Text("noteTypes.list.edited", bundle: .module)
                    .foregroundStyle(.secondary)
                    .fixedSize()
                    .accessibilityIdentifier("noteTypes.list.edited.\(item.id)")
            }
            Spacer(minLength: Spacing.xSmall)
            if let notes = item.notes {
                Text(notes, format: .number)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
                    // The count is never cut; the name truncates first.
                    .fixedSize()
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("noteTypes.list.row.\(item.id)")
    }
}
