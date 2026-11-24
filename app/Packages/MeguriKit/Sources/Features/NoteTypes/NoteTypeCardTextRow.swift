import AppModel
import CoreBridge
import DesignSystem
import SwiftUI
#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// W10's Card Text row above the styling (`notetypes/workspace` W10 to
/// W10.2; N22.4): Font, Size, Alignment, Text Colour and Background for the
/// plain `.card` rule, each showing its declaration's value (or an empty
/// state) and writing only that value into the CSS, through the styling's
/// text view. Absent, not disabled, when the CSS has no single plain
/// `.card` rule.
struct NoteTypeCardTextRow: View {
    let model: NoteTypesModel
    let id: Int64
    let card: Int
    let css: String
    @Environment(\.layoutDirection) private var direction
    @State private var row: CardTextRow?
    @State private var read: String?
    @State private var sizeText = ""
    @State private var colourTask: Task<Void, Never>?

    private var rightToLeft: Bool { direction == .rightToLeft }
    private var editable: Bool { model.editable(id) }

    var body: some View {
        // A stack, not a Group: a Group with nothing in it runs no task, and the
        // row's content comes from that task.
        VStack(spacing: .zero) {
            if let row, read == css {
                content(row)
                    .padding(.horizontal, Spacing.small)
                    .padding(.vertical, Spacing.xSmall)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .disabled(!editable)
                    .accessibilityElement(children: .contain)
                    .accessibilityLabel(Text("noteTypes.cardText.title", bundle: .module))
                    .accessibilityIdentifier("noteTypes.cards.cardText")
                Divider()
            }
        }
        .task(id: css) {
            let found = await model.cardTextRow(css, rightToLeft: rightToLeft)
            guard !Task.isCancelled else { return }
            row = found
            read = css
            sizeText = found?.size.map { $0.formatted(.number.precision(.fractionLength(0...1))) } ?? ""
        }
    }

    @ViewBuilder private func content(_ row: CardTextRow) -> some View {
        // The title goes first where the column is narrow, then the font menu narrows.
        // Narrower still, the controls take two lines: nothing clips.
        ViewThatFits(in: .horizontal) {
            controls(row, title: true, fontWidth: Self.fontMenuWidth)
            controls(row, title: false, fontWidth: Self.fontMenuWidth)
            controls(row, title: false, fontWidth: Self.fontMenuNarrow)
            VStack(alignment: .leading, spacing: Spacing.xSmall) {
                HStack(spacing: Spacing.small) {
                    fontMenu(row.font, width: Self.fontMenuNarrow)
                    sizeField
                }
                HStack(spacing: Spacing.small) {
                    alignment(row.alignment)
                    colourControls(row)
                }
            }
            .fixedSize()
        }
    }

    private func controls(_ row: CardTextRow, title: Bool, fontWidth: CGFloat) -> some View {
        HStack(spacing: Spacing.small) {
            if title {
                Text("noteTypes.cardText.title", bundle: .module)
                    .foregroundStyle(.secondary)
                    .fixedSize()
            }
            fontMenu(row.font, width: fontWidth)
            sizeField
            alignment(row.alignment)
            colourControls(row)
        }
        .fixedSize()
    }

    @ViewBuilder private func colourControls(_ row: CardTextRow) -> some View {
        colourWell(row.textColour, opacity: true, label: "noteTypes.cardText.textColour",
                   identifier: "noteTypes.cards.cardText.textColour") { .textColour($0) }
        colourWell(row.background, opacity: false, label: "noteTypes.cardText.background",
                   identifier: "noteTypes.cards.cardText.background") { .background($0) }
    }

    private func change(_ change: CardTextChange) {
        let model = model, id = id, card = card, rightToLeft = rightToLeft
        Task { await model.changeCardText(id, card: card, change, rightToLeft: rightToLeft) }
    }

    // MARK: Font

    private func fontMenu(_ font: CardTextFont?, width: CGFloat) -> some View {
        Menu {
            Picker(selection: Binding<CardTextFont?>(get: { font }, set: { if let value = $0 { change(.font(value)) } })) {
                Text("noteTypes.cardText.system", bundle: .module).tag(Optional(CardTextFont.system))
                Divider()
                ForEach(Self.families, id: \.self) { family in
                    Text(verbatim: family).tag(Optional(CardTextFont.family(family)))
                }
            } label: {
                Text("noteTypes.cardText.font", bundle: .module)
            }
            .pickerStyle(.inline)
        } label: {
            switch font {
            case .system?: Text("noteTypes.cardText.system", bundle: .module)
            case .family(let name)?: Text(verbatim: name)
            case nil: Text("noteTypes.cardText.font", bundle: .module).foregroundStyle(.secondary)
            }
        }
        #if os(iOS)
        .menuStyle(.button)
        .buttonStyle(.borderless)
        #endif
        .frame(width: width)
        .help(Text("noteTypes.cardText.font", bundle: .module))
        .accessibilityLabel(Text("noteTypes.cardText.font", bundle: .module))
        .accessibilityIdentifier("noteTypes.cards.cardText.font")
    }

    /// The system's font families, read once.
    private static let families: [String] = {
        #if os(macOS)
        NSFontManager.shared.availableFontFamilies.sorted { $0.localizedStandardCompare($1) == .orderedAscending }
        #else
        UIFont.familyNames.sorted { $0.localizedStandardCompare($1) == .orderedAscending }
        #endif
    }()

    private static let fontMenuWidth: CGFloat = 150
    private static let fontMenuNarrow: CGFloat = 100

    // MARK: Size

    private var sizeField: some View {
        HStack(spacing: Spacing.xxSmall) {
            TextField(text: $sizeText) { Text("noteTypes.cardText.size", bundle: .module) }
                .labelsHidden()
                .multilineTextAlignment(.trailing)
                .monospacedDigit()
                .frame(width: sizeFieldWidth)
                .onSubmit { commitSize() }
                .accessibilityIdentifier("noteTypes.cards.cardText.size")
            Stepper {
                Text("noteTypes.cardText.size", bundle: .module)
            } onIncrement: {
                step(by: 1)
            } onDecrement: {
                step(by: -1)
            }
            .labelsHidden()
            .accessibilityIdentifier("noteTypes.cards.cardText.sizeStepper")
        }
        .help(Text("noteTypes.cardText.size", bundle: .module))
    }

    /// Room for three digits at any text size.
    @ScaledMetric(relativeTo: .body) private var sizeFieldWidth: CGFloat = 40

    private func commitSize() {
        let typed = sizeText.trimmingCharacters(in: .whitespaces).replacingOccurrences(of: ",", with: ".")
        guard let points = Double(typed), points > 0, points != row?.size else { return }
        change(.size(points))
    }

    private func step(by delta: Double) {
        let points = max(1, (row?.size ?? Self.defaultSize) + delta)
        sizeText = points.formatted(.number.precision(.fractionLength(0...1)))
        change(.size(points))
    }

    /// Where a step starts when the rule has no size: the page's default.
    private static let defaultSize: Double = 16

    // MARK: Alignment

    private func alignment(_ value: CardTextAlignment?) -> some View {
        Picker(selection: Binding<CardTextAlignment?>(get: { value }, set: { if let value = $0 { change(.alignment(value)) } })) {
            Label { Text("noteTypes.cardText.leading", bundle: .module) } icon: { Image(systemName: "text.alignleft") }
                .labelStyle(.iconOnly)
                .tag(Optional(CardTextAlignment.leading))
                .accessibilityIdentifier("noteTypes.cards.cardText.alignLeading")
            Label { Text("noteTypes.cardText.centre", bundle: .module) } icon: { Image(systemName: "text.aligncenter") }
                .labelStyle(.iconOnly)
                .tag(Optional(CardTextAlignment.center))
                .accessibilityIdentifier("noteTypes.cards.cardText.alignCentre")
            Label { Text("noteTypes.cardText.trailing", bundle: .module) } icon: { Image(systemName: "text.alignright") }
                .labelStyle(.iconOnly)
                .tag(Optional(CardTextAlignment.trailing))
                .accessibilityIdentifier("noteTypes.cards.cardText.alignTrailing")
        } label: {
            Text("noteTypes.cardText.alignment", bundle: .module)
        }
        .pickerStyle(.segmented)
        .labelsHidden()
        .fixedSize()
        .help(Text("noteTypes.cardText.alignment", bundle: .module))
        .accessibilityIdentifier("noteTypes.cards.cardText.alignment")
    }

    // MARK: Colours

    /// A colour control; the system panel reports every step of a drag, so a
    /// change is written once the colour rests for a moment, as one edit.
    private func colourWell(_ colour: CardTextColour?, opacity: Bool, label: LocalizedStringKey, identifier: String,
                            change make: @escaping (CardTextColour) -> CardTextChange) -> some View {
        ColorPicker(selection: Binding<CGColor>(
            get: { colour?.cgColor ?? CardTextColour.unsetWell },
            set: { value in
                guard let picked = Self.colour(value, opacity: opacity) else { return }
                colourTask?.cancel()
                colourTask = Task {
                    guard (try? await Task.sleep(for: .milliseconds(250))) != nil else { return }
                    change(make(picked))
                }
            }), supportsOpacity: opacity) {
            Text(label, bundle: .module)
        }
        .labelsHidden()
        // The row's colour controls at the size of its other controls.
        .controlSize(.small)
        .help(Text(label, bundle: .module))
        .accessibilityIdentifier(identifier)
    }

    /// The picked colour in 8-bit sRGB (opaque where the control allows no opacity).
    private static func colour(_ value: CGColor, opacity: Bool) -> CardTextColour? {
        guard let space = CGColorSpace(name: CGColorSpace.sRGB),
              let converted = value.converted(to: space, intent: .defaultIntent, options: nil),
              let parts = converted.components, parts.count >= 3 else { return nil }
        func byte(_ part: CGFloat) -> UInt8 { UInt8((min(max(part, 0), 1) * 255).rounded()) }
        let alpha = opacity && parts.count >= 4 ? byte(parts[3]) : 255
        return CardTextColour(red: byte(parts[0]), green: byte(parts[1]), blue: byte(parts[2]), alpha: alpha)
    }
}
