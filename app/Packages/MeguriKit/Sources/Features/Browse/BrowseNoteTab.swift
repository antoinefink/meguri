import AppModel
import CardRendering
import CoreBridge
import DesignSystem
import Editor
import SwiftUI
#if os(macOS)
import AppKit

/// The inspector's Note tab (`browser/workspace` V12, N21.4; Option A, the
/// owner's choice of 2026-11-23): one scroll view holds the card preview
/// (folding with its header), Deck and Type as label and pop-up rows, the
/// fields as a plain list (name small and secondary, text at the body size,
/// no box until edited), the "<e> Empty Fields" row and a Tags section; the
/// formatting bar stays at the pane's bottom edge. The fields' page grows
/// with its content and never scrolls itself; the scroll view follows the
/// caret.
struct BrowseNoteTab: View {
    let model: BrowseModel
    let note: BrowseInspectorNote
    @Environment(\.locale) private var locale
    @State private var position = ScrollPosition(edge: .top)
    @State private var visible: CGRect = .zero
    @State private var fieldsTop: CGFloat = 0
    /// Deck and Type: the rows' width and the longer label's, so the two pop-ups share one width.
    @State private var rowsWidth: CGFloat = 0
    @State private var labelWidth: CGFloat = 0

    private var inspector: BrowseInspector { model.inspector }
    private var host: FieldEditorHost? { note.runtime as? FieldEditorHost }

    var body: some View {
        VStack(spacing: 0) {
            // The preview's header stays; the card under it scrolls with the rest.
            BrowsePreviewSection(inspector: inspector, note: note, part: .header)
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    BrowsePreviewSection(inspector: inspector, note: note, part: .card)
                    Divider()
                    if let error = note.error {
                        ContentUnavailableView {
                            Label { Text("browse.inspector.editorUnavailable", bundle: .module) } icon: { Image(systemName: "exclamationmark.triangle") }
                        } description: { Text(verbatim: error) }
                    } else {
                        selectionRows
                        Divider().padding(.horizontal)
                        fields
                        emptyFieldsRow
                        Divider().padding(.horizontal)
                        tagsSection
                        if let failure = note.saveFailure { saveFailure(failure) }
                    }
                }
                .coordinateSpace(.named(Self.content))
            }
            .scrollPosition($position)
            .onScrollGeometryChange(for: CGRect.self) { $0.visibleRect } action: { _, rect in visible = rect }
            .onChange(of: host?.caret) { _, caret in follow(caret) }
            // A line typed at the end: the page grows after the caret moves there.
            .onChange(of: host?.contentHeight) { follow(host?.caret) }
            if note.error == nil {
                Divider()
                BrowseFormattingBar(host: host, cloze: note.isCloze)
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("browse.inspector.note")
    }

    nonisolated private static let content = "browse.inspector.content"

    /// The caret's line stays in sight while typing in a long field.
    private func follow(_ caret: ClosedRange<Double>?) {
        guard let caret, host?.fieldFocused == true else { return }
        // After this layout pass, so the scroll view already has the page's new height.
        Task { @MainActor in
            guard visible.height > 0 else { return }
            let top = fieldsTop + caret.lowerBound, bottom = fieldsTop + caret.upperBound, margin = Spacing.small
            if bottom > visible.maxY - margin { position.scrollTo(y: bottom - visible.height + margin) }
            else if top < visible.minY + margin { position.scrollTo(y: max(0, top - margin)) }
        }
    }

    // MARK: Deck and Type (N17.1's rows)

    /// Option A: a short trailing-aligned label column, the two pop-ups the
    /// same width, filling the rest of the row.
    private var popupWidth: CGFloat? { rowsWidth > 0 ? max(0, rowsWidth - labelWidth - Spacing.small) : nil }

    private func label(_ key: LocalizedStringKey) -> some View {
        Text(key, bundle: .module)
            .fixedSize()
            .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { labelWidth = max(labelWidth, $0) }
    }

    private var selectionRows: some View {
        Grid(alignment: .leading, horizontalSpacing: Spacing.small, verticalSpacing: Spacing.small) {
            GridRow {
                label("browse.inspector.deck").gridColumnAlignment(.trailing)
                Picker(selection: Binding(get: { note.card?.deckId ?? 0 }, set: { inspector.chooseDeck($0) })) {
                    ForEach(deckChoices, id: \.id) { deck in
                        Text(verbatim: Self.deckName(deck, home: homeDeck(for: deck))).tag(deck.id)
                    }
                } label: { Text("browse.inspector.deck", bundle: .module) }
                .labelsHidden()
                .frame(width: popupWidth, alignment: .leading)
                .accessibilityIdentifier("browse.inspector.deck")
            }
            GridRow {
                label("browse.inspector.type")
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
                .frame(width: popupWidth, alignment: .leading)
                .accessibilityLabel(Text("browse.inspector.type", bundle: .module))
                .accessibilityIdentifier("browse.inspector.type")
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { rowsWidth = $0 }
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
                // As tall as its fields: the page never scrolls, the pane does.
                .frame(maxWidth: .infinity)
                .frame(height: max(BrowseInspectorMetrics.fieldsMinimum, host.contentHeight))
                .onGeometryChange(for: CGFloat.self) { $0.frame(in: .named(Self.content)).minY } action: { fieldsTop = $0 }
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

    /// Option A: Tags as a section of its own, its small header over a
    /// token field that wraps onto as many lines as it needs.
    private var tagsSection: some View {
        VStack(alignment: .leading, spacing: Spacing.xxSmall) {
            Text("browse.inspector.tags", bundle: .module)
                .font(.subheadline)
                .foregroundStyle(.secondary)
            BrowseTagField(tags: note.draft.tags, names: tagNames) { [note] in inspector.setTags($0, for: note) }
                // A field per note: what was typed for one note never reaches the next.
                .id(note.noteID)
                .accessibilityIdentifier("browse.inspector.tags")
        }
        .padding(.horizontal)
        .padding(.vertical, Spacing.small)
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
    enum Part { case header, card }
    let inspector: BrowseInspector
    let note: BrowseInspectorNote
    let part: Part
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.locale) private var locale

    var body: some View {
        switch part {
        case .header:
            // The header is always there, folded or not: it keeps the card's appearance.
            header.onChange(of: colorScheme, initial: true) { _, scheme in
                inspector.preview.setAppearance(scheme == .dark ? .dark : .light)
            }
        case .card: card
        }
    }

    private var header: some View {
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
            if inspector.preview.hasAudio {
                Button { inspector.preview.replay() } label: {
                    Label { Text("browse.inspector.preview.play", bundle: .module) } icon: { Image(systemName: "speaker.wave.2") }
                }
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .help(Text("browse.inspector.preview.play", bundle: .module))
                .accessibilityIdentifier("browse.inspector.preview.play")
            }
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
    }

    @ViewBuilder private var card: some View {
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
}

enum BrowseInspectorMetrics {
    /// Option A: the preview's height, small enough that the fields below it stay in reach.
    static let previewHeight: CGFloat = 160
    /// The fields' page before it reports its height.
    static let fieldsMinimum: CGFloat = 44
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
                .disabled(!host.canToggleSource)
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
