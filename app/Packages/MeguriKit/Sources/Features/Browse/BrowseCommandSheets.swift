import AppModel
import CoreBridge
import DesignSystem
import SwiftUI
#if os(macOS)
import AppKit
#endif

/// The sheets' frame on every platform (N21.6), as round 6's New Deck form
/// has it: on the Mac the title leads in the headline style above the
/// content, and Cancel and the default button sit at the bottom trailing
/// edge (the system's places for a sheet's cancellation and confirmation
/// actions); on touch a navigation bar with an inline title. Return runs
/// the default button, which is never destructive; Esc cancels. Save
/// Search uses it too.
struct BrowseSheetFrame<Content: View>: View {
    let title: Text
    let confirm: Text
    let canConfirm: Bool
    let canCancel: Bool
    let identifier: String
    let onConfirm: () -> Void
    let onCancel: () -> Void
    @ViewBuilder let content: () -> Content

    var body: some View {
        #if os(macOS)
        VStack(alignment: .leading, spacing: Spacing.medium) {
            title
                .font(.headline)
                .accessibilityAddTraits(.isHeader)
                .accessibilityIdentifier("\(identifier).title")
            content()
        }
        .padding()
        .presentationSizing(.form.fitted(horizontal: false, vertical: true))
        .toolbar { buttons }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier(identifier)
        #else
        NavigationStack {
            content()
                .navigationTitle(title)
                .navigationBarTitleDisplayMode(.inline)
                .toolbar { buttons }
        }
        .presentationSizing(.form)
        .accessibilityIdentifier(identifier)
        #endif
    }

    @ToolbarContentBuilder private var buttons: some ToolbarContent {
        ToolbarItem(placement: .cancellationAction) {
            Button(role: .cancel) { onCancel() } label: { Text("browse.sheet.cancel", bundle: .module) }
                .keyboardShortcut(.cancelAction)
                .disabled(!canCancel)
                .accessibilityIdentifier("\(identifier).cancel")
        }
        ToolbarItem(placement: .confirmationAction) {
            Button { onConfirm() } label: { confirm }
                .keyboardShortcut(.defaultAction)
                .disabled(!canConfirm)
                .accessibilityIdentifier("\(identifier).confirm")
        }
    }
}

/// The open command sheet (U276), on the rows it opened on.
struct BrowseCommandSheetView: View {
    let model: BrowseModel
    @Bindable var sheet: BrowseSheet
    #if os(macOS)
    @State private var tagField = BrowseSheetTagCommit()
    #endif

    var body: some View {
        BrowseSheetFrame(title: title, confirm: confirmTitle, canConfirm: sheet.canConfirm, canCancel: sheet.canCancel,
                         identifier: "browse.sheet.\(identifier)", onConfirm: confirm, onCancel: { sheet.cancel() }) {
            content
            BrowseSheetStatus(sheet: sheet)
        }
        // A command under way ends only by its own end or Cancel.
        .interactiveDismissDisabled(sheet.isRunning)
    }

    @ViewBuilder private var content: some View {
        switch sheet.kind {
        case .changeDeck: BrowseChangeDeckContent(sheet: sheet)
        case .addTags, .removeTags: tagsContent
        case .setDueDate: BrowseDueDateContent(sheet: sheet)
        case .forget: BrowseForgetContent(sheet: sheet)
        }
    }

    /// Add Tags and Remove Tags: the system token field, completed from the
    /// collection's tags (Add) or the target notes' own (Remove, V8.1).
    @ViewBuilder private var tagsContent: some View {
        VStack(alignment: .leading, spacing: Spacing.xSmall) {
            #if os(macOS)
            BrowseSheetTagField(sheet: sheet, names: tagNames, commit: tagField)
                .accessibilityIdentifier("browse.sheet.tags.field")
            #else
            TextField(text: Binding(get: { sheet.tags.joined(separator: " ") },
                                    set: { sheet.tags = $0.split(whereSeparator: \.isWhitespace).map(String.init) })) {
                Text("browse.tags.prompt", bundle: .module)
            }
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .accessibilityIdentifier("browse.sheet.tags.field")
            #endif
            Text(sheet.kind == .addTags ? "browse.addTags.footer" : "browse.removeTags.footer", bundle: .module)
                .font(.footnote)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            if sheet.kind == .removeTags, sheet.selectionTags?.isEmpty == true {
                Text("browse.removeTags.none", bundle: .module)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("browse.sheet.tags.none")
            }
        }
    }

    /// The names the field completes from.
    private var tagNames: [String] {
        if sheet.kind == .removeTags { return sheet.selectionTags ?? [] }
        var names: [String] = []
        func walk(_ nodes: [TagNode]) { for node in nodes { names.append(node.fullName); walk(node.children) } }
        walk(model.tags)
        return names
    }

    private func confirm() {
        #if os(macOS)
        // The text still being typed becomes a token first (B9 splits the same way).
        if sheet.kind == .addTags || sheet.kind == .removeTags { tagField.commit() }
        #endif
        sheet.confirm()
    }

    private var title: Text {
        switch sheet.kind {
        case .changeDeck: Text("browse.changeDeck.title", bundle: .module)
        case .addTags: Text("browse.addTags.title", bundle: .module)
        case .removeTags: Text("browse.removeTags.title", bundle: .module)
        case .setDueDate: Text("browse.setDue.title", bundle: .module)
        case .forget: Text("browse.forget.title", bundle: .module)
        }
    }

    private var confirmTitle: Text {
        switch sheet.kind {
        case .changeDeck: Text("browse.changeDeck.move", bundle: .module)
        case .addTags: Text("browse.addTags.add", bundle: .module)
        case .removeTags: Text("browse.removeTags.remove", bundle: .module)
        case .setDueDate: Text("browse.setDue.set", bundle: .module)
        case .forget: Text("browse.forget.forget", bundle: .module)
        }
    }

    private var identifier: String {
        switch sheet.kind {
        case .changeDeck: "changeDeck"
        case .addTags: "addTags"
        case .removeTags: "removeTags"
        case .setDueDate: "setDueDate"
        case .forget: "forget"
        }
    }
}

/// Under the content: the command's progress with its Cancel in the frame,
/// or why it wrote nothing.
private struct BrowseSheetStatus: View {
    let sheet: BrowseSheet

    var body: some View {
        switch sheet.phase {
        case .running(let progress):
            if let progress, let total = progress.total, total > 0 {
                ProgressView(value: Double(min(progress.completed, total)), total: Double(total)) {
                    Text("browse.sheet.working", bundle: .module)
                }
                .accessibilityIdentifier("browse.sheet.progress")
            } else {
                ProgressView { Text("browse.sheet.working", bundle: .module) }
                    .progressViewStyle(.linear)
                    .accessibilityIdentifier("browse.sheet.progress")
            }
        case .cancelling:
            ProgressView { Text("browse.sheet.cancelling", bundle: .module) }
                .progressViewStyle(.linear)
                .accessibilityIdentifier("browse.sheet.progress")
        case .editing:
            if let failure = sheet.failure {
                Label { Text(verbatim: BrowseSheetText.failure(failure)) } icon: { Image(systemName: "exclamationmark.triangle") }
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("browse.sheet.failure")
            }
        }
    }
}

/// The sheets' texts that take values.
enum BrowseSheetText {
    static func failure(_ failure: BrowseSheetFailure, locale: Locale = .current) -> String {
        let text = { (key: String.LocalizationValue) in BrowseStrings.text(key, locale: locale) }
        switch failure {
        case .rowsChanged: return text("browse.sheet.failed.rowsChanged")
        case .deck(.filteredDeck): return text("browse.changeDeck.failed.filteredDeck")
        case .deck(.filteredParent): return text("browse.changeDeck.failed.filteredParent")
        case .deck(.tooDeep): return text("browse.changeDeck.failed.tooDeep")
        case .invalidDays(let kind): return dueError(kind, locale: locale)
        case .deck, .failed: return text("browse.sheet.failed.other")
        }
    }

    /// S2's refusals as the field explains them while it is edited (S9).
    static func dueError(_ kind: InvalidDaysKind, locale: Locale = .current) -> String {
        switch kind {
        case .empty: BrowseStrings.text("browse.setDue.error.empty", locale: locale)
        case .malformed: BrowseStrings.text("browse.setDue.error.malformed", locale: locale)
        case .tooLarge: BrowseStrings.text("browse.setDue.error.tooLarge", locale: locale)
        }
    }

    /// The day the text gives, as the preview reads it: "Due today", "Due
    /// tomorrow", "Due in 3 days", "Due in 3 to 7 days".
    static func duePreview(_ days: DueDays, locale: Locale = .current) -> String {
        let low = Int(days.minimum), high = Int(days.maximum)
        if low == high {
            switch low {
            case 0: return BrowseStrings.text("browse.setDue.preview.today", locale: locale)
            case 1: return BrowseStrings.text("browse.setDue.preview.tomorrow", locale: locale)
            default:
                return String(localized: "browse.setDue.preview.days", defaultValue: "\(low) \(low.formatted(.number.locale(locale)))",
                              bundle: .module, locale: locale)
            }
        }
        let from = low.formatted(.number.locale(locale)), to = high.formatted(.number.locale(locale))
        return String(localized: "browse.setDue.preview.range", defaultValue: "\(from) \(to)", bundle: .module, locale: locale)
    }

    /// "Create deck named “Verbs”" (N20.1's create row).
    static func create(_ name: String, locale: Locale = .current) -> String {
        String(localized: "browse.changeDeck.create", defaultValue: "\(name)", bundle: .module, locale: locale)
    }
}

// MARK: Change Deck

/// N20.1's deck search inside the sheet: the field, then the matching decks
/// as two-line rows (the decks the selection's cards are in marked), then
/// the create row while no deck has the typed name. ↑ and ↓ move the
/// highlight from the field, Return moves the cards to it.
private struct BrowseChangeDeckContent: View {
    @Bindable var sheet: BrowseSheet
    @Environment(\.locale) private var locale

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.small) {
            #if os(macOS)
            BrowseSheetSearchField(text: $sheet.deckQuery, prompt: BrowseStrings.text("browse.changeDeck.search", locale: locale),
                                   move: { sheet.moveHighlight(by: $0) })
                .accessibilityIdentifier("browse.sheet.changeDeck.search")
            #else
            TextField(text: $sheet.deckQuery) { Text("browse.changeDeck.search", bundle: .module) }
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .accessibilityIdentifier("browse.sheet.changeDeck.search")
            #endif
            list
        }
    }

    private var list: some View {
        ScrollViewReader { proxy in
            List(selection: Binding(get: { sheet.highlightedDeck }, set: { if let choice = $0 { sheet.chosenDeck = choice } })) {
                ForEach(sheet.deckRows) { row in
                    BrowseDeckRowView(row: row)
                        .tag(BrowseDeckChoice.deck(row.id))
                        .id(BrowseDeckChoice.deck(row.id))
                        .accessibilityIdentifier("browse.sheet.changeDeck.deck.\(row.id)")
                }
                if sheet.offersCreate {
                    Label { Text(verbatim: BrowseSheetText.create(sheet.deckQuery, locale: locale)) }
                        icon: { Image(systemName: "plus") }
                        .tag(BrowseDeckChoice.create)
                        .id(BrowseDeckChoice.create)
                        .accessibilityIdentifier("browse.sheet.changeDeck.create")
                }
            }
            .contextMenu(forSelectionType: BrowseDeckChoice.self, menu: { _ in }) { choices in
                // A double click on a row moves the cards there.
                if let choice = choices.first { sheet.choose(choice) }
            }
            .overlay {
                if sheet.decks != nil, sheet.deckChoices.isEmpty, !sheet.deckReadFailed {
                    Text("browse.changeDeck.noMatches", bundle: .module)
                        .foregroundStyle(.secondary)
                        .accessibilityIdentifier("browse.sheet.changeDeck.empty")
                } else if sheet.deckReadFailed {
                    Text("browse.changeDeck.readFailed", bundle: .module)
                        .foregroundStyle(.secondary)
                        .accessibilityIdentifier("browse.sheet.changeDeck.readFailed")
                }
            }
            .frame(minHeight: BrowseSheetMetrics.listHeight)
            .onChange(of: sheet.highlightedDeck) { _, choice in
                if let choice { proxy.scrollTo(choice) }
            }
            .accessibilityIdentifier("browse.sheet.changeDeck.list")
        }
    }
}

/// A deck: its last name component over its parents joined with " › ", and
/// a checkmark when a target card is in it now.
private struct BrowseDeckRowView: View {
    let row: BrowseDeckRow

    var body: some View {
        HStack {
            VStack(alignment: .leading) {
                Text(verbatim: row.name)
                if !row.parents.isEmpty {
                    Text(verbatim: row.parents.joined(separator: " › "))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            Spacer()
            if row.current {
                Image(systemName: "checkmark")
                    .foregroundStyle(.secondary)
                    .accessibilityHidden(true)
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text(verbatim: row.spokenPath))
        .accessibilityValue(row.current ? Text("browse.changeDeck.current", bundle: .module) : Text(verbatim: ""))
    }
}

enum BrowseSheetMetrics {
    /// The deck list's height in the Change Deck sheet: about eight rows.
    static let listHeight: CGFloat = 260
}

// MARK: Set Due Date

/// One text field (days, or a range "3-7"), the day it gives or why it is
/// refused under it, and a footer explaining the forms (N21.6, S2).
private struct BrowseDueDateContent: View {
    @Bindable var sheet: BrowseSheet
    @FocusState private var focused: Bool
    @State private var selection: TextSelection?
    @Environment(\.locale) private var locale

    var body: some View {
        Form {
            Section {
                TextField(text: $sheet.dueText, selection: $selection) { Text("browse.setDue.field", bundle: .module) }
                    .focused($focused)
                    .accessibilityIdentifier("browse.sheet.setDueDate.days")
            } footer: {
                VStack(alignment: .leading, spacing: Spacing.xSmall) {
                    check
                    Text("browse.setDue.footer", bundle: .module)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .font(.footnote)
            }
        }
        #if os(macOS)
        .formStyle(.columns)
        #endif
        .task {
            focused = true
            await Task.yield()
            // The remembered text, selected, so typing replaces it.
            selection = TextSelection(range: sheet.dueText.startIndex..<sheet.dueText.endIndex)
        }
    }

    @ViewBuilder private var check: some View {
        switch sheet.due {
        case .valid(let days):
            Text(verbatim: BrowseSheetText.duePreview(days, locale: locale))
                .accessibilityIdentifier("browse.sheet.setDueDate.preview")
        case .invalid(let kind):
            Text(verbatim: BrowseSheetText.dueError(kind, locale: locale))
                .foregroundStyle(.red)
                .accessibilityIdentifier("browse.sheet.setDueDate.error")
        case .checking:
            Text(verbatim: " ").accessibilityHidden(true)
        }
    }
}

// MARK: Forget

/// The two options and "Review history is kept." (V8.2, `browser/browse-strings`
/// W3); the button is Forget, the default and not destructive: Undo restores.
private struct BrowseForgetContent: View {
    @Bindable var sheet: BrowseSheet

    var body: some View {
        Form {
            Section {
                Toggle(isOn: $sheet.restorePosition) { Text("browse.reset.restorePosition", bundle: .module) }
                    .accessibilityIdentifier("browse.sheet.forget.restorePosition")
                Toggle(isOn: $sheet.resetCounts) { Text("browse.reset.resetCounts", bundle: .module) }
                    .accessibilityIdentifier("browse.sheet.forget.resetCounts")
            } footer: {
                Text("browse.forget.kept", bundle: .module)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("browse.sheet.forget.kept")
            }
        }
        #if os(macOS)
        .formStyle(.columns)
        #endif
    }
}

#if os(macOS)
// MARK: Mac fields

/// The system search field, whose ↑ and ↓ move the list's highlight while
/// the field keeps the focus (N20.1, H6.3). Return is the sheet's default
/// button; Esc its Cancel.
private struct BrowseSheetSearchField: NSViewRepresentable {
    @Binding var text: String
    let prompt: String
    let move: (Int) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeNSView(context: Context) -> NSSearchField {
        let field = NSSearchField()
        field.placeholderString = prompt
        field.delegate = context.coordinator
        field.sendsSearchStringImmediately = true
        field.stringValue = text
        DispatchQueue.main.async { field.window?.makeFirstResponder(field) }
        return field
    }

    func updateNSView(_ field: NSSearchField, context: Context) {
        context.coordinator.parent = self
        if field.stringValue != text { field.stringValue = text }
    }

    final class Coordinator: NSObject, NSSearchFieldDelegate {
        var parent: BrowseSheetSearchField
        init(_ parent: BrowseSheetSearchField) { self.parent = parent }

        func controlTextDidChange(_ notification: Notification) {
            guard let field = notification.object as? NSSearchField else { return }
            parent.text = field.stringValue
        }

        func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
            switch selector {
            case #selector(NSResponder.moveDown(_:)): parent.move(1); return true
            case #selector(NSResponder.moveUp(_:)): parent.move(-1); return true
            default: return false
            }
        }
    }
}

/// What the sheet's default button asks of its token field: the text still
/// being typed becomes a token, and the tags reach the sheet.
@MainActor final class BrowseSheetTagCommit {
    fileprivate weak var field: NSTokenField?
    fileprivate var sheet: BrowseSheet?

    func commit() {
        guard let field, let sheet else { return }
        field.window?.makeFirstResponder(nil)
        sheet.tags = BrowseSheetTagField.tokens(field)
        sheet.typingTag = false
    }
}

/// The system token field (N21.6): a token per tag, completed from `names`
/// (`tags/complete`: a prefix of the full name or of a component, ignoring
/// case). A space or Return ends a token.
private struct BrowseSheetTagField: NSViewRepresentable {
    let sheet: BrowseSheet
    let names: [String]
    let commit: BrowseSheetTagCommit

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeNSView(context: Context) -> NSTokenField {
        let field = NSTokenField()
        field.delegate = context.coordinator
        field.tokenStyle = .rounded
        field.tokenizingCharacterSet = CharacterSet.whitespacesAndNewlines
        field.completionDelay = 0
        field.lineBreakMode = .byWordWrapping
        field.cell?.wraps = true
        field.cell?.isScrollable = false
        field.objectValue = sheet.tags
        field.placeholderString = BrowseStrings.text("browse.tags.prompt")
        field.setContentHuggingPriority(.defaultLow, for: .horizontal)
        field.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        commit.field = field
        commit.sheet = sheet
        DispatchQueue.main.async { field.window?.makeFirstResponder(field) }
        return field
    }

    func updateNSView(_ field: NSTokenField, context: Context) {
        context.coordinator.parent = self
        commit.field = field
        commit.sheet = sheet
    }

    func sizeThatFits(_ proposal: ProposedViewSize, nsView field: NSTokenField, context: Context) -> CGSize? {
        guard let width = proposal.width, width.isFinite else { return nil }
        let height = field.cell?.cellSize(forBounds: NSRect(x: 0, y: 0, width: width, height: .greatestFiniteMagnitude)).height
        return CGSize(width: width, height: max(height ?? 0, field.intrinsicContentSize.height))
    }

    /// The field's tokens, each split at white space.
    static func tokens(_ field: NSTokenField) -> [String] {
        (field.objectValue as? [Any] ?? []).compactMap { $0 as? String }
            .flatMap { $0.split(whereSeparator: \.isWhitespace).map(String.init) }
    }

    final class Coordinator: NSObject, NSTokenFieldDelegate {
        var parent: BrowseSheetTagField
        init(_ parent: BrowseSheetTagField) { self.parent = parent }

        func tokenField(_ tokenField: NSTokenField, completionsForSubstring substring: String, indexOfToken tokenIndex: Int,
                        indexOfSelectedItem selectedIndex: UnsafeMutablePointer<Int>?) -> [Any]? {
            let typed = substring.lowercased()
            guard !typed.isEmpty else { return [] }
            let held = Set(BrowseSheetTagField.tokens(tokenField).map { $0.lowercased() })
            let matches = parent.names.filter { name in
                let lower = name.lowercased()
                return !held.contains(lower)
                    && (lower.hasPrefix(typed) || lower.components(separatedBy: "::").contains { $0.hasPrefix(typed) })
            }
            selectedIndex?.pointee = -1
            return Array(matches.prefix(50))
        }

        func controlTextDidChange(_ notification: Notification) {
            guard let field = notification.object as? NSTokenField,
                  let editor = field.currentEditor() as? NSTextView else { return }
            // Tokens show as attachment characters, typed text as itself: the
            // field holds a tag when either is there. The tokens are read when
            // editing ends or the default button commits them.
            parent.sheet.typingTag = editor.string.contains { !$0.isWhitespace }
        }

        func controlTextDidEndEditing(_ notification: Notification) {
            guard let field = notification.object as? NSTokenField else { return }
            parent.sheet.tags = BrowseSheetTagField.tokens(field)
            parent.sheet.typingTag = false
        }
    }
}
#endif
