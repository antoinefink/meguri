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
                // then Cards / Notes as a secondary action, which macOS draws after
                // the title (a navigation item stands before it); then the sync
                // button, the Add menu and the inspector toggle. macOS draws the
                // search field last (see BrowseSearchField's placement).
                ToolbarItem(placement: .secondaryAction) { modePicker }
                if let syncStatusItem { ToolbarItem(placement: .primaryAction) { syncStatusItem } }
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
    #if DEBUG
    /// The UI suites read which view had the keys at the last key press.
    @State private var responder = ""
    #endif

    var body: some View {
        ZStack {
            Color.clear.accessibilityHidden(true)
            #if DEBUG
            Text(verbatim: responder)
                .frame(width: 1, height: 1)
                .opacity(0.01)
                .accessibilityIdentifier("browse.debug.responder")
            #endif
        }
        .onAppear {
            #if DEBUG
            monitor.onKey = { responder = $0 }
            #endif
            monitor.start(model: model)
        }
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
    var onKey: ((String) -> Void)?
    /// Where the last click in Browse's window landed. SwiftUI keeps the
    /// window's hosting view as the first responder after a click on the
    /// table or the sidebar (VM, 2026-11-23), so the keys follow the click.
    private var clicked: Area = .elsewhere
    private enum Area { case table, sidebar, elsewhere }

    func start(model: BrowseModel) {
        stop()
        token = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .leftMouseDown, .rightMouseDown]) { [weak self, weak model] event in
            // Local monitors run on the main thread.
            MainActor.assumeIsolated {
                guard let self, let model else { return false }
                if event.type != .keyDown {
                    self.clicked = Self.area(of: event)
                    model.results.tableFocused = self.clicked == .table
                    return false
                }
                self.onKey?(Self.describe(event.window?.firstResponder) + " · clicked \(self.clicked)")
                return self.handles(event, model)
            } ? nil : event
        }
    }

    private static func area(of event: NSEvent) -> Area {
        guard let root = event.window?.contentView else { return .elsewhere }
        let point = event.locationInWindow
        func hit(_ view: NSView?) -> Bool {
            guard let frame = (view?.enclosingScrollView ?? view).map({ $0.convert($0.bounds, to: nil) }) else { return false }
            return frame.contains(point)
        }
        if hit(BrowseResponder.resultsTable(in: root)) { return .table }
        if hit(BrowseResponder.sidebarList(in: root)) { return .sidebar }
        return .elsewhere
    }

    /// The keys belong to the window rather than a view inside it.
    private static func windowHasKeys(_ window: NSWindow) -> Bool {
        guard let responder = window.firstResponder else { return true }
        return responder === window || responder === window.contentView
    }

    /// The first responder's class and its first enclosing views' classes.
    private static func describe(_ responder: NSResponder?) -> String {
        var names = [responder.map { String(describing: type(of: $0)) } ?? "nil"]
        var view = (responder as? NSView)?.superview
        while let current = view, names.count < 6 {
            names.append(String(describing: type(of: current)))
            view = current.superview
        }
        return names.joined(separator: " < ")
    }

    func stop() {
        if let token { NSEvent.removeMonitor(token) }
        token = nil
    }

    private func handles(_ event: NSEvent, _ model: BrowseModel) -> Bool {
        guard let window = event.window, window.sheetParent == nil, window.attachedSheet == nil else { return false }
        let flags = event.modifierFlags.intersection([.command, .option, .shift, .control])
        let key = event.charactersIgnoringModifiers?.lowercased()
        if key == "a" {
            let inTable = BrowseResponder.tableIsFirst(window.firstResponder)
                || (clicked == .table && Self.windowHasKeys(window))
            let take = flags == [.command, .option] || (flags == .command && inTable)
            guard take, model.commands.canSelectAll else { return false }
            model.commands.selectAll()
            return true
        }
        // ⌘⌫ after a click in the table: Note › Delete Note (N21.8). The menu
        // item's key did not reach it while the window's hosting view had the keys.
        if flags == .command, event.keyCode == 51, clicked == .table, Self.windowHasKeys(window),
           model.commands.state(.delete).enabled {
            model.commands.perform(.delete)
            return true
        }
        // Return or Space on the sidebar's one selected filter row: a second click (T9).
        // The list draws that row selected beside Browse's: a row folded away or
        // filtered out is not acted on.
        let sidebar = BrowseResponder.sidebar(window.firstResponder)
            ?? (clicked == .sidebar && Self.windowHasKeys(window) ? window.contentView.flatMap(BrowseResponder.sidebarList(in:)) : nil)
        if flags.isEmpty, key == "\r" || key == " ", (sidebar?.numberOfSelectedRows ?? 0) >= 2,
           model.selectedRows.count == 1, let row = model.selectedRows.first, model.activationRemoves(row) {
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
        enclosing(responder, NSTableView.self).map { $0.accessibilityIdentifier() == "browse.results.table" || $0.headerView != nil } ?? false
    }

    /// The sidebar's list (an outline view with no column header), when it has the keys.
    public static func sidebar(_ responder: NSResponder?) -> NSOutlineView? {
        enclosing(responder, NSOutlineView.self).flatMap { $0.accessibilityIdentifier() == "browse.sidebar.list" || $0.headerView == nil ? $0 : nil }
    }

    /// The results table in `root`: the table view with a column header.
    public static func resultsTable(in root: NSView) -> NSTableView? {
        first(in: root) { ($0 as? NSTableView)?.headerView != nil }.flatMap { $0 as? NSTableView }
    }

    /// The sidebar's list in `root`: the outline view with no column header.
    public static func sidebarList(in root: NSView) -> NSOutlineView? {
        first(in: root) { ($0 as? NSOutlineView).map { $0.headerView == nil } ?? false }.flatMap { $0 as? NSOutlineView }
    }

    private static func first(in view: NSView, where test: (NSView) -> Bool) -> NSView? {
        if test(view) { return view }
        for child in view.subviews {
            if let found = first(in: child, where: test) { return found }
        }
        return nil
    }

    /// The table view holding the first responder: SwiftUI may make a view
    /// inside the table, not the table itself, the first responder (VM,
    /// 2026-11-23). A text input never counts: it keeps its own keys.
    private static func enclosing<T: NSTableView>(_ responder: NSResponder?, _ type: T.Type) -> T? {
        guard !(responder is NSText) else { return nil }
        var view = responder as? NSView
        while let current = view {
            if let table = current as? T { return table }
            view = current.superview
        }
        return nil
    }
}
#endif
