import AppModel
import CoreBridge
import SwiftUI

/// Browse's detail beside its sidebar (N21.1, N21.3): the window title
/// "Browse", the Cards / Notes control in the toolbar, the results table and
/// its status line, or the empty and error states (`docs/design/
/// screen-inventory.yaml` `browse.browser`). The search field (U271) and the
/// inspector (U273) join the toolbar later.
public struct BrowseView: View {
    private let model: BrowseModel
    private let addNote: () -> Void
    private let editNoteType: (Int64) -> Void
    @Environment(\.locale) private var locale
    @Environment(\.timeZone) private var timeZone
    @Environment(\.appClock) private var clock

    /// `addNote` opens Add (the empty collection's action); `editNoteType`
    /// opens Note Types on a type (V2.2, `notetypes/workspace` W14).
    public init(model: BrowseModel, addNote: @escaping () -> Void = {}, editNoteType: @escaping (Int64) -> Void = { _ in }) {
        self.model = model
        self.addNote = addNote
        self.editNoteType = editNoteType
    }

    private var results: BrowseResults { model.results }

    public var body: some View {
        content
            .navigationTitle(Text("browse.title", bundle: .module))
            .toolbar {
                ToolbarItem(placement: .principal) { modePicker }
            }
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
                BrowseTable(results: results)
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
            .accessibilityIdentifier("browse.empty.collection")
        case .noResults:
            ContentUnavailableView {
                Label { Text(mode == .notes ? "browse.empty.notes" : "browse.empty.cards", bundle: .module) }
                    icon: { Image(systemName: "magnifyingglass") }
            } actions: {
                Button { model.clearQuery() } label: { Text("browse.empty.clear", bundle: .module) }
                    .accessibilityIdentifier("browse.empty.clear")
            }
            .accessibilityIdentifier("browse.empty.noResults")
        case .failed:
            ContentUnavailableView {
                Label { Text(mode == .notes ? "browse.failed.notes" : "browse.failed.cards", bundle: .module) }
                    icon: { Image(systemName: "exclamationmark.triangle") }
            } actions: {
                Button { results.retry() } label: { Text("browse.failed.tryAgain", bundle: .module) }
                    .accessibilityIdentifier("browse.failed.tryAgain")
            }
            .accessibilityIdentifier("browse.failed")
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
