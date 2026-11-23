import AppModel
import CoreBridge
import SwiftUI

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
        content
            .navigationTitle(Text("browse.title", bundle: .module))
            .toolbar {
                #if os(macOS)
                // N21.3: the Cards / Notes control right after the title.
                ToolbarItem(placement: .navigation) { modePicker }
                // N21.3, N23.6: the trailing group, in this order: the sync button,
                // the search field, the Add menu. U273's inspector toggle
                // (`sidebar.trailing`) goes last, after the Add menu.
                if let syncStatusItem { ToolbarItem(placement: .primaryAction) { syncStatusItem } }
                DefaultToolbarItem(kind: .search, placement: .primaryAction)
                if let addMenu { ToolbarItem(placement: .primaryAction) { addMenu } }
                #else
                ToolbarItem(placement: .principal) { modePicker }
                #endif
            }
            .browseSearchField(model)
            .modifier(BrowseCommandFeedback(model: model))
            #if os(macOS)
            .background { BrowseSelectAllKey(commands: model.commands) }
            #endif
            .onChange(of: formatterKey) { setFormatter() }
            .onAppear {
                setFormatter()
                model.appear()
            }
    }

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

extension BrowseModel {
    /// The table's cells take the device's locale and time zone; set before
    /// the first search, so its rows never show unformatted.
    func useFormatter(locale: Locale, timeZone: TimeZone, clock: AppClock) {
        let formatter = BrowseFormatter(locale: locale, timeZone: timeZone, now: { clock.now() })
        results.setPresenter(formatter, key: formatter.key)
    }
}

#if os(macOS)
/// N21.8: ⌥⌘A is Select All anywhere in Browse (⌘A is the system's Select
/// All, which selects every row while the table has the focus): a key
/// equivalent of the window with no menu item of its own, as Find…'s ⇧⌘O.
private struct BrowseSelectAllKey: View {
    let commands: BrowseCommands

    var body: some View {
        Button { commands.selectAll() } label: { EmptyView() }
            .keyboardShortcut("a", modifiers: [.command, .option])
            .disabled(!commands.canSelectAll)
            .opacity(0)
            .allowsHitTesting(false)
            .accessibilityHidden(true)
    }
}
#endif
