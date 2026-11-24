import AppModel
import CardRendering
import CoreBridge
import DesignSystem
import SwiftUI

/// The Cards tab's live preview (`notetypes/workspace` W9; N22.3): a bar of
/// the column bars' height with the note menu (and the Cloze Number menu
/// for a cloze note type), Question | Answer and Light | Dark, then the card
/// as review draws it, the note type's own styling included. The card on
/// the page stays until the next render is ready; a template error shows
/// over it in the section's words, as the issues list says it.
struct NoteTypePreviewPane: View {
    @Bindable var model: NoteTypesModel
    let id: Int64
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.locale) private var locale

    private var preview: NoteTypePreview { model.preview }

    var body: some View {
        VStack(spacing: .zero) {
            bar
                .padding(.vertical, Spacing.xxSmall)
                .modifier(CardsColumnBar())
            Divider()
            page
        }
        .onAppear { preview.visible = true }
        .onDisappear { preview.visible = false }
        .onChange(of: colorScheme, initial: true) { _, scheme in
            preview.systemAppearance = scheme == .dark ? .dark : .light
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("noteTypes.cards.preview")
    }

    // MARK: The bar

    /// The bar's controls, each at its own size. Where the column is too
    /// narrow for all of them, the Cloze Number moves into the note menu,
    /// then the note menu shows only its symbol, then Light | Dark moves
    /// into it: the bar never touches the column's edges or clips.
    private var bar: some View {
        ViewThatFits(in: .horizontal) {
            barLine(noteLabel: true, appearanceInMenu: false, clozeInMenu: false)
            barLine(noteLabel: true, appearanceInMenu: false, clozeInMenu: true)
            barLine(noteLabel: false, appearanceInMenu: false, clozeInMenu: true)
            barLine(noteLabel: false, appearanceInMenu: true, clozeInMenu: true)
        }
    }

    private var cloze: Bool { model.draft(id)?.isCloze == true }

    private func barLine(noteLabel: Bool, appearanceInMenu: Bool, clozeInMenu: Bool) -> some View {
        HStack(spacing: Spacing.small) {
            noteMenu(label: noteLabel, appearanceInMenu: appearanceInMenu, clozeInMenu: clozeInMenu && cloze)
            if cloze && !clozeInMenu { clozeMenu }
            Spacer(minLength: Spacing.xSmall)
            sidePicker
            if !appearanceInMenu { appearancePicker }
        }
    }

    private var sidePicker: some View {
        Picker(selection: Binding(get: { preview.side }, set: { preview.side = $0 })) {
            Text("noteTypes.preview.question", bundle: .module).tag(CardRenderSide.question)
            Text("noteTypes.preview.answer", bundle: .module).tag(CardRenderSide.answer)
        } label: {
            Text("noteTypes.preview.side", bundle: .module)
        }
        .pickerStyle(.segmented)
        .labelsHidden()
        .fixedSize()
        .accessibilityIdentifier("noteTypes.cards.preview.side")
    }

    /// Light | Dark, a sun and a moon in the bar (or named, in the note menu).
    private func appearanceChoices(iconsOnly: Bool) -> some View {
        Picker(selection: Binding(get: { preview.appearance }, set: { preview.appearanceChoice = $0 })) {
            Label { Text("noteTypes.preview.light", bundle: .module) } icon: { Image(systemName: "sun.max") }
                .labelStyle(iconsOnly ? AnyLabelStyle(.iconOnly) : AnyLabelStyle(.titleAndIcon))
                .tag(CardRuntime.Appearance.light)
                .accessibilityIdentifier("noteTypes.cards.preview.light")
            Label { Text("noteTypes.preview.dark", bundle: .module) } icon: { Image(systemName: "moon") }
                .labelStyle(iconsOnly ? AnyLabelStyle(.iconOnly) : AnyLabelStyle(.titleAndIcon))
                .tag(CardRuntime.Appearance.dark)
                .accessibilityIdentifier("noteTypes.cards.preview.dark")
        } label: {
            Text("noteTypes.preview.appearance", bundle: .module)
        }
        .accessibilityIdentifier("noteTypes.cards.preview.appearance")
    }

    private var appearancePicker: some View {
        appearanceChoices(iconsOnly: true)
            .pickerStyle(.segmented)
            .labelsHidden()
            .fixedSize()
    }

    private func noteMenu(label: Bool, appearanceInMenu: Bool, clozeInMenu: Bool) -> some View {
        Menu {
            Button { preview.step(by: -1) } label: { Text("noteTypes.preview.previous", bundle: .module) }
                .disabled(!preview.canStep.previous)
            Button { preview.step(by: 1) } label: { Text("noteTypes.preview.next", bundle: .module) }
                .disabled(!preview.canStep.next)
            Divider()
            Toggle(isOn: Binding(get: { preview.fillEmpty }, set: { preview.setFillEmpty($0) })) {
                Text("noteTypes.preview.fillEmpty", bundle: .module)
            }
            if clozeInMenu {
                Divider()
                clozeChoices
            }
            if appearanceInMenu {
                Divider()
                appearanceChoices(iconsOnly: false).pickerStyle(.inline)
            }
        } label: {
            if !label {
                Label { Text("noteTypes.preview.note", bundle: .module) } icon: { Image(systemName: "doc.text") }
                    .labelStyle(.iconOnly)
            } else if let position = preview.notePosition {
                Text(verbatim: NoteTypesStrings.previewPosition(position.index, of: position.count, locale: locale))
                    .monospacedDigit()
            } else {
                Text("noteTypes.preview.noNotes", bundle: .module)
            }
        }
        .help(notePositionText ?? "")
        #if os(iOS)
        .menuStyle(.button)
        .buttonStyle(.borderless)
        #endif
        .fixedSize()
        .accessibilityLabel(Text("noteTypes.preview.note", bundle: .module))
        .accessibilityIdentifier("noteTypes.cards.preview.note")
    }

    /// "3 of 12" (the note menu's help where it shows only its symbol).
    private var notePositionText: String? {
        preview.notePosition.map { NoteTypesStrings.previewPosition($0.index, of: $0.count, locale: locale) }
    }

    private var clozeChoices: some View {
        Picker(selection: Binding(get: { preview.clozeNumber }, set: { preview.setClozeNumber($0) })) {
            ForEach(preview.clozeNumbers, id: \.self) { number in
                Text(verbatim: NoteTypesStrings.previewCloze(Int(number), locale: locale)).tag(number)
            }
        } label: {
            Text("noteTypes.preview.clozeNumber", bundle: .module)
        }
        .pickerStyle(.inline)
    }

    private var clozeMenu: some View {
        Menu {
            clozeChoices
        } label: {
            Text(verbatim: NoteTypesStrings.previewCloze(Int(preview.clozeNumber), locale: locale))
                .monospacedDigit()
        }
        #if os(iOS)
        .menuStyle(.button)
        .buttonStyle(.borderless)
        #endif
        .fixedSize()
        .accessibilityLabel(Text("noteTypes.preview.clozeNumber", bundle: .module))
        .accessibilityIdentifier("noteTypes.cards.preview.cloze")
    }

    // MARK: The page

    private var deleted: Bool {
        model.shownCard(id).flatMap { model.draft(id)?.cardRow($0)?.deleted } == true
    }

    @ViewBuilder private var page: some View {
        ZStack {
            if let pair = preview.pair {
                // The pages are in the window from the start: a page out of any
                // window never finishes drawing (its frames do not run), so the
                // first card would never be ready. Until it is, nothing shows.
                let drawn = preview.shown != nil
                CardWebView(pair: pair)
                    #if os(iOS)
                    // The card's background runs to the bottom edge, under the home indicator.
                    .ignoresSafeArea(.container, edges: .bottom)
                    #endif
                    .opacity(drawn ? 1 : 0)
                    // Under an error the last card stays, faded by a layer of the
                    // pane's own background, so the place keeps its shape. The page
                    // itself is never made translucent or untouchable: on the Mac
                    // that took the window toolbar's background away.
                    .overlay {
                        if covered {
                            Rectangle()
                                .fill(.background.opacity(0.75))
                                .accessibilityHidden(true)
                        }
                    }
                    .accessibilityElement(children: .contain)
                    // Tests and captures wait for the drawn card's identifier.
                    .accessibilityIdentifier(drawn ? "noteTypes.cards.preview.page" : "noteTypes.cards.preview.loading")
                    .accessibilityHidden(!drawn || covered)
            }
            if deleted {
                message(Text("noteTypes.preview.deleted", bundle: .module), detail: nil, symbol: "trash")
            } else if case .templateError(let side) = preview.problem {
                errorMessage(side)
            } else if preview.problem == .unavailable {
                message(Text("noteTypes.preview.unavailable", bundle: .module),
                        detail: Text("noteTypes.preview.unavailable.message", bundle: .module), symbol: "eye.slash")
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    /// The card on the page is not the draft's: it stays, faded, under the reason.
    private var covered: Bool { deleted || preview.problem != nil }

    /// The issue the status line shows for this card type and side, in its words.
    private func errorMessage(_ side: CardRenderSide) -> some View {
        let card = model.shownCard(id)
        let part: NoteTypeTemplatePart = side == .question ? .front : .back
        let issues = model.liveIssues(id).filter { $0.severity == .red && $0.card == card }
        let issue = issues.first { $0.part == part } ?? issues.first
        let fallback = side == .question ? Text("noteTypes.preview.error.front", bundle: .module) : Text("noteTypes.preview.error.back", bundle: .module)
        return Group {
            if let issue {
                let words = NoteTypesStrings.issue(issue, locale: locale)
                message(Text(verbatim: words.location), detail: words.message.isEmpty ? nil : Text(verbatim: words.message),
                        symbol: "exclamationmark.triangle")
            } else {
                message(fallback, detail: nil, symbol: "exclamationmark.triangle")
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("noteTypes.cards.preview.error")
    }

    /// The preview's message over (or in place of) the card, laid out as
    /// the system's content-unavailable view is. Not that view itself: on
    /// the Mac it took the window toolbar's background away (the columns'
    /// separators then ran up through the toolbar).
    private func message(_ title: Text, detail: Text?, symbol: String) -> some View {
        VStack(spacing: Spacing.small) {
            Image(systemName: symbol)
                .font(.largeTitle)
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)
            title
                .font(.headline)
            if let detail {
                detail
                    .foregroundStyle(.secondary)
            }
        }
        .multilineTextAlignment(.center)
        .padding(Spacing.large)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// The toolbar's Show Preview | Hide Preview, last in the Cards tab's
/// toolbar as the Fields tab's inspector toggle is (N22.2, N22.3); kept on
/// the device. Disabled, with the reason, while the window is too narrow
/// for the editor and the preview side by side.
struct PreviewToggle: View {
    let preview: NoteTypePreview
    private var fits: Bool { preview.fits }

    var body: some View {
        let key: LocalizedStringKey = preview.requested && fits ? "noteTypes.preview.hide" : "noteTypes.preview.show"
        Button {
            preview.requested.toggle()
        } label: {
            Label { Text(key, bundle: .module) } icon: { Image(systemName: "sidebar.trailing") }
        }
        .disabled(!fits)
        .help(fits ? Text(key, bundle: .module) : Text("noteTypes.preview.narrow", bundle: .module))
        .accessibilityIdentifier("noteTypes.cards.previewToggle")
    }
}

/// A label style chosen at run time.
private struct AnyLabelStyle: LabelStyle {
    private let make: (Configuration) -> AnyView
    init<S: LabelStyle>(_ style: S) { make = { AnyView(style.makeBody(configuration: $0)) } }
    func makeBody(configuration: Configuration) -> some View { make(configuration) }
}
