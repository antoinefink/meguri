import AppModel
import CardInfo
import CoreBridge
import DesignSystem
import SwiftUI

/// The inspector's text (`browser/workspace` V7, V12, V16).
enum BrowseInspectorText {
    static func counted(_ key: StaticString, _ count: UInt64, locale: Locale = .current) -> String {
        let n = Int(clamping: count), shown = n.formatted(.number.locale(locale))
        return String(localized: key, defaultValue: "\(n) \(shown)", bundle: .module, locale: locale)
    }

    /// V7: "12 Cards Selected", "3 Notes Selected".
    static func title(_ summary: BrowseSelectionSummary, locale: Locale = .current) -> String {
        summary.mode == .cards ? counted("browse.inspector.cardsSelected", UInt64(summary.rows), locale: locale)
            : counted("browse.inspector.notesSelected", UInt64(summary.rows), locale: locale)
    }

    /// V7's one line: "<k> notes" (Cards only), the deck or "<d> decks",
    /// "<s> suspended" when s > 0, joined with " · ".
    static func line(_ summary: BrowseSelectionSummary, locale: Locale = .current) -> String {
        var parts: [String] = []
        if let notes = summary.notes { parts.append(counted("browse.inspector.notes", notes, locale: locale)) }
        if let deck = summary.deck { parts.append(deck.components(separatedBy: "::").joined(separator: " › ")) }
        else if let decks = summary.decks { parts.append(counted("browse.inspector.decks", decks, locale: locale)) }
        if let suspended = summary.suspended, suspended > 0 { parts.append(counted("browse.inspector.suspended", suspended, locale: locale)) }
        return parts.joined(separator: " · ")
    }

    /// V12: "Card 1 of 3".
    static func card(_ index: Int, of count: Int, locale: Locale = .current) -> String {
        let shown = { (n: Int) in n.formatted(.number.locale(locale)) }
        return String(localized: "browse.inspector.cardOf", defaultValue: "\(shown(index + 1)) \(shown(count))", bundle: .module,
                      locale: locale)
    }

    /// N21.5: each command's key at the trailing edge of its button.
    static func key(_ command: BrowseCommand) -> String? {
        switch command {
        case .changeDeck: "⌘D"
        case .addTags: "⇧⌘A"
        case .removeTags: "⌥⇧⌘A"
        case .suspend: "⇧⌘J"
        case .bury: "⌘J"
        case .setDueDate: "⇧⌘D"
        case .forget: "⌥⌘N"
        case .changeNoteType: "⇧⌘M"
        case .findReplace: "⌥⌘F"
        case .delete: "⌘⌫"
        case .flag, .unsuspend, .unbury, .editNoteType, .findDuplicates, .editNote: nil
        }
    }
}

#if os(macOS)
/// The trailing pane (N21.4, N21.5; `browser/workspace` V7, V13): "No
/// Selection", the summary of several rows with their commands, or the one
/// row's Note tab or Info tab, over a bottom bar with the Note | Info control
/// at its trailing edge (owner decision 2026-11-25). The two tabs show no row
/// and several rows alike; the tab chosen is kept on the device.
struct BrowseInspectorPane: View {
    let model: BrowseModel

    private var inspector: BrowseInspector { model.inspector }

    var body: some View {
        VStack(spacing: 0) {
            content
            Divider()
            bar
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .onChange(of: inspector.infoCard, initial: true) { inspector.followCardInfo() }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("browse.inspector")
    }

    /// The note whose fields the formatting controls act on: one row, the
    /// Note tab, an editor that could be shown.
    private var formattedNote: BrowseInspectorNote? {
        guard inspector.content == .note, inspector.tab == .note, let note = inspector.note, note.error == nil else { return nil }
        return note
    }

    /// The pane's bottom bar: the formatting controls at the leading edge
    /// while a note is edited, Note | Info at the trailing edge in every state.
    private var bar: some View {
        HStack(spacing: Spacing.small) {
            if let note = formattedNote { BrowseFormattingBar(note: note) }
            Spacer(minLength: 0)
            tabs
        }
        .padding(.horizontal)
        .padding(.vertical, Spacing.xSmall)
    }

    /// N21.4: Note | Info.
    private var tabs: some View {
        Picker(selection: Binding(get: { inspector.tab }, set: { inspector.tab = $0 })) {
            Text("browse.inspector.tab.note", bundle: .module).tag(BrowseInspectorTab.note)
            Text("browse.inspector.tab.info", bundle: .module).tag(BrowseInspectorTab.info)
        } label: {
            Text("browse.inspector.tab.label", bundle: .module)
        }
        .pickerStyle(.segmented)
        .labelsHidden()
        .fixedSize()
        .accessibilityIdentifier("browse.inspector.tabs")
    }

    @ViewBuilder private var content: some View {
        Group {
            switch inspector.content {
            case .noSelection:
                if inspector.loading { loadingView } else { unavailable(Text("browse.inspector.noSelection", bundle: .module)) }
            case .unavailable:
                unavailable(Text(inspector.notice == nil ? "browse.inspector.unavailable" : "browse.inspector.noSelection", bundle: .module))
            case .several:
                if let summary = inspector.summary {
                    BrowseSelectionSummaryView(model: model, summary: summary)
                } else {
                    Color.clear
                }
            case .note:
                if inspector.tab == .info {
                    CardInfoView(model: inspector.cardInfo)
                } else if let note = inspector.note {
                    BrowseNoteTab(model: model, note: note)
                } else if inspector.loading {
                    loadingView
                } else {
                    unavailable(Text("browse.inspector.noSelection", bundle: .module))
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var loadingView: some View {
        ProgressView().controlSize(.small).accessibilityIdentifier("browse.inspector.loading")
    }

    /// V7's "No Selection" in its content-unavailable style; W5.4's notice under it.
    private func unavailable(_ title: Text) -> some View {
        ContentUnavailableView {
            title
        } description: {
            if inspector.notice != nil { Text("browse.inspector.noteDeleted", bundle: .module) }
        }
        .accessibilityIdentifier("browse.inspector.empty")
    }
}

/// N21.5: several rows: the title, one secondary line, then V8's commands as
/// plain buttons in one column, each with its key at the trailing edge,
/// grouped as the row's context menu groups them, Delete last in red.
struct BrowseSelectionSummaryView: View {
    let model: BrowseModel
    let summary: BrowseSelectionSummary
    @Environment(\.locale) private var locale

    private var commands: BrowseCommands { model.commands }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                Text(verbatim: BrowseInspectorText.title(summary, locale: locale))
                    .font(.title3.weight(.semibold))
                    .accessibilityIdentifier("browse.inspector.summary.title")
                let line = BrowseInspectorText.line(summary, locale: locale)
                if !line.isEmpty {
                    Text(verbatim: line)
                        .foregroundStyle(.secondary)
                        .padding(.top, Spacing.xxSmall)
                        .accessibilityIdentifier("browse.inspector.summary.line")
                }
                VStack(alignment: .leading, spacing: 0) {
                    group([.changeDeck, .addTags, .removeTags])
                    Divider()
                    flag
                    group([.suspend, .unsuspend, .bury, .unbury])
                    Divider()
                    group([.setDueDate, .forget])
                    Divider()
                    group([.changeNoteType, .editNoteType])
                    Divider()
                    group([.findReplace, .findDuplicates])
                    Divider()
                    group([.delete])
                }
                .padding(.top, Spacing.small)
            }
            .padding()
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .accessibilityIdentifier("browse.inspector.summary")
    }

    @ViewBuilder private func group(_ list: [BrowseCommand]) -> some View {
        ForEach(list, id: \.self) { command in
            let state = commands.state(command)
            if state.shown { row(command, state: state) }
        }
    }

    private func row(_ command: BrowseCommand, state: BrowseCommandState) -> some View {
        Button(role: command == .delete ? .destructive : nil) { commands.perform(command) } label: {
            HStack {
                // One rule with the menus: a disabled command reads disabled. An explicit
                // primary style kept Find Duplicates… (no key beside it) looking enabled.
                Text(verbatim: BrowseCommandText.title(command, state, locale: locale))
                    .foregroundStyle(Self.titleStyle(command, enabled: state.enabled))
                Spacer(minLength: Spacing.small)
                if let key = BrowseInspectorText.key(command) { Text(verbatim: key).foregroundStyle(.secondary) }
            }
            .padding(.vertical, Spacing.xxSmall)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(!state.enabled)
        .help(BrowseCommandText.disabledHelp(command, state, commands: commands, locale: locale))
        .accessibilityIdentifier("browse.inspector.command.\(BrowseCommandIdentifier.of(command))")
    }

    private static func titleStyle(_ command: BrowseCommand, enabled: Bool) -> AnyShapeStyle {
        switch (command == .delete, enabled) {
        case (true, true): AnyShapeStyle(.red)
        case (true, false): AnyShapeStyle(Color.red.opacity(0.4))
        case (false, true): AnyShapeStyle(.primary)
        case (false, false): AnyShapeStyle(.tertiary)
        }
    }

    /// Flag ▸ with No Flag and the seven flags, each in its colour.
    private var flag: some View {
        Menu {
            ForEach(Array(UInt8(0)...7), id: \.self) { flag in
                Button { commands.perform(.flag(flag)) } label: {
                    if flag == 0 {
                        Text(verbatim: BrowseCommandText.flag(0, names: model.flagNames, locale: locale))
                    } else {
                        Label { Text(verbatim: BrowseCommandText.flag(flag, names: model.flagNames, locale: locale)) }
                            icon: { BrowseFlagSymbol(flag: flag) }
                    }
                }
                .disabled(!commands.state(.flag(flag)).enabled)
                if flag == 0 { Divider() }
            }
        } label: {
            HStack {
                Text(verbatim: BrowseCommandText.title(.flag(1), commands.state(.flag(1)), locale: locale))
                Spacer(minLength: Spacing.small)
                Image(systemName: "chevron.right").foregroundStyle(.secondary).imageScale(.small)
            }
            .padding(.vertical, Spacing.xxSmall)
            .contentShape(Rectangle())
        }
        .menuStyle(.button)
        .buttonStyle(.plain)
        .menuIndicator(.hidden)
        .accessibilityIdentifier("browse.inspector.command.flag")
    }
}

#endif

/// The identifiers of the commands' controls (`browse.<place>.<command>`).
enum BrowseCommandIdentifier {
    static func of(_ command: BrowseCommand) -> String {
        switch command {
        case .changeDeck: "changeDeck"
        case .addTags: "addTags"
        case .removeTags: "removeTags"
        case .flag(let flag): "flag.\(flag)"
        case .suspend: "suspend"
        case .unsuspend: "unsuspend"
        case .bury: "bury"
        case .unbury: "unbury"
        case .setDueDate: "setDueDate"
        case .forget: "forget"
        case .changeNoteType: "changeNoteType"
        case .editNoteType: "editNoteType"
        case .findReplace: "findReplace"
        case .findDuplicates: "findDuplicates"
        case .delete: "delete"
        case .editNote: "editNote"
        }
    }
}
