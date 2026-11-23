import AppModel
import CoreBridge
import SwiftUI
#if os(macOS)
import AppKit
#endif

/// The commands' text (`browser/workspace` V8.1, V8.2, V8.3, V16), for the
/// row's context menu here and the menu bar's items (N21.8).
public enum BrowseCommandText {
    /// V8.1's title of `command` as `state` counts it.
    public static func title(_ command: BrowseCommand, _ state: BrowseCommandState, locale: Locale = .current) -> String {
        let text = { (key: String.LocalizationValue) in BrowseStrings.text(key, locale: locale) }
        /// A plural title; a count not known yet reads as plural.
        func counted(_ key: StaticString, _ count: UInt64?) -> String {
            let n = Int(clamping: count ?? 2), shown = n.formatted(.number.locale(locale))
            return String(localized: key, defaultValue: "\(n) \(shown)", bundle: .module, locale: locale)
        }
        switch command {
        case .changeDeck: return text("browse.command.changeDeck")
        case .addTags: return text("browse.command.addTags")
        case .removeTags: return text("browse.command.removeTags")
        case .flag: return text("browse.command.flag")
        case .suspend: return counted("browse.command.suspend", state.count)
        case .bury: return counted("browse.command.bury", state.count)
        case .unsuspend:
            // Disabled in the menu bar with none suspended: plain "Unsuspend" (N21.8).
            if state.count == 0 { return text("browse.command.unsuspend.none") }
            return state.count == nil ? text("browse.command.unsuspend.unknown") : counted("browse.command.unsuspend", state.count)
        case .unbury:
            if state.count == 0 { return text("browse.command.unbury.none") }
            return state.count == nil ? text("browse.command.unbury.unknown") : counted("browse.command.unbury", state.count)
        case .setDueDate: return text("browse.command.setDueDate")
        case .forget: return text("browse.command.forget")
        case .changeNoteType: return text("browse.command.changeNoteType")
        case .editNoteType: return text("browse.columns.editNoteType")
        case .findReplace: return text("browse.command.findReplace")
        case .findDuplicates: return text("browse.command.findDuplicates")
        case .editNote: return text("browse.command.editNote")
        case .delete:
            // "Delete Note", or with the "…" of its question (V8.2).
            return state.asks ? counted("browse.command.deleteAsks", state.count) : text("browse.command.delete")
        }
    }

    /// Flag ▸'s items: No Flag, then each flag by its shown name.
    public static func flag(_ flag: UInt8, names: [String], locale: Locale = .current) -> String {
        guard flag > 0 else { return BrowseStrings.text("browse.flag.none", locale: locale) }
        return names.indices.contains(Int(flag) - 1) ? names[Int(flag) - 1] : BrowseStrings.flagLabel(flag, locale: locale)
    }

    /// The flags' colours (`tags/flags-and-marked` F1).
    public static func flagColor(_ flag: UInt8) -> Color { BrowseStrings.flagColor(flag) }

    public static func selectSiblings(locale: Locale = .current) -> String {
        BrowseStrings.text("browse.command.selectSiblings", locale: locale)
    }

    // MARK: Delete's question (V8.2)

    /// "Delete 3 Notes?", "Delete “<sort field>”?" (40 characters, then
    /// "…"), or "Delete Note?" when that text is empty.
    static func deleteTitle(_ question: BrowseDeleteQuestion, locale: Locale = .current) -> String {
        if question.notes == 1, let field = question.sortField?.trimmingCharacters(in: .whitespacesAndNewlines), !field.isEmpty {
            let cut = field.count > 40 ? String(field.prefix(40)) + "…" : field
            return String(localized: "browse.delete.titleNamed", defaultValue: "\(cut)", bundle: .module, locale: locale)
        }
        let n = Int(clamping: question.notes)
        return String(localized: "browse.delete.title", defaultValue: "\(n) \(n.formatted(.number.locale(locale)))",
                      bundle: .module, locale: locale)
    }

    /// "Their 5 cards will also be deleted." (several notes; left out when
    /// each note has one card), "Its 3 cards will also be deleted." (one).
    static func deleteMessage(_ question: BrowseDeleteQuestion, locale: Locale = .current) -> String? {
        let c = Int(clamping: question.cards), shown = c.formatted(.number.locale(locale))
        if question.notes >= 2 {
            guard question.cards != question.notes else { return nil }
            return String(localized: "browse.delete.theirCards", defaultValue: "\(c) \(shown)", bundle: .module, locale: locale)
        }
        guard question.cards >= 2 else { return nil }
        return String(localized: "browse.delete.itsCards", defaultValue: "\(c) \(shown)", bundle: .module, locale: locale)
    }

    // MARK: VoiceOver (V8.3)

    /// "12 cards suspended", "3 notes deleted".
    static func announcement(_ result: BrowseAnnouncement, locale: Locale = .current) -> String {
        let n = Int(clamping: result.count), shown = n.formatted(.number.locale(locale))
        return switch result.kind {
        case .suspended: String(localized: "browse.done.suspended", defaultValue: "\(n) \(shown)", bundle: .module, locale: locale)
        case .unsuspended: String(localized: "browse.done.unsuspended", defaultValue: "\(n) \(shown)", bundle: .module, locale: locale)
        case .buried: String(localized: "browse.done.buried", defaultValue: "\(n) \(shown)", bundle: .module, locale: locale)
        case .unburied: String(localized: "browse.done.unburied", defaultValue: "\(n) \(shown)", bundle: .module, locale: locale)
        case .flagged: String(localized: "browse.done.flagged", defaultValue: "\(n) \(shown)", bundle: .module, locale: locale)
        case .unflagged: String(localized: "browse.done.unflagged", defaultValue: "\(n) \(shown)", bundle: .module, locale: locale)
        case .deleted: String(localized: "browse.done.deleted", defaultValue: "\(n) \(shown)", bundle: .module, locale: locale)
        }
    }
}

/// A flag's symbol in its colour, also in a menu, which draws plain
/// symbols in the text colour (V8.1: "each with its colour").
public struct BrowseFlagSymbol: View {
    let flag: UInt8

    public init(flag: UInt8) { self.flag = flag }

    public var body: some View {
        #if os(macOS)
        // An AppKit menu draws a template image in the text colour: the
        // symbol is drawn in its palette colour as a non-template image.
        if let image = NSImage(systemSymbolName: "flag.fill", accessibilityDescription: nil)?
            .withSymbolConfiguration(.init(paletteColors: [NSColor(BrowseStrings.flagColor(flag))])) {
            Image(nsImage: Self.untemplated(image))
        }
        #else
        Image(systemName: "flag.fill").foregroundStyle(BrowseStrings.flagColor(flag))
        #endif
    }

    #if os(macOS)
    private static func untemplated(_ image: NSImage) -> NSImage {
        image.isTemplate = false
        return image
    }
    #endif
}

/// N21.5: a row's context menu, grouped [Change Deck…, Add Tags…, Remove
/// Tags…] | [Flag ▸, Suspend, Unsuspend, Bury, Unbury] | [Set Due Date…,
/// Forget…] | [Change Note Type…, Edit Note Type…] | [Find and Replace…,
/// Find Duplicates…] | [Delete], each with its key. A command V8.1 does
/// not show for these rows is left out; one whose sheet a later unit
/// builds is there, disabled. It acts on the rows it was opened on.
struct BrowseRowMenu: View {
    let model: BrowseModel
    let rows: Set<Int>
    @Environment(\.locale) private var locale

    private var commands: BrowseCommands { model.commands }

    var body: some View {
        // The rows as a command takes them, read once, when the menu is drawn (H12, K1).
        if !rows.isEmpty, let target = commands.target(rows: rows) {
            BrowseRowMenuItems(model: model, target: target)
        }
    }
}

/// The context menu's items, on the rows it was opened on.
private struct BrowseRowMenuItems: View {
    let model: BrowseModel
    let target: BrowseTarget
    @Environment(\.locale) private var locale

    private var commands: BrowseCommands { model.commands }

    var body: some View {
        Group {
            Section {
                item(.changeDeck, key: "d", [.command])
                item(.addTags, key: "a", [.command, .shift])
                item(.removeTags, key: "a", [.command, .option, .shift])
            }
            Section {
                flagMenu
                item(.suspend, key: "j", [.command, .shift])
                item(.unsuspend)
                item(.bury, key: "j", [.command])
                item(.unbury)
            }
            Section {
                item(.setDueDate, key: "d", [.command, .shift])
                item(.forget, key: "n", [.command, .option])
            }
            Section {
                item(.changeNoteType, key: "m", [.command, .shift])
                item(.editNoteType)
            }
            Section {
                item(.findReplace, key: "f", [.command, .option])
                item(.findDuplicates)
            }
            Section {
                item(.delete, key: .delete, [.command], role: .destructive)
            }
        }
    }

    @ViewBuilder
    private func item(_ command: BrowseCommand, key: KeyEquivalent? = nil, _ modifiers: EventModifiers = [],
                      role: ButtonRole? = nil) -> some View {
        let state = commands.state(command, on: target)
        if state.shown {
            Button(role: role) { commands.perform(command, on: target) } label: {
                Text(verbatim: BrowseCommandText.title(command, state, locale: locale))
            }
            .keyboardShortcut(key.map { KeyboardShortcut($0, modifiers: modifiers) })
            .disabled(!state.enabled)
            .accessibilityIdentifier("browse.menu.\(Self.identifier(command))")
        }
    }

    /// Flag ▸: No Flag ⌘0, then the seven flags by shown name, ⌘1 to ⌘7.
    private var flagMenu: some View {
        Menu {
            ForEach(Array(UInt8(0)...7), id: \.self) { flag in
                let state = commands.state(.flag(flag), on: target)
                Button { commands.perform(.flag(flag), on: target) } label: {
                    if flag == 0 {
                        Text(verbatim: BrowseCommandText.flag(0, names: model.flagNames, locale: locale))
                    } else {
                        Label { Text(verbatim: BrowseCommandText.flag(flag, names: model.flagNames, locale: locale)) }
                            icon: { BrowseFlagSymbol(flag: flag) }
                    }
                }
                .keyboardShortcut(KeyEquivalent(Character(String(flag))), modifiers: .command)
                .disabled(!state.enabled)
                .accessibilityIdentifier("browse.menu.flag.\(flag)")
                if flag == 0 { Divider() }
            }
        } label: {
            Text(verbatim: BrowseCommandText.title(.flag(1), commands.state(.flag(1), on: target), locale: locale))
        }
        .accessibilityIdentifier("browse.menu.flag")
    }

    static func identifier(_ command: BrowseCommand) -> String {
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

/// The commands' feedback (V8.2, V8.3, N20.7): the Delete question as the
/// Mac's alert (Cancel and Delete; Delete destructive and never the
/// default, so Return deletes nothing; Esc cancels), the alert when a
/// command wrote nothing, and VoiceOver's result.
struct BrowseCommandFeedback: ViewModifier {
    let model: BrowseModel
    @Environment(\.locale) private var locale

    private var commands: BrowseCommands { model.commands }

    func body(content: Content) -> some View {
        let question = commands.deleteQuestion, locale = locale
        content
            .alert(Text(verbatim: question.map { BrowseCommandText.deleteTitle($0, locale: locale) } ?? ""),
                   isPresented: Binding(get: { commands.deleteQuestion != nil }, set: { if !$0 { commands.cancelDelete() } }),
                   presenting: question) { question in
                Button(role: .cancel) { commands.cancelDelete() } label: { Text("browse.delete.cancel", bundle: .module) }
                Button(role: .destructive) { commands.confirmDelete(question) } label: {
                    Text("browse.delete.delete", bundle: .module)
                }
            } message: { question in
                if let message = BrowseCommandText.deleteMessage(question, locale: locale) { Text(verbatim: message) }
            }
            .alert(Text("browse.failed.command.title", bundle: .module),
                   isPresented: Binding(get: { commands.failed != nil }, set: { if !$0 { commands.dismissFailure() } })) {
                Button { commands.dismissFailure() } label: { Text("browse.failed.command.ok", bundle: .module) }
                    .keyboardShortcut(.defaultAction)
            } message: {
                Text("browse.failed.command.message", bundle: .module)
            }
            .onChange(of: commands.announcement) { _, result in
                guard let result else { return }
                AccessibilityNotification.Announcement(BrowseCommandText.announcement(result, locale: locale)).post()
            }
    }
}
