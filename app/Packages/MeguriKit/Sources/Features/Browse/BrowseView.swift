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
                    // Weak references: the inspector owns this factory and the hosts it makes.
                    // When it has gone the page edits on, with no media.
                    model.inspector.makeRuntime = { [weak inspector = model.inspector] draft, current in
                        let host = try FieldEditorHost(draft: draft, isCurrent: current)
                        // `browser/inspector` I3: a row change opens the note with no field focused.
                        host.browsing = true
                        // Option A: the fields' thumbnails and sound chips.
                        host.mediaDirectory = inspector?.mediaDirectory
                        host.onPlayMedia = { [weak inspector] in inspector?.playSound($0) }
                        host.onOpenMedia = { [weak inspector] name in
                            if let url = inspector?.mediaFile(name) { NSWorkspace.shared.open(url) }
                        }
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
/// N21.8: ⌥⌘A is Select All anywhere in Browse: a key of the window with no
/// menu item of its own, as Find…'s ⇧⌘O. (⌘A in the table is the menu bar's
/// Select All, which reaches the table once it has the keys.)
private struct BrowseKeys: View {
    let model: BrowseModel
    @State private var focus = BrowseClickFocus()
    #if DEBUG
    /// The UI suites read which view has the keys after the last click.
    @State private var responder = ""
    #endif

    var body: some View {
        ZStack {
            Button { model.commands.selectAll() } label: { EmptyView() }
                .keyboardShortcut("a", modifiers: [.command, .option])
                .disabled(!model.commands.canSelectAll)
                .opacity(0)
                .allowsHitTesting(false)
                .accessibilityHidden(true)
            #if DEBUG
            Text(verbatim: responder)
                .frame(width: 1, height: 1)
                .opacity(0.01)
                .accessibilityIdentifier("browse.debug.responder")
            Text(verbatim: model.results.scrollTrace.joined(separator: " | "))
                .frame(width: 1, height: 1)
                .opacity(0.01)
                .accessibilityIdentifier("browse.debug.scroll")
            #endif
        }
        .onAppear {
            #if DEBUG
            focus.onFocus = { responder = $0 }
            #endif
            focus.start(model: model)
        }
        .onDisappear { focus.stop() }
        #if DEBUG
        .onChange(of: model.results.scrollTrace) { _, steps in responder = "trace: " + steps.joined(separator: " | ") }
        #endif
    }
}

/// A click in the results table or the sidebar's list gives that view the
/// keys. SwiftUI left them with the window's hosting view (and so with the
/// sidebar) after a click on a table row (VM, 2026-11-23): Return, the
/// arrows, ⌘A and ⌘⌫ never reached the table. With the table the first
/// responder, its own keys, the menu bar's Select All and Delete Note, and
/// its primary action (Return opens the inspector, N21.3) all work natively.
/// Exception to the native contract, for that functional need: a local
/// monitor of mouse-downs (never consumed) and of ⌘⌫ (below), live only
/// while Browse shows.
@MainActor private final class BrowseClickFocus {
    private var token: Any?
    var onFocus: ((String) -> Void)?

    func start(model: BrowseModel) {
        stop()
        let results = model.results
        token = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown, .keyDown]) { [weak self, weak results, weak model] event in
            // Local monitors run on the main thread; the view is focused once
            // the click itself has been handled.
            if event.type == .keyDown {
                return MainActor.assumeIsolated { Self.deletesNotes(event, model) } ? nil : event
            }
            MainActor.assumeIsolated {
                // The view under the click (the window's frame view hit-tests in window
                // coordinates, the toolbar included), then the table view holding it: a
                // click in the toolbar's search field over the table is not the table's.
                guard let window = event.window, window.attachedSheet == nil,
                      let frame = window.contentView?.superview ?? window.contentView,
                      var view: NSView = frame.hitTest(event.locationInWindow) else { return }
                while !(view is NSTableView), let parent = view.superview { view = parent }
                guard let view = view as? NSTableView else { return }
                let isTable = view.headerView != nil
                DispatchQueue.main.async { [weak self, weak results] in
                    MainActor.assumeIsolated {
                        if window.firstResponder !== view { window.makeFirstResponder(view) }
                        results?.tableFocused = isTable
                        self?.onFocus?(Self.describe(window.firstResponder))
                    }
                }
            }
            return event
        }
    }

    /// ⌘⌫ while the results table has the keys: Note › Delete Note (N21.8).
    /// With the table the first responder and the menu item enabled, the key
    /// still did not reach the menu item in the VM (2026-11-23), so it is
    /// taken here; ⌘A, Return and the arrows take their native paths.
    private static func deletesNotes(_ event: NSEvent, _ model: BrowseModel?) -> Bool {
        guard let model, let window = event.window, window.attachedSheet == nil, event.keyCode == 51,
              event.modifierFlags.intersection([.command, .option, .shift, .control]) == .command,
              BrowseResponder.tableIsFirst(window.firstResponder), model.commands.state(.delete).enabled else { return false }
        model.commands.perform(.delete)
        return true
    }

    func stop() {
        if let token { NSEvent.removeMonitor(token) }
        token = nil
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

    /// Every table view with a column header in `root`.
    public static func resultsTables(in root: NSView) -> [NSTableView] {
        var tables: [NSTableView] = []
        func collect(_ view: NSView) {
            if let table = view as? NSTableView, table.headerView != nil { tables.append(table) }
            view.subviews.forEach(collect)
        }
        collect(root)
        return tables
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
