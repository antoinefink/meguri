import AppModel
import CardRendering
import CoreBridge
import DesignSystem
import Editor
import SwiftUI
#if os(macOS)
import AppKit

/// The inspector's Note tab (`browser/workspace` V12, N21.4): the card
/// preview, which folds with its header; Deck and Type as round 5's form
/// rows; the note's filled fields in the round 5 editor's page, then one
/// "<e> Empty Fields" row; Tags as a token field; the formatting bar under
/// the fields. The page scrolls the fields itself (the editor owns its
/// scrolling and focus), so a long note scrolls inside the pane.
struct BrowseNoteTab: View {
    let model: BrowseModel
    let note: BrowseInspectorNote
    @Environment(\.locale) private var locale

    private var inspector: BrowseInspector { model.inspector }
    private var host: FieldEditorHost? { note.runtime as? FieldEditorHost }

    var body: some View {
        VStack(spacing: 0) {
            BrowsePreviewSection(inspector: inspector, note: note)
            Divider()
            if let error = note.error {
                ContentUnavailableView {
                    Label { Text("browse.inspector.editorUnavailable", bundle: .module) } icon: { Image(systemName: "exclamationmark.triangle") }
                } description: { Text(verbatim: error) }
            } else {
                selectionRows
                fields
                emptyFieldsRow
                tagsRow
                if let failure = note.saveFailure { saveFailure(failure) }
                Divider()
                BrowseFormattingBar(host: host, cloze: note.isCloze)
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("browse.inspector.note")
    }

    // MARK: Deck and Type (N17.1's rows)

    private var selectionRows: some View {
        Form {
            Picker(selection: Binding(get: { note.card?.deckId ?? 0 }, set: { inspector.chooseDeck($0) })) {
                ForEach(deckChoices, id: \.id) { deck in
                    Text(verbatim: Self.deckName(deck, home: homeDeck(for: deck))).tag(deck.id)
                }
            } label: { Text("browse.inspector.deck", bundle: .module) }
            .accessibilityIdentifier("browse.inspector.deck")
            LabeledContent {
                Menu {
                    // Choosing another type opens Change Note Type… on this note (U284).
                    ForEach(model.notetypes.filter { $0.id != note.notetype.id }, id: \.id) { type in
                        Button {} label: { Text(verbatim: type.name) }.disabled(true)
                    }
                    if model.notetypes.count > 1 { Divider() }
                    Button { model.commands.openNoteType(note.notetype.id) } label: {
                        Text(verbatim: BrowseCommandText.title(.editNoteType, .of(.editNoteType, facts: nil, available: true), locale: locale))
                    }
                    .accessibilityIdentifier("browse.inspector.type.editNoteType")
                } label: { Text(verbatim: note.notetype.name) }
                .fixedSize()
                .accessibilityIdentifier("browse.inspector.type")
            } label: { Text("browse.inspector.type", bundle: .module) }
        }
        .formStyle(.columns)
        .padding(.horizontal)
        .padding(.vertical, Spacing.small)
    }

    /// Normal decks by name; the card's own deck too when it is a filtered one.
    private var deckChoices: [DeckNameEntry] {
        var decks = inspector.decks.filter { !$0.filtered }.sorted { $0.fullName.localizedStandardCompare($1.fullName) == .orderedAscending }
        if let id = note.card?.deckId, !decks.contains(where: { $0.id == id }) {
            decks.insert(inspector.deckEntry(id) ?? DeckNameEntry(id: id, fullName: "", filtered: true), at: 0)
        }
        return decks
    }

    private func homeDeck(for deck: DeckNameEntry) -> DeckNameEntry? {
        guard deck.filtered, let card = note.card, card.deckId == deck.id, card.originalDeckId != 0 else { return nil }
        return inspector.deckEntry(card.originalDeckId)
    }

    /// V5.1's words: the path joined with " › "; a filtered deck with its
    /// home deck's last component in brackets.
    static func deckName(_ deck: DeckNameEntry, home: DeckNameEntry?) -> String {
        let path = deck.fullName.components(separatedBy: "::").joined(separator: " › ")
        guard let home, let last = home.fullName.components(separatedBy: "::").last else { return path }
        return "\(path) (\(last))"
    }

    // MARK: Fields

    @ViewBuilder private var fields: some View {
        if let host {
            FieldEditorView(host: host)
                // The page, not the note: the next note opens in the same web view.
                .id(ObjectIdentifier(host.webView))
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            Spacer()
        }
    }

    /// V12: "<e> Empty Fields", a disclosure; open, every field shows.
    @ViewBuilder private var emptyFieldsRow: some View {
        if !note.emptyFields.isEmpty {
            let open = inspector.emptyFieldsOpen(note.notetype.id)
            Button { inspector.setEmptyFieldsOpen(!open) } label: {
                Label {
                    Text(verbatim: BrowseInspectorText.counted("browse.inspector.emptyFields", UInt64(note.emptyFields.count), locale: locale))
                } icon: {
                    Image(systemName: "chevron.right").rotationEffect(.degrees(open ? 90 : 0)).imageScale(.small)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .foregroundStyle(.secondary)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal)
            .padding(.vertical, Spacing.xSmall)
            .accessibilityValue(Text(verbatim: open ? "expanded" : "collapsed"))
            .accessibilityIdentifier("browse.inspector.emptyFields")
        }
    }

    // MARK: Tags

    private var tagsRow: some View {
        Form {
            LabeledContent {
                BrowseTagField(tags: note.draft.tags, names: tagNames) { inspector.setTags($0) }
                    .accessibilityIdentifier("browse.inspector.tags")
            } label: { Text("browse.inspector.tags", bundle: .module) }
        }
        .formStyle(.columns)
        .padding(.horizontal)
        .padding(.vertical, Spacing.xSmall)
    }

    /// Every tag's full name, for the token field's completion.
    private var tagNames: [String] {
        var names: [String] = []
        func walk(_ nodes: [TagNode]) { for node in nodes { names.append(node.fullName); walk(node.children) } }
        walk(model.tags)
        return names
    }

    private func saveFailure(_ failure: EditNoteFlow.SaveFailure) -> some View {
        Label {
            switch failure {
            case .checkDatabase: Text("browse.inspector.saveFailed.checkDatabase", bundle: .module)
            case .notSaved: Text("browse.inspector.saveFailed.notSaved", bundle: .module)
            }
        } icon: { Image(systemName: "exclamationmark.triangle") }
        .foregroundStyle(.secondary)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal)
        .padding(.bottom, Spacing.xSmall)
        .accessibilityIdentifier("browse.inspector.saveFailed")
    }
}

/// V12's preview: the shown card as review renders it; its header folds it
/// away; a click flips question and answer; "Card <i> of <k> ›" steps
/// through the note's cards.
struct BrowsePreviewSection: View {
    let inspector: BrowseInspector
    let note: BrowseInspectorNote
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.locale) private var locale

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Button { inspector.previewFolded.toggle() } label: {
                    Label {
                        Text("browse.inspector.preview", bundle: .module)
                    } icon: {
                        Image(systemName: "chevron.right").rotationEffect(.degrees(inspector.previewFolded ? 0 : 90)).imageScale(.small)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .font(.headline)
                .accessibilityValue(Text(verbatim: inspector.previewFolded ? "collapsed" : "expanded"))
                .accessibilityIdentifier("browse.inspector.preview.header")
                Spacer()
                if note.cards.count > 1 {
                    Button { inspector.showCard((note.cardIndex + 1) % note.cards.count) } label: {
                        HStack(spacing: Spacing.xxSmall) {
                            Text(verbatim: BrowseInspectorText.card(note.cardIndex, of: note.cards.count, locale: locale))
                            Image(systemName: "chevron.right").imageScale(.small)
                        }
                    }
                    .buttonStyle(.borderless)
                    .accessibilityIdentifier("browse.inspector.preview.nextCard")
                }
            }
            .padding(.horizontal)
            .padding(.vertical, Spacing.xSmall)
            if !inspector.previewFolded {
                Group {
                    if let pair = inspector.preview.pair {
                        BrowsePreviewCard(pair: pair) { inspector.preview.flip() }
                    } else {
                        Color.clear
                    }
                }
                .frame(height: BrowseInspectorMetrics.previewHeight)
                .accessibilityElement(children: .contain)
                .accessibilityIdentifier("browse.inspector.preview")
            }
        }
        .onChange(of: colorScheme, initial: true) { _, scheme in
            inspector.preview.setAppearance(scheme == .dark ? .dark : .light)
        }
    }
}

enum BrowseInspectorMetrics {
    /// The preview's height: a card's question at its own size, with room around it.
    static let previewHeight: CGFloat = 200
}

/// The preview's page pair, with a click that flips the card. The click
/// recognizer sits on the pair's container and lets the page have the
/// click too, so links and play buttons inside the card still work.
struct BrowsePreviewCard: NSViewRepresentable {
    let pair: CardPagePair
    let flip: () -> Void

    func makeCoordinator() -> Coordinator { Coordinator(flip: flip) }

    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        attach(view, context: context)
        return view
    }

    func updateNSView(_ view: NSView, context: Context) {
        context.coordinator.flip = flip
        attach(view, context: context)
    }

    private func attach(_ view: NSView, context: Context) {
        guard pair.container.superview !== view else { return }
        pair.container.removeFromSuperview()
        pair.container.frame = view.bounds
        pair.container.autoresizingMask = [.width, .height]
        view.addSubview(pair.container)
        let click = NSClickGestureRecognizer(target: context.coordinator, action: #selector(Coordinator.clicked))
        click.delaysPrimaryMouseButtonEvents = false
        view.gestureRecognizers.forEach { view.removeGestureRecognizer($0) }
        view.addGestureRecognizer(click)
    }

    final class Coordinator: NSObject {
        var flip: () -> Void
        init(flip: @escaping () -> Void) { self.flip = flip }
        @objc func clicked() { flip() }
    }
}

/// V12's formatting bar under the fields: Bold, Italic, Underline, Cloze
/// (a cloze note type only), Attach, HTML. They act on the focused rich
/// field (`editor/formatting` F2). Attach waits for the editor's media
/// insertion (U194), so it is in its place, disabled.
struct BrowseFormattingBar: View {
    let host: FieldEditorHost?
    let cloze: Bool

    var body: some View {
        HStack(spacing: Spacing.xSmall) {
            button(.bold, "bold", "browse.inspector.format.bold")
            button(.italic, "italic", "browse.inspector.format.italic")
            button(.underline, "underline", "browse.inspector.format.underline")
            if cloze {
                Button { host?.cloze(sameNumber: false) } label: {
                    Label { Text("browse.inspector.format.cloze", bundle: .module) } icon: { Image(systemName: "curlybraces") }
                }
                .disabled(host?.canCloze != true)
                .help(Text("browse.inspector.format.cloze", bundle: .module))
                .accessibilityIdentifier("browse.inspector.format.cloze")
            }
            Button {} label: {
                Label { Text("browse.inspector.format.attach", bundle: .module) } icon: { Image(systemName: "paperclip") }
            }
            .disabled(true)
            .help(Text("browse.inspector.format.attach", bundle: .module))
            .accessibilityIdentifier("browse.inspector.format.attach")
            Spacer()
            if let host {
                Toggle(isOn: Binding(get: { host.focusedMode == .html }, set: { _ in host.toggleFocusedMode() })) {
                    Label { Text("browse.inspector.format.html", bundle: .module) } icon: {
                        Image(systemName: "chevron.left.forwardslash.chevron.right")
                    }
                }
                .toggleStyle(.button)
                .disabled(host.focusedMode == nil)
                .help(Text("browse.inspector.format.html", bundle: .module))
                .accessibilityIdentifier(host.modeControlIdentifier)
            }
        }
        .labelStyle(.iconOnly)
        .buttonStyle(.borderless)
        .padding(.horizontal)
        .padding(.vertical, Spacing.xSmall)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("browse.inspector.formatting")
    }

    private func button(_ format: FieldEditorHost.InlineFormat, _ symbol: String, _ key: LocalizedStringKey) -> some View {
        Button { host?.format(format) } label: {
            Label { Text(key, bundle: .module) } icon: { Image(systemName: symbol) }
        }
        .disabled(host?.canFormat != true)
        .help(Text(key, bundle: .module))
        .accessibilityIdentifier("browse.inspector.format.\(format.rawValue)")
    }
}
#endif
