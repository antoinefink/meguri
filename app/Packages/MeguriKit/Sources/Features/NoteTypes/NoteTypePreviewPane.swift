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

    /// The bar's controls; where half a screen is too narrow for all of
    /// them (iPad in portrait), Light | Dark moves into the note menu.
    private var bar: some View {
        ViewThatFits(in: .horizontal) {
            barLine(appearanceInMenu: false)
            barLine(appearanceInMenu: true)
        }
    }

    private func barLine(appearanceInMenu: Bool) -> some View {
        HStack(spacing: Spacing.small) {
            noteMenu(appearanceInMenu: appearanceInMenu)
            if model.draft(id)?.isCloze == true { clozeMenu }
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
            Label { Text("noteTypes.preview.dark", bundle: .module) } icon: { Image(systemName: "moon") }
                .labelStyle(iconsOnly ? AnyLabelStyle(.iconOnly) : AnyLabelStyle(.titleAndIcon))
                .tag(CardRuntime.Appearance.dark)
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

    private func noteMenu(appearanceInMenu: Bool) -> some View {
        Menu {
            Button { preview.step(by: -1) } label: { Text("noteTypes.preview.previous", bundle: .module) }
                .disabled(!preview.canStep.previous)
            Button { preview.step(by: 1) } label: { Text("noteTypes.preview.next", bundle: .module) }
                .disabled(!preview.canStep.next)
            Divider()
            Toggle(isOn: Binding(get: { preview.fillEmpty }, set: { preview.setFillEmpty($0) })) {
                Text("noteTypes.preview.fillEmpty", bundle: .module)
            }
            if appearanceInMenu {
                Divider()
                appearanceChoices(iconsOnly: false).pickerStyle(.inline)
            }
        } label: {
            if let position = preview.notePosition {
                Text(verbatim: NoteTypesStrings.previewPosition(position.index, of: position.count, locale: locale))
                    .monospacedDigit()
            } else {
                Text("noteTypes.preview.noNotes", bundle: .module)
            }
        }
        #if os(iOS)
        .menuStyle(.button)
        .buttonStyle(.borderless)
        #endif
        .fixedSize()
        .accessibilityLabel(Text("noteTypes.preview.note", bundle: .module))
        .accessibilityIdentifier("noteTypes.cards.preview.note")
    }

    private var clozeMenu: some View {
        Menu {
            Picker(selection: Binding(get: { preview.clozeNumber }, set: { preview.setClozeNumber($0) })) {
                ForEach(preview.clozeNumbers, id: \.self) { number in
                    Text(verbatim: NoteTypesStrings.previewCloze(Int(number), locale: locale)).tag(number)
                }
            } label: {
                Text("noteTypes.preview.clozeNumber", bundle: .module)
            }
            .pickerStyle(.inline)
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
            if let pair = preview.pair, preview.shown != nil {
                CardWebView(pair: pair)
                    // Under an error the last card stays, faded, so the place keeps its shape.
                    .opacity(covered ? 0.25 : 1)
                    .allowsHitTesting(!covered)
                    .accessibilityElement(children: .contain)
                    // Present once a card is on the page: tests and captures wait for it.
                    .accessibilityIdentifier("noteTypes.cards.preview.page")
                    .accessibilityHidden(covered)
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
        .accessibilityIdentifier("noteTypes.cards.preview.error")
    }

    private func message(_ title: Text, detail: Text?, symbol: String) -> some View {
        ContentUnavailableView {
            Label { title } icon: { Image(systemName: symbol) }
        } description: {
            if let detail { detail }
        }
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
