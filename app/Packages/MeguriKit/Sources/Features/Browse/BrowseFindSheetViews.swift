import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

// U277's two sheets (N21.6; `browser/workspace` V9, V10) in the frame every
// Browse sheet shares: the title over a system Form, Cancel and the default
// button in the system's places, Return runs the default (never destructive),
// Esc cancels.

/// The sheets' texts that take values (V16, `browser/browse-strings` W2.1).
enum BrowseFindText {
    private static func number(_ value: UInt64, _ locale: Locale) -> (Int, String) {
        let n = Int(clamping: value)
        return (n, n.formatted(.number.locale(locale)))
    }

    /// "3 Selected", or "Selected" before the notes are counted.
    static func selected(_ notes: UInt64?, locale: Locale = .current) -> String {
        guard let notes else { return BrowseStrings.text("browse.findReplace.notes.selectedUnknown", locale: locale) }
        let (_, shown) = number(notes, locale)
        return String(localized: "browse.findReplace.notes.selected", defaultValue: "\(shown)", bundle: .module, locale: locale)
    }

    /// "All 120 Found", or "All Found" before they are counted.
    static func allFound(_ notes: UInt64?, locale: Locale = .current) -> String {
        guard let notes else { return BrowseStrings.text("browse.findReplace.notes.allFoundUnknown", locale: locale) }
        let (_, shown) = number(notes, locale)
        return String(localized: "browse.findReplace.notes.allFound", defaultValue: "\(shown)", bundle: .module, locale: locale)
    }

    /// "3 of 12 notes will change".
    static func willChange(_ changed: UInt64, of considered: UInt64, locale: Locale = .current) -> String {
        let (c, cShown) = number(changed, locale), (k, kShown) = number(considered, locale)
        return String(localized: "browse.findReplace.willChange", defaultValue: "\(c) \(k) \(cShown) \(kShown)", bundle: .module,
                      locale: locale)
    }

    /// "Replace in 3 Notes" ("Replace in 1 Note"); "Replace" before a count.
    static func replace(_ count: UInt64?, locale: Locale = .current) -> String {
        guard let count, count > 0 else { return BrowseStrings.text("browse.findReplace.replaceUnknown", locale: locale) }
        let (n, shown) = number(count, locale)
        return String(localized: "browse.findReplace.replace", defaultValue: "\(n) \(shown)", bundle: .module, locale: locale)
    }

    /// W2.1's inline errors.
    static func invalid(_ invalid: BrowseFindReplaceInvalid, locale: Locale = .current) -> String {
        switch invalid {
        case .pattern(let reason):
            String(localized: "browse.findReplace.error.invalidPattern", defaultValue: "\(reason)", bundle: .module, locale: locale)
        case .tooLarge: BrowseStrings.text("browse.findReplace.error.patternTooLarge", locale: locale)
        case .tagSeparator: BrowseStrings.text("browse.findReplace.error.tagSeparator", locale: locale)
        }
    }

    /// "Duplicates in Front: 3 groups" (V10's token, its help and VoiceOver).
    static func duplicates(field: String, groups: Int, locale: Locale = .current) -> String {
        let shown = groups.formatted(.number.locale(locale))
        return String(localized: "browse.token.duplicates", defaultValue: "\(field) \(groups) \(shown)", bundle: .module,
                      locale: locale)
    }

    /// The token's value part after its kind ("in Front: 3 groups").
    static func duplicatesValue(field: String, groups: Int, locale: Locale = .current) -> String {
        let shown = groups.formatted(.number.locale(locale))
        return String(localized: "browse.token.duplicatesValue", defaultValue: "\(field) \(groups) \(shown)", bundle: .module,
                      locale: locale)
    }

    /// The help of a disabled Find and Replace… or Find Duplicates….
    static func unavailable(_ reason: BrowseUnavailable, locale: Locale = .current) -> String {
        switch reason {
        case .searching: BrowseStrings.text("browse.command.unavailable.searching", locale: locale)
        case .busy: BrowseStrings.text("browse.command.unavailable.busy", locale: locale)
        case .notReady: BrowseStrings.text("browse.command.unavailable.notReady", locale: locale)
        }
    }

    /// An entry's text with its changes marked (V9): removed text crossed
    /// out in red, inserted text in the accent colour.
    static func marked(_ spans: [FindReplaceSpan]) -> AttributedString {
        var text = AttributedString()
        for span in spans {
            var part = AttributedString(span.text)
            switch span.edit {
            case .kept: break
            case .removed:
                part.strikethroughStyle = .single
                part.foregroundColor = .red
            case .inserted:
                part.foregroundColor = .accentColor
            }
            text += part
        }
        return text
    }

    /// The old and new texts of an entry, for VoiceOver.
    static func spoken(_ spans: [FindReplaceSpan]) -> (old: String, new: String) {
        var old = "", new = ""
        for span in spans {
            switch span.edit {
            case .kept: old += span.text; new += span.text
            case .removed: old += span.text
            case .inserted: new += span.text
            }
        }
        return (old, new)
    }
}

// MARK: Find and Replace

/// V9's sheet: Find, Replace With, In, Notes, the three options; the
/// preview's count and list; "Undo restores every note."; Cancel and
/// "Replace in <c> Notes".
struct BrowseFindReplaceSheetView: View {
    @Bindable var sheet: BrowseFindReplaceSheet
    @FocusState private var findFocused: Bool
    @Environment(\.locale) private var locale

    var body: some View {
        BrowseSheetFrame(title: Text("browse.findReplace.title", bundle: .module),
                         confirm: Text(verbatim: BrowseFindText.replace(sheet.confirmCount, locale: locale)),
                         canConfirm: sheet.canConfirm, canCancel: sheet.canCancel, identifier: "browse.sheet.findReplace",
                         onConfirm: { sheet.confirm() }, onCancel: { sheet.cancel() }) {
            form
            BrowseFindReplacePreviewView(sheet: sheet)
            BrowseSheetStatus(phase: sheet.phase, failure: sheet.failure)
            Text("browse.findReplace.undoNote", bundle: .module)
                .font(.footnote)
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("browse.sheet.findReplace.undoNote")
        }
        .interactiveDismissDisabled(sheet.isRunning)
        .task {
            findFocused = true
        }
    }

    private var form: some View {
        Form {
            TextField(text: $sheet.find) { Text("browse.findReplace.find", bundle: .module) }
                .focused($findFocused)
                .autocorrectionDisabled()
                #if os(iOS)
                .textInputAutocapitalization(.never)
                #endif
                .textInputSuggestions { suggestions(sheet.findHistory, typed: sheet.find) }
                .accessibilityIdentifier("browse.sheet.findReplace.find")
            if case .invalid(let invalid) = sheet.preview {
                Text(verbatim: BrowseFindText.invalid(invalid, locale: locale))
                    .font(.footnote)
                    .foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("browse.sheet.findReplace.error")
            }
            TextField(text: $sheet.replace) { Text("browse.findReplace.replaceWith", bundle: .module) }
                .autocorrectionDisabled()
                #if os(iOS)
                .textInputAutocapitalization(.never)
                #endif
                .textInputSuggestions { suggestions(sheet.replaceHistory, typed: sheet.replace) }
                .accessibilityIdentifier("browse.sheet.findReplace.replace")
            Picker(selection: $sheet.inEntry) {
                ForEach(BrowseFieldMenu.sections(sheet.entries), id: \.self) { section in
                    if let title = section.title {
                        Section(title) { ForEach(section.entries) { entry in entryLabel(entry).tag(entry.id) } }
                    } else {
                        ForEach(section.entries) { entry in entryLabel(entry).tag(entry.id) }
                    }
                }
            } label: {
                Text("browse.findReplace.in", bundle: .module)
            }
            .accessibilityIdentifier("browse.sheet.findReplace.in")
            Picker(selection: $sheet.notes) {
                Text(verbatim: BrowseFindText.selected(sheet.selectedNotes, locale: locale))
                    .tag(BrowseFindReplaceSheet.Notes.selected)
                    .disabled(!sheet.canChooseSelected)
                Text(verbatim: BrowseFindText.allFound(sheet.foundNotes, locale: locale))
                    .tag(BrowseFindReplaceSheet.Notes.allFound)
                    .disabled(sheet.allFound == nil)
            } label: {
                Text("browse.findReplace.notes", bundle: .module)
            }
            #if os(macOS)
            .pickerStyle(.radioGroup)
            .horizontalRadioGroupLayout()
            #endif
            .accessibilityIdentifier("browse.sheet.findReplace.notes")
            Toggle(isOn: $sheet.matchCase) { Text("browse.findReplace.matchCase", bundle: .module) }
                .accessibilityIdentifier("browse.sheet.findReplace.matchCase")
            Toggle(isOn: $sheet.wholeWords) { Text("browse.findReplace.wholeWords", bundle: .module) }
                .accessibilityIdentifier("browse.sheet.findReplace.wholeWords")
            Toggle(isOn: $sheet.regex) { Text("browse.findReplace.regex", bundle: .module) }
                .accessibilityIdentifier("browse.sheet.findReplace.regex")
        }
        #if os(macOS)
        .formStyle(.columns)
        #endif
        .disabled(sheet.isRunning)
    }

    /// F2's history entries that continue the typed text (all of them while
    /// it is empty), the typed text itself left out.
    @ViewBuilder private func suggestions(_ history: [String], typed: String) -> some View {
        ForEach(history.filter { $0 != typed && (typed.isEmpty || $0.hasPrefix(typed)) }.prefix(10), id: \.self) { entry in
            Text(verbatim: entry).textInputCompletion(entry)
        }
    }

    private func entryLabel(_ entry: BrowseFieldEntry) -> Text {
        switch entry.kind {
        case .allFields: Text("browse.findReplace.in.allFields", bundle: .module)
        case .tags: Text("browse.findReplace.in.tags", bundle: .module)
        case .field(let name): Text(verbatim: name)
        }
    }
}

/// The field menus' sections: the entries before, under and after each
/// note type's name (V9, V10).
enum BrowseFieldMenu {
    struct Section: Hashable {
        let title: String?
        let entries: [BrowseFieldEntry]
    }

    static func sections(_ entries: [BrowseFieldEntry]) -> [Section] {
        var sections: [Section] = []
        for entry in entries {
            if let last = sections.last, last.title == entry.group {
                sections[sections.count - 1] = Section(title: last.title, entries: last.entries + [entry])
            } else {
                sections.append(Section(title: entry.group, entries: [entry]))
            }
        }
        return sections
    }
}

/// V9's preview: "<c> of <k> notes will change", "Counting…" or "No
/// Matches" over the list of the first 100 notes that change, each with its
/// Sort Field and its changed fields' texts, the changes marked.
private struct BrowseFindReplacePreviewView: View {
    let sheet: BrowseFindReplaceSheet
    @Environment(\.locale) private var locale

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.xSmall) {
            header
                .font(.callout)
                .accessibilityIdentifier("browse.sheet.findReplace.count")
            List {
                ForEach(notes, id: \.id) { note in
                    BrowseFindReplaceNoteRow(note: note, showsField: showsField)
                        .accessibilityIdentifier("browse.sheet.findReplace.note.\(note.id)")
                }
            }
            .frame(minHeight: BrowseSheetMetrics.previewHeight)
            .overlay {
                if case .ready(let preview) = sheet.preview, preview.changed == 0 {
                    Text("browse.findReplace.noMatches", bundle: .module)
                        .foregroundStyle(.secondary)
                        .accessibilityIdentifier("browse.sheet.findReplace.noMatches")
                }
            }
            .accessibilityIdentifier("browse.sheet.findReplace.list")
            if let shown = sheet.shownPreview, shown.changed > UInt64(notes.count), !notes.isEmpty {
                Text("browse.findReplace.firstNotes", bundle: .module)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        }
    }

    @ViewBuilder private var header: some View {
        switch sheet.preview {
        case .counting:
            Text("browse.findReplace.counting", bundle: .module).foregroundStyle(.secondary)
        case .ready(let preview):
            if preview.changed == 0 {
                Text("browse.findReplace.noMatches", bundle: .module).foregroundStyle(.secondary)
            } else {
                Text(verbatim: BrowseFindText.willChange(preview.changed, of: preview.considered, locale: locale))
            }
        case .invalid:
            // The error shows under Find; the count line stays empty.
            Text(verbatim: " ").accessibilityHidden(true)
        case .damaged:
            Text("browse.findReplace.damaged", bundle: .module).foregroundStyle(.secondary)
        case .rowsChanged:
            Text("browse.sheet.failed.rowsChanged", bundle: .module).foregroundStyle(.secondary)
        case .failed:
            Text("browse.sheet.failed.other", bundle: .module).foregroundStyle(.secondary)
        }
    }

    /// The field name shows only with All Fields (V9).
    private var showsField: Bool { sheet.chosenEntry?.kind == .allFields }

    /// The entries by note, in the preview's order; the last preview's
    /// while a newer one counts, none while the inputs cannot run.
    private var notes: [BrowseFindReplaceNote] {
        switch sheet.preview {
        case .invalid, .damaged, .rowsChanged, .failed: return []
        case .counting, .ready: break
        }
        var notes: [BrowseFindReplaceNote] = []
        for entry in sheet.shownPreview?.entries ?? [] {
            if let last = notes.last, last.id == entry.noteID {
                notes[notes.count - 1].entries.append(entry)
            } else {
                notes.append(BrowseFindReplaceNote(id: entry.noteID, sortField: entry.sortField, entries: [entry]))
            }
        }
        return notes
    }
}

private struct BrowseFindReplaceNote {
    let id: Int64
    let sortField: String
    var entries: [FindReplacePreviewEntry]
}

/// One note of the preview: its Sort Field, then each changed field's new
/// text with the changes marked ("formatting only" when only markup
/// changes, U265's design note).
private struct BrowseFindReplaceNoteRow: View {
    let note: BrowseFindReplaceNote
    let showsField: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.xxSmall) {
            Text(verbatim: note.sortField)
                .fontWeight(.semibold)
                .lineLimit(1)
            ForEach(Array(note.entries.enumerated()), id: \.offset) { _, entry in
                VStack(alignment: .leading, spacing: 0) {
                    if showsField, let field = entry.field {
                        Text(verbatim: field)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    if entry.spans.contains(where: { $0.edit != .kept }) {
                        Text(BrowseFindText.marked(entry.spans))
                            .lineLimit(3)
                    } else {
                        Text("browse.findReplace.formattingOnly", bundle: .module)
                            .italic()
                            .foregroundStyle(.secondary)
                    }
                }
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(Text(verbatim: spoken(entry)))
            }
        }
        .padding(.vertical, Spacing.xxSmall)
        .accessibilityElement(children: .contain)
    }

    private func spoken(_ entry: FindReplacePreviewEntry) -> String {
        let texts = BrowseFindText.spoken(entry.spans)
        let field = entry.field.map { $0 + ": " } ?? ""
        return note.sortField + ". " + field + texts.new + " (" + texts.old + ")"
    }
}

// MARK: Find Duplicates

/// V10's small sheet: Field, Notes, Cancel and Find; "No duplicates found."
/// when Find finds no group.
struct BrowseFindDuplicatesSheetView: View {
    @Bindable var sheet: BrowseFindDuplicatesSheet
    @Environment(\.locale) private var locale

    var body: some View {
        BrowseSheetFrame(title: Text("browse.findDuplicates.title", bundle: .module),
                         confirm: Text("browse.findDuplicates.find", bundle: .module), canConfirm: sheet.canFind,
                         canCancel: sheet.canCancel, identifier: "browse.sheet.findDuplicates",
                         onConfirm: { sheet.confirm() }, onCancel: { sheet.cancel() }) {
            Form {
                Picker(selection: $sheet.fieldEntry) {
                    if sheet.fieldEntry == nil {
                        Text("browse.findDuplicates.chooseField", bundle: .module).tag(Int?.none)
                    }
                    ForEach(BrowseFieldMenu.sections(sheet.entries), id: \.self) { section in
                        if let title = section.title {
                            Section(title) { ForEach(section.entries) { entry in label(entry).tag(Int?.some(entry.id)) } }
                        } else {
                            ForEach(section.entries) { entry in label(entry).tag(Int?.some(entry.id)) }
                        }
                    }
                } label: {
                    Text("browse.findDuplicates.field", bundle: .module)
                }
                .accessibilityIdentifier("browse.sheet.findDuplicates.field")
                Picker(selection: $sheet.notes) {
                    Text("browse.findDuplicates.allNotes", bundle: .module).tag(BrowseFindDuplicatesSheet.Notes.allNotes)
                    Text(verbatim: BrowseFindText.allFound(sheet.foundNotes, locale: locale))
                        .tag(BrowseFindDuplicatesSheet.Notes.allFound)
                        .disabled(sheet.allFound == nil)
                } label: {
                    Text("browse.findReplace.notes", bundle: .module)
                }
                #if os(macOS)
                .pickerStyle(.radioGroup)
                .horizontalRadioGroupLayout()
                #endif
                .accessibilityIdentifier("browse.sheet.findDuplicates.notes")
            }
            #if os(macOS)
            .formStyle(.columns)
            #endif
            .disabled(sheet.isRunning)
            if sheet.noneFound {
                Text("browse.findDuplicates.none", bundle: .module)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("browse.sheet.findDuplicates.none")
            }
            BrowseSheetStatus(phase: sheet.phase, failure: sheet.failure)
        }
        .interactiveDismissDisabled(sheet.isRunning)
        .onChange(of: sheet.noneFound) { _, none in
            if none { AccessibilityNotification.Announcement(BrowseStrings.text("browse.findDuplicates.none", locale: locale)).post() }
        }
    }

    private func label(_ entry: BrowseFieldEntry) -> Text {
        if case .field(let name) = entry.kind { return Text(verbatim: name) }
        return Text(verbatim: "")
    }
}
