import AppModel
import CoreBridge
import SwiftUI

/// The first version's Deck Options page (`native-interface-contract` N20.9,
/// `sched/deck-options` O17, strings `decks/deck-options-strings` K6 and K10).
/// The shell pushes it in place of the deck page and passes `onClose`, which
/// returns to the deck page after a save, a discard or the end of the draft.
public struct DeckOptionsPage: View {
    @Bindable private var model: DeckOptionsPageModel
    private let onClose: () -> Void
    /// Four digits and a margin at the body size (`range` tops at 9999).
    @ScaledMetric(relativeTo: .body) private var fieldWidth: CGFloat = 64
    @FocusState private var focused: DeckOptionsPageRow?
    @Environment(\.locale) private var locale

    public init(model: DeckOptionsPageModel, onClose: @escaping () -> Void) {
        self.model = model
        self.onClose = onClose
    }

    public var body: some View {
        content
            .navigationTitle(Text("deckOptions.v1.title", bundle: .module))
            #if os(macOS)
            .navigationSubtitle(DeckOptionsStrings.path(model.deckName))
            .onExitCommand { leave() }
            #endif
            .toolbar { toolbar }
            // N20.9: while the draft differs, Cancel replaces back and swipe back is off.
            .navigationBarBackButtonHidden(model.hasChanges || model.phase == .saving)
            .confirmationDialog(Text("deckOptions.discard.title", bundle: .module), isPresented: Binding(
                get: { model.confirmingDiscard }, set: { if !$0 { model.keepEditing() } }), titleVisibility: .visible) {
                Button(role: .destructive) { model.discardChanges() } label: { Text("deckOptions.discard.discard", bundle: .module) }
                Button(role: .cancel) { model.keepEditing() } label: { Text("deckOptions.discard.keepEditing", bundle: .module) }
            }
            .alert(Text("deckOptions.saveFailed.title", bundle: .module), isPresented: Binding(
                get: { model.saveFailure != nil }, set: { if !$0 { model.saveFailure = nil } })) {
                Button(role: .cancel) { model.saveFailure = nil } label: { Text("deckOptions.saveFailed.ok", bundle: .module) }
            } message: {
                switch model.saveFailure {
                case .busy?: Text("deckOptions.saveFailed.busy", bundle: .module)
                default: Text("deckOptions.saveFailed.failed", bundle: .module)
                }
            }
            .onChange(of: model.outcome) { _, outcome in
                // The model already returned to the deck page; the view only acknowledges.
                guard outcome != nil else { return }
                model.acknowledgeOutcome()
                onClose()
            }
            .onChange(of: focused) { previous, _ in
                if let previous { model.commit(previous) }
            }
            .onAppear { model.locale = locale }
    }

    @ViewBuilder private var content: some View {
        switch model.phase {
        case .unavailable(let failure):
            ContentUnavailableView {
                Label { Text("deckOptions.unavailable.title", bundle: .module) } icon: { Image(systemName: "gearshape") }
            } description: {
                switch failure {
                case .notFound: Text("deckOptions.unavailable.notFound", bundle: .module)
                case .filtered: Text("deckOptions.unavailable.filtered", bundle: .module)
                case .failed: Text("deckOptions.unavailable.failed", bundle: .module)
                }
            } actions: {
                if failure == .failed {
                    Button { model.reload() } label: { Text("deckOptions.unavailable.tryAgain", bundle: .module) }
                        .accessibilityIdentifier("deckOptions.options.tryAgain")
                }
            }
        case .loading, .closed:
            form.redacted(reason: .placeholder).disabled(true)
        case .editing, .saving:
            form.disabled(model.phase == .saving)
        }
    }

    private var form: some View {
        Form {
            Section {
                presetPicker
            } footer: {
                Text(DeckOptionsStrings.presetFooter(count: model.selectedUseCount, locale: locale))
                    .accessibilityIdentifier("deckOptions.options.presetFooter")
            }
            Section {
                numberRow(.newPerDay)
                numberRow(.reviewsPerDay)
            } header: {
                Text("deckOptions.v1.section.dailyLimits", bundle: .module)
                    .accessibilityIdentifier("deckOptions.options.limitsHeader")
            } footer: {
                if model.ownLimits {
                    Text("deckOptions.v1.ownLimits", bundle: .module)
                        .accessibilityIdentifier("deckOptions.options.limitsFooter")
                }
            }
            Section {
                retentionRow
            } header: {
                Text("deckOptions.v1.section.memory", bundle: .module)
                    .accessibilityIdentifier("deckOptions.options.memoryHeader")
            } footer: {
                Text(verbatim: DeckOptionsStrings.memoryFooter(ownRetention: model.ownRetention, locale: locale))
                    .accessibilityIdentifier("deckOptions.options.memoryFooter")
            }
        }
        .formStyle(.grouped)
    }

    private var presetPicker: some View {
        Picker(selection: Binding(get: { model.selectedPreset }, set: { model.selectPreset($0) })) {
            ForEach(model.presets) { preset in
                Text(DeckOptionsStrings.usedBy(name: preset.name, count: model.useCount(preset), locale: locale))
                    .tag(preset.id)
            }
        } label: {
            Text("deckOptions.v1.preset", bundle: .module)
        } currentValueLabel: {
            Text(model.selected?.name ?? "").truncationMode(.middle)
        }
        .pickerStyle(.menu)
        // An error in the selected preset is fixed before another is chosen.
        .disabled(!model.canSwitchPreset)
        .accessibilityIdentifier("deckOptions.options.presetPicker")
    }

    /// The row itself is the form's row, so the system keeps its label and
    /// value on one line; its issues follow as rows of their own (N11.9).
    @ViewBuilder private func numberRow(_ row: DeckOptionsPageRow) -> some View {
        LabeledContent {
            field(row)
        } label: {
            label(row)
        }
        issueLines(row)
    }

    @ViewBuilder private var retentionRow: some View {
        LabeledContent {
            HStack {
                field(.desiredRetention)
                Stepper(onIncrement: { model.stepRetention(by: 1) }, onDecrement: { model.stepRetention(by: -1) }) {
                    label(.desiredRetention)
                }
                .labelsHidden()
                .accessibilityIdentifier("deckOptions.options.stepper.desiredRetention")
            }
        } label: {
            label(.desiredRetention)
        }
        issueLines(.desiredRetention)
    }

    private func label(_ row: DeckOptionsPageRow) -> Text {
        switch row {
        case .newPerDay: Text("deckOptions.v1.newPerDay", bundle: .module)
        case .reviewsPerDay: Text("deckOptions.v1.reviewsPerDay", bundle: .module)
        case .desiredRetention: Text("deckOptions.v1.desiredRetention", bundle: .module)
        }
    }

    private func field(_ row: DeckOptionsPageRow) -> some View {
        TextField(text: Binding(get: { model.fieldText[row] ?? "" }, set: { model.fieldText[row] = $0 })) {
            label(row)
        }
        .labelsHidden()
        .multilineTextAlignment(.trailing)
        // A value's width, scaled with the text: a text field's own ideal
        // width is wide enough that the row would put its value under the
        // label at compact width.
        .frame(width: fieldWidth)
        .focused($focused, equals: row)
        .onSubmit { model.commit(row) }
        #if os(iOS)
        .keyboardType(row == .desiredRetention ? .decimalPad : .numberPad)
        #endif
        .accessibilityLabel(label(row))
        .accessibilityHint(Text(model.issues(for: row).map { DeckOptionsStrings.issue($0, range: model.range(row), locale: locale) }
            .joined(separator: " ")))
        .accessibilityIdentifier("deckOptions.options.option.\(row.rawValue)")
    }

    /// O4's issues under their row, one symbol per severity (N11.9).
    private func issueLines(_ row: DeckOptionsPageRow) -> some View {
        ForEach(model.issues(for: row), id: \.self) { issue in
            Label {
                Text(DeckOptionsStrings.issue(issue, range: model.range(row), locale: locale))
                    .foregroundStyle(.primary)
            } icon: {
                switch issue.severity {
                case .error: Image(systemName: "xmark.octagon").foregroundStyle(.red)
                case .strongWarning: Image(systemName: "exclamationmark.octagon.fill").foregroundStyle(.red)
                case .warning: Image(systemName: "exclamationmark.triangle.fill").symbolRenderingMode(.multicolor)
                }
            }
            .font(.footnote)
            .accessibilityHidden(true)
            .accessibilityIdentifier("deckOptions.options.issue.\(row.rawValue)-\(String(describing: issue.code))")
        }
    }

    @ToolbarContentBuilder private var toolbar: some ToolbarContent {
        ToolbarItem(placement: .confirmationAction) {
            if model.phase == .saving {
                ProgressView()
            } else {
                Button { save() } label: { Text("deckOptions.v1.save", bundle: .module) }
                    .disabled(!model.canSave)
                    .keyboardShortcut("s", modifiers: .command)
                    .accessibilityIdentifier("deckOptions.options.save")
                    // O15.3: ⌘↩ saves too.
                    .background {
                        Button { save() } label: { EmptyView() }
                            .keyboardShortcut(.return, modifiers: .command)
                            .disabled(!model.canSave)
                            .hidden()
                    }
            }
        }
        if model.hasChanges {
            ToolbarItem(placement: .cancellationAction) {
                Button { leave() } label: { Text("deckOptions.cancel", bundle: .module) }
                    .disabled(model.phase == .saving)
                    .accessibilityIdentifier("deckOptions.options.cancel")
            }
        }
    }

    private func save() {
        // O15.3: the field being edited is committed before the save.
        if let focused { model.commit(focused) }
        model.save()
    }

    private func leave() {
        if let focused { model.commit(focused) }
        model.requestLeave()
    }
}

/// The page's formatted strings (K6, K10 and Meguri's issue texts).
public enum DeckOptionsStrings {
    /// The DeckOptions String Catalog's bundle.
    static var bundle: Bundle { .module }

    /// A deck path as the interface shows it, its components joined with " › " (N20.3).
    public static func path(_ name: String) -> String {
        name.components(separatedBy: "::").joined(separator: " \u{203A} ")
    }

    /// K6: "Default (used by 6 decks)", the count grouped in the locale's format.
    public static func usedBy(name: String, count: UInt64, locale: Locale) -> String {
        let grouped = count.formatted(.number.locale(locale))
        let plural = Int(clamping: count)
        return String(localized: "deckOptions.preset.usedBy", defaultValue: "\(name) (used by \(plural) \(grouped))",
            bundle: .module, locale: locale)
    }

    /// K10: "Used by N decks. Changes apply to all of them.", or "Used by this deck only." when N is 1.
    public static func presetFooter(count: UInt64, locale: Locale) -> String {
        let grouped = count.formatted(.number.locale(locale))
        let plural = Int(clamping: count)
        return String(localized: "deckOptions.v1.presetFooter", defaultValue: "\(plural) \(grouped)",
            bundle: .module, locale: locale)
    }

    /// Memory's footer; N20.9 adds the own-value sentence when the deck holds its own retention.
    public static func memoryFooter(ownRetention: Bool, locale: Locale) -> String {
        let footer = String(localized: "deckOptions.v1.desiredRetentionFooter", bundle: .module, locale: locale)
        guard ownRetention else { return footer }
        return footer + " " + String(localized: "deckOptions.v1.ownRetention", bundle: .module, locale: locale)
    }

    /// Meguri's wording of an O4 issue on one of the page's rows (K7).
    public static func issue(_ issue: DeckOptionsPageIssue, range: ClosedRange<Double>?, locale: Locale) -> String {
        let percent = issue.row == .desiredRetention
        func bound(_ value: Double) -> String {
            percent ? value.formatted(.percent.precision(.fractionLength(0...2)).locale(locale))
                : value.formatted(.number.locale(locale))
        }
        switch issue.code {
        case .outOfRange:
            let low = bound(range?.lowerBound ?? issue.params["min"].flatMap(Double.init) ?? 0)
            let high = bound(range?.upperBound ?? issue.params["max"].flatMap(Double.init) ?? 0)
            return percent
                ? String(localized: "deckOptions.issue.outOfRange.percent", defaultValue: "\(low) \(high)", bundle: .module, locale: locale)
                : String(localized: "deckOptions.issue.outOfRange.number", defaultValue: "\(low) \(high)", bundle: .module, locale: locale)
        case .notWhole:
            return String(localized: "deckOptions.issue.notWhole", bundle: .module, locale: locale)
        case .reviewsBelowTenTimesNew:
            let expected = issue.params["expected"].flatMap(Double.init).map { $0.formatted(.number.locale(locale)) } ?? ""
            return String(localized: "deckOptions.issue.reviewsBelowTenTimesNew", defaultValue: "\(expected)", bundle: .module, locale: locale)
        case .retentionLow:
            return issue.severity == .strongWarning
                ? String(localized: "deckOptions.issue.retentionLow.strong", bundle: .module, locale: locale)
                : String(localized: "deckOptions.issue.retentionLow", bundle: .module, locale: locale)
        case .retentionHigh:
            return issue.severity == .strongWarning
                ? String(localized: "deckOptions.issue.retentionHigh.strong", bundle: .module, locale: locale)
                : String(localized: "deckOptions.issue.retentionHigh", bundle: .module, locale: locale)
        default:
            return String(localized: "deckOptions.saveFailed.failed", bundle: .module, locale: locale)
        }
    }
}
