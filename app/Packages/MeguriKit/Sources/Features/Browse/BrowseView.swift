import AppModel
import CoreBridge
import DesignSystem
import Editor
import SwiftUI
#if os(macOS)
import AppKit
#endif

/// Browse's detail beside its sidebar (N21.1, N21.3): the window title
/// "Browse", the Cards / Notes control right after it, then the trailing
/// group (N21.3, N23.6): the shell's sync button, the search field (U271,
/// N21.2), the Add menu and, when U273 adds it, the inspector toggle last;
/// the results table and its status line, or the empty and error states
/// (`docs/design/screen-inventory.yaml` `browse.browser`). The commands of
/// U275 act on the table's rows: its context menu, the menu bar's (N21.8),
/// the Delete question and VoiceOver's results.
public struct BrowseView: View {
    private let model: BrowseModel
    private let addNote: () -> Void
    private let editNoteType: (Int64) -> Void
    private let addMenu: AnyView?
    @Environment(\.locale) private var locale
    @Environment(\.timeZone) private var timeZone
    @Environment(\.appClock) private var clock
    @Environment(\.syncStatusItem) private var syncStatusItem

    /// `addNote` opens Add (the empty collection's action); `editNoteType`
    /// opens Note Types on a type (V2.2, `notetypes/workspace` W14);
    /// `addMenu` is round 6's Add menu, for the toolbar (N21.3).
    public init(model: BrowseModel, addNote: @escaping () -> Void = {}, editNoteType: @escaping (Int64) -> Void = { _ in },
                addMenu: AnyView? = nil) {
        self.model = model
        self.addNote = addNote
        self.editNoteType = editNoteType
        self.addMenu = addMenu
    }

    private var results: BrowseResults { model.results }

    public var body: some View {
        detail
            .navigationTitle(Text("browse.title", bundle: .module))
            .toolbar {
                #if os(macOS)
                // N21.3, N23.6 (VM probe of every placement, 2026-11-23): the title,
                // then Cards / Notes and the sync button as secondary actions, which
                // macOS draws after the title and before the principal search field;
                // the Add menu and the inspector toggle trail it. A navigation item
                // stands before the title; a primary one after the search field.
                ToolbarItem(placement: .secondaryAction) { modePicker }
                if let syncStatusItem { ToolbarItem(placement: .secondaryAction) { syncStatusItem } }
                if let addMenu { ToolbarItem(placement: .primaryAction) { addMenu } }
                // N21.3: the inspector toggle, last.
                ToolbarItem(placement: .primaryAction) { inspectorToggle }
                #else
                ToolbarItem(placement: .principal) { modePicker }
                #endif
            }
            .browseSearchField(model)
            .modifier(BrowseCommandFeedback(model: model))
            #if os(macOS)
            .background { BrowseKeys(model: model) }
            #endif
            .onChange(of: formatterKey) { setFormatter() }
            .onAppear {
                setFormatter()
                #if os(macOS)
                if model.inspector.makeRuntime == nil {
                    model.inspector.makeRuntime = { draft, current in
                        let host = try FieldEditorHost(draft: draft, isCurrent: current)
                        // `browser/inspector` I3: a row change opens the note with no field focused.
                        host.browsing = true
                        return host
                    }
                }
                #endif
                model.appear()
            }
    }

    /// N21.3, N21.4: the table and its status line with the inspector
    /// beside them; under about 900 points the inspector hides itself, and
    /// Return or a double click shows it over the table.
    @ViewBuilder private var detail: some View {
        #if os(macOS)
        content
            .overlay(alignment: .trailing) {
                if inspector.narrow, inspector.overlay {
                    HStack(spacing: 0) {
                        Divider()
                        BrowseInspectorPane(model: model)
                            .frame(width: BrowseViewMetrics.overlayWidth)
                            .background(.background)
                    }
                    .onExitCommand { inspector.toggle() }
                }
            }
            .inspector(isPresented: Binding(get: { !inspector.narrow && inspector.presented },
                                            set: { if !inspector.narrow { inspector.presented = $0 } })) {
                BrowseInspectorPane(model: model)
                    .inspectorColumnWidth(min: BrowseViewMetrics.inspectorMinimum, ideal: BrowseViewMetrics.overlayWidth,
                                          max: BrowseViewMetrics.inspectorMaximum)
            }
            .onGeometryChange(for: Double.self) { Double($0.frame(in: .global).maxX) } action: { inspector.widthChanged($0) }
        #else
        content
        #endif
    }

    private var inspector: BrowseInspector { model.inspector }

    #if os(macOS)
    /// N21.3: `sidebar.trailing`, after the Add menu.
    private var inspectorToggle: some View {
        Button { inspector.toggle() } label: {
            Label { Text("browse.inspector.toggle", bundle: .module) } icon: { Image(systemName: "sidebar.trailing") }
        }
        .help(Text(inspector.shown ? "browse.inspector.hide" : "browse.inspector.show", bundle: .module))
        .accessibilityIdentifier("browse.toolbar.inspector")
    }
    #endif

    @ViewBuilder private var content: some View {
        switch results.content {
        case .starting, .table:
            VStack(spacing: 0) {
                BrowseTable(model: model)
                Divider()
                BrowseStatusLine(model: model, editNoteType: editNoteType)
            }
        case .emptyCollection:
            ContentUnavailableView {
                Label { Text(mode == .notes ? "browse.empty.notes" : "browse.empty.cards", bundle: .module) }
                    icon: { Image(systemName: "rectangle.stack") }
            } actions: {
                Button { addNote() } label: { Text("browse.empty.add", bundle: .module) }
                    .accessibilityIdentifier("browse.empty.add")
            }
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("browse.empty.collection")
        case .noResults:
            ContentUnavailableView {
                Label { Text(mode == .notes ? "browse.empty.notes" : "browse.empty.cards", bundle: .module) }
                    icon: { Image(systemName: "magnifyingglass") }
            } actions: {
                Button { model.clearQuery() } label: { Text("browse.empty.clear", bundle: .module) }
                    .accessibilityIdentifier("browse.empty.clear")
            }
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("browse.empty.noResults")
        case .failed:
            ContentUnavailableView {
                Label { Text(mode == .notes ? "browse.failed.notes" : "browse.failed.cards", bundle: .module) }
                    icon: { Image(systemName: "exclamationmark.triangle") }
            } actions: {
                Button { results.retry() } label: { Text("browse.failed.tryAgain", bundle: .module) }
                    .accessibilityIdentifier("browse.failed.tryAgain")
            }
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("browse.results.failed")
        }
    }

    private var mode: BrowseMode { model.mode }

    /// V2.1: Cards / Notes.
    private var modePicker: some View {
        Picker(selection: Binding(get: { model.mode }, set: { model.setMode($0) })) {
            Text("browse.mode.cards", bundle: .module).tag(BrowseMode.cards)
            Text("browse.mode.notes", bundle: .module).tag(BrowseMode.notes)
        } label: {
            Text("browse.mode.label", bundle: .module)
        }
        .pickerStyle(.segmented)
        .labelsHidden()
        .fixedSize()
        .accessibilityIdentifier("browse.toolbar.mode")
    }

    private var formatterKey: String {
        "\(locale.identifier)|\(timeZone.identifier)"
    }

    private func setFormatter() {
        model.useFormatter(locale: locale, timeZone: timeZone, clock: clock)
    }
}

enum BrowseViewMetrics {
    /// The inspector's width beside the table and over it (N21.4).
    static let overlayWidth: CGFloat = 320
    static let inspectorMinimum: CGFloat = 280
    static let inspectorMaximum: CGFloat = 460
}

extension BrowseModel {
    /// The table's cells take the device's locale and time zone; set before
    /// the first search, so its rows never show unformatted.
    func useFormatter(locale: Locale, timeZone: TimeZone, clock: AppClock) {
        let formatter = BrowseFormatter(locale: locale, timeZone: timeZone, now: { clock.now() })
        results.setPresenter(formatter, key: formatter.key)
    }
}

#if os(macOS)
/// N21.8: ⌥⌘A is Select All anywhere in Browse, and ⌘A in the table:
/// keys of the window with no menu item of their own, as Find…'s ⇧⌘O.
/// T9: Return or Space in the sidebar on its one selected filter row.
private struct BrowseKeys: View {
    let model: BrowseModel
    @State private var monitor = BrowseKeyMonitor()

    var body: some View {
        Color.clear
            .accessibilityHidden(true)
            .onAppear { monitor.start(model: model) }
            .onDisappear { monitor.stop() }
    }
}

/// Exception to the native contract, for a functional need (VM, 2026-11-23):
/// AppKit's Select All reached the outline view but not the SwiftUI table's
/// selection, hidden buttons' key equivalents never fired (the menu bar's
/// Select All took ⌘A), and the sidebar list's key handler never saw Return.
/// A local key monitor, live only while Browse shows, runs before them; any
/// other focus keeps its keys, and sheets keep them all.
@MainActor private final class BrowseKeyMonitor {
    private var token: Any?

    func start(model: BrowseModel) {
        stop()
        token = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak model] event in
            // Local monitors run on the main thread.
            MainActor.assumeIsolated { Self.handles(event, model) } ? nil : event
        }
    }

    func stop() {
        if let token { NSEvent.removeMonitor(token) }
        token = nil
    }

    private static func handles(_ event: NSEvent, _ model: BrowseModel?) -> Bool {
        guard let model, let window = event.window, window.sheetParent == nil, window.attachedSheet == nil else { return false }
        let flags = event.modifierFlags.intersection([.command, .option, .shift, .control])
        let key = event.charactersIgnoringModifiers?.lowercased()
        if key == "a" {
            let take = flags == [.command, .option] || (flags == .command && BrowseResponder.tableIsFirst(window.firstResponder))
            guard take, model.commands.canSelectAll else { return false }
            model.commands.selectAll()
            return true
        }
        // Return or Space on the sidebar's one selected filter row: a second click (T9).
        if flags.isEmpty, key == "\r" || key == " ", BrowseResponder.sidebarIsFirst(window.firstResponder),
           model.selectedRows.count == 1, let row = model.selectedRows.first {
            model.activate(row)
            return true
        }
        return false
    }
}
#endif

#if os(macOS)
/// Whether the results table has the keys: its outline view is the window's
/// first responder (SwiftUI gives it the table's accessibility identifier).
public enum BrowseResponder {
    public static func tableIsFirst(_ responder: NSResponder?) -> Bool {
        guard let table = responder as? NSTableView else { return false }
        // The sidebar is an outline view too, without a column header.
        return table.accessibilityIdentifier() == "browse.results.table" || table.headerView != nil
    }

    /// The sidebar's list (an outline view with no column header) has the keys.
    public static func sidebarIsFirst(_ responder: NSResponder?) -> Bool {
        guard let outline = responder as? NSOutlineView else { return false }
        return outline.accessibilityIdentifier() == "browse.sidebar.list" || outline.headerView == nil
    }
}
#endif
