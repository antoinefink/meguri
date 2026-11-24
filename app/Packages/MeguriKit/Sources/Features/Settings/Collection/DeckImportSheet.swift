import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

/// U162's deck package import sheet (`package/transfer-sheets` X2): the
/// facts and options, the confirmation the apply asks for, progress with
/// Cancel, the result with its log, and each refusal's message and offer.
public struct DeckImportSheet: View {
    @Bindable private var model: DeckImportModel
    public init(model: DeckImportModel) { self.model = model }
    public var body: some View {
        DeckImportContent(presentation: DeckImportPresentation(model), options: $model.options, actions: .init(model))
    }
}

/// What X2.6 offers after a refusal.
enum DeckImportOffer: Equatable {
    case retry, anotherFile, withoutLearningProgress, withoutMerging

    static func offers(_ refusal: DeckImportModel.Refusal, source: DeckImportModel.RefusalSource?,
                       withScheduling: Bool) -> [DeckImportOffer] {
        switch refusal {
        case .sync: return [.retry]
        case .engine(let value):
            switch value {
            case .tooLarge(.rows, _, _) where source == .apply && withScheduling: return [.withoutLearningProgress, .anotherFile]
            case .notetypeKindConflict: return [.withoutMerging]
            case .insufficientSpace, .diskFull, .io, .stale, .busy, .safetyBackupFailed, .collectionUnavailable,
                 .staleGeneration, .engine:
                return [.retry]
            case .cancelled: return source == .apply ? [.retry] : []
            case .committedWithoutResult: return []
            default: return [.anotherFile]
            }
        }
    }

    var title: String {
        switch self {
        case .retry: "Try Again"
        case .anotherFile: "Choose Another File"
        case .withoutLearningProgress: "Import Without Learning Progress"
        case .withoutMerging: "Import Without Merging"
        }
    }
    var identifier: String {
        switch self {
        case .retry: "retry"
        case .anotherFile: "anotherFile"
        case .withoutLearningProgress: "withoutLearningProgress"
        case .withoutMerging: "withoutMerging"
        }
    }
}

/// Only the values the sheet draws: fixtures build it without a model.
struct DeckImportPresentation {
    var step: DeckImportModel.Step = .options
    var fileName = "Deck.apkg"
    var otherFilesGiven = false
    var facts: DeckImportFacts?
    var progressStage: DeckImportModel.ProgressStage?
    var progressCompleted: UInt64 = 0
    var progressTotal: UInt64?
    var progressVisible = false
    var cancellable = true
    var cancelling = false
    var confirmation: DeckImportPreview?
    var confirmationReasons = false
    var notetypeNames: [Int64: String] = [:]
    var syncParagraph: DeckImportModel.SyncParagraph?
    var cancelledNotice = false
    var result: DeckImportResult?
    var upload: MeguriStructureUpload?
    var refusal: DeckImportModel.Refusal?
    var refusalSource: DeckImportModel.RefusalSource?
    var canUndo = false
    var undoing = false
    var undone = false
    var log: [DeckImportLogCategory: [DeckImportLogEntry]] = [:]
    var moreLog: Set<DeckImportLogCategory> = []
    var canShowInBrowse = false

    init() {}
    @MainActor init(_ model: DeckImportModel) {
        step = model.step
        fileName = model.fileName ?? ""
        otherFilesGiven = model.otherFilesGiven
        facts = model.facts
        progressStage = model.progressStage
        progressCompleted = model.progressCompleted; progressTotal = model.progressTotal
        progressVisible = model.progressVisible
        cancellable = model.cancellable; cancelling = model.cancelling
        confirmation = model.confirmation?.preview
        confirmationReasons = model.confirmation.map { !$0.reasons.isEmpty } ?? false
        notetypeNames = model.notetypeNames
        syncParagraph = model.syncParagraph
        cancelledNotice = model.cancelledNotice
        result = model.result; upload = model.upload
        refusal = model.refusal; refusalSource = model.refusalSource
        canUndo = model.canUndo; undoing = model.undoing; undone = model.undone
        log = model.log
        moreLog = Set(DeckImportLogCategory.allCases.filter { model.moreLog($0) })
        canShowInBrowse = model.canShowInBrowse
    }

    var busy: Bool { step == .inspecting || step == .importing }
    /// The snappy rule: until the work is slow, the sheet keeps the form it showed.
    var showsProgress: Bool { busy && (progressVisible || facts == nil) }
    var hasLosses: Bool { confirmation.map { !$0.isEmpty } ?? false }

    var progressTitle: String { DeckImportText.progress(progressStage, file: fileName) }
    var progressFraction: (UInt64, UInt64)? {
        guard let total = progressTotal, total > 0 else { return nil }
        return (min(progressCompleted, total), total)
    }

    /// X2.5's lines, in their order, each only when not empty.
    struct Line: Identifiable {
        let category: DeckImportLogCategory?
        let text: String
        /// The categories whose entries the line lists.
        let categories: [DeckImportLogCategory]
        let showable: Bool
        var id: String { text }
    }
    var lines: [Line] {
        guard let result else { return [] }
        let log = result.log
        var lines: [Line] = []
        if log[.new] > 0 { lines.append(.init(category: .new, text: DeckImportText.added(log[.new]), categories: [.new], showable: true)) }
        if log[.duplicate] > 0 {
            lines.append(.init(category: .duplicate, text: DeckImportText.duplicate(log[.duplicate]), categories: [.duplicate], showable: true))
        }
        if log[.updated] > 0 {
            lines.append(.init(category: .updated, text: DeckImportText.updated(log[.updated]), categories: [.updated], showable: true))
        }
        if result.notImported > 0 {
            lines.append(.init(category: nil, text: DeckImportText.notImported(result.notImported),
                               categories: [.conflicting, .missingNotetype, .emptyFirstField], showable: false))
        }
        if log[.missingDeck] > 0 {
            lines.append(.init(category: .missingDeck, text: DeckImportText.defaultDeck(log[.missingDeck]),
                               categories: [.missingDeck], showable: true))
        }
        return lines
    }

    /// The counts of I14.5 that are not zero, as rows.
    var countRows: [(String, String)] {
        guard let counts = result?.counts else { return [] }
        var rows: [(String, String)] = []
        func add(_ label: String, _ value: UInt64) { if value > 0 { rows.append((label, value.formatted())) } }
        add("Cards added", counts.cardsAdded)
        add("Reviews added", counts.revlogAdded)
        add("Note types added", counts.notetypesAdded + counts.notetypesCopied)
        add("Note types updated", counts.notetypesUpdated)
        add("Note types removed", counts.notetypesRemoved)
        add("Decks added", counts.decksAdded)
        add("Decks updated", counts.decksUpdated)
        add("Presets added", counts.presetsAdded)
        add("Media files added", counts.mediaWritten)
        return rows
    }

    /// X2.5 for VoiceOver: the whole result in one reading.
    var resultSummary: String {
        guard let result else { return "" }
        return ([DeckImportText.importedTitle(fileName), DeckImportText.found(result.log.foundNotes)]
            + lines.map(\.text)).joined(separator: ". ")
    }

    var uploadPending: Bool {
        switch upload {
        case .pending?, .chooseCollection?, .acceptanceUnknown?, .setupInProgress?: true
        default: false
        }
    }
}

struct DeckImportActions {
    var importPackage: () -> Void = {}
    var confirm: () -> Void = {}
    var cancelConfirmation: () -> Void = {}
    var cancelWork: () -> Void = {}
    var close: () -> Void = {}
    var offer: (DeckImportOffer) -> Void = { _ in }
    var undo: () -> Void = {}
    var loadLog: (DeckImportLogCategory) -> Void = { _ in }
    var show: ([Int64]) -> Void = { _ in }

    init() {}
    @MainActor init(_ model: DeckImportModel) {
        importPackage = model.importPackage
        confirm = model.confirm
        cancelConfirmation = model.cancelConfirmation
        cancelWork = model.cancelWork
        close = model.done
        offer = { offer in
            switch offer {
            case .retry: model.retry()
            case .anotherFile: model.chooseAnotherFile()
            case .withoutLearningProgress: model.importWithoutLearningProgress()
            case .withoutMerging: model.importWithoutMerging()
            }
        }
        undo = model.undoImport
        loadLog = model.loadLog
        show = model.showInBrowse
    }
}

/// The production sheet, also drawn by the registered fixtures.
struct DeckImportContent: View {
    let presentation: DeckImportPresentation
    @Binding var options: DeckImportOptions
    let actions: DeckImportActions
    /// The result's lines whose entries show (fixtures open one).
    @State private var expanded: Set<String>

    init(presentation: DeckImportPresentation, options: Binding<DeckImportOptions>, actions: DeckImportActions,
         initiallyExpanded: Set<String> = []) {
        self.presentation = presentation
        _options = options
        self.actions = actions
        _expanded = State(initialValue: initiallyExpanded)
    }

    private static let id = "settings.importExport.deck"

    var body: some View {
        NavigationStack {
            Form { content }
                .formStyle(.grouped)
                .accessibilityIdentifier("\(Self.id).content")
                .navigationTitle(Text(navigationTitle))
                #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
                #endif
                .toolbar { toolbar }
                .disabled(presentation.undoing)
        }
        .presentationSizing(.form)
        .interactiveDismissDisabled(presentation.busy || presentation.step == .confirming || presentation.undoing)
        .onChange(of: presentation.step) { _, step in announce(step) }
        #if os(macOS)
        .onExitCommand { escape() }
        #endif
    }

    private var navigationTitle: String {
        switch presentation.step {
        case .result: "Import Complete"
        case .confirming: "Confirm Import"
        default: "Import Deck"
        }
    }

    // MARK: Content per step

    @ViewBuilder private var content: some View {
        if presentation.showsProgress {
            progressSection
        } else {
            switch presentation.step {
            case .idle, .inspecting: progressSection
            case .options, .importing: optionsSections
            case .confirming: confirmationSections
            case .result: resultSections
            case .refused: refusalSections
            case .importedWithoutResult: committedWithoutResultSection
            }
        }
    }

    private func heading(_ title: String, _ subtitle: String?) -> some View {
        VStack(alignment: .leading) {
            Text(verbatim: title)
                .font(.headline)
                .accessibilityAddTraits(.isHeader)
                .accessibilityIdentifier("\(Self.id).heading")
            if let subtitle {
                Text(verbatim: subtitle)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("\(Self.id).summary")
            }
        }
    }

    @ViewBuilder private var progressSection: some View {
        Section {
            Group {
                if let (done, total) = presentation.progressFraction {
                    ProgressView(value: Double(done), total: Double(total)) { Text(verbatim: presentation.progressTitle) }
                } else {
                    ProgressView { Text(verbatim: presentation.progressTitle) }
                }
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(Text(verbatim: presentation.progressTitle))
            .accessibilityValue(Text(verbatim: presentation.progressFraction.map { "\($0.0.formatted()) of \($0.1.formatted())" }
                ?? "In progress"))
            .accessibilityIdentifier("\(Self.id).progress")
        } footer: {
            if presentation.busy, !presentation.cancellable { Text(verbatim: DeckImportText.finishing) }
        }
    }

    // MARK: X2.1, X2.2

    @ViewBuilder private var optionsSections: some View {
        let facts = presentation.facts
        Section {
            heading(DeckImportText.importTitle(presentation.fileName), facts.map(DeckImportText.summary))
            if let facts {
                LabeledContent {
                    Text(verbatim: DeckImportText.list(facts.decks.map(DeckImportText.deckName)))
                } label: {
                    Text(verbatim: facts.decks.count == 1 ? "Deck" : "Decks")
                }
                .accessibilityIdentifier("\(Self.id).decks")
                LabeledContent {
                    Text(verbatim: DeckImportText.list(facts.notetypes))
                } label: {
                    Text(verbatim: facts.notetypes.count == 1 ? "Note Type" : "Note Types")
                }
                .accessibilityIdentifier("\(Self.id).notetypes")
                LabeledContent { Text(verbatim: DeckImportText.media(facts)) } label: { Text(verbatim: "Media") }
                    .accessibilityIdentifier("\(Self.id).media")
            }
        } footer: {
            VStack(alignment: .leading) {
                if let repairs = facts.flatMap({ DeckImportText.repairs($0.repaired) }) {
                    Text(verbatim: repairs).accessibilityIdentifier("\(Self.id).repairs")
                }
                if presentation.otherFilesGiven { Text(verbatim: DeckImportText.otherFiles(presentation.fileName)) }
                if presentation.cancelledNotice {
                    Text(verbatim: DeckImportText.cancelled).accessibilityIdentifier("\(Self.id).cancelled")
                }
            }
        }
        Section {
            Toggle(isOn: $options.withScheduling) {
                Text("import.apkg.withScheduling", bundle: .module)
                Text(verbatim: DeckImportText.schedulingHelp(facts))
            }
            .accessibilityIdentifier("\(Self.id).withScheduling")
            Toggle(isOn: $options.withDeckConfigs) {
                Text("import.apkg.withDeckConfigs", bundle: .module)
                Text(verbatim: DeckImportText.presetsHelp(facts))
            }
            .accessibilityIdentifier("\(Self.id).withDeckConfigs")
        }
        Section {
            Toggle(isOn: $options.mergeNotetypes) {
                Text("import.apkg.mergeNotetypes", bundle: .module)
                Text(verbatim: DeckImportText.mergeHelp)
            }
            .accessibilityIdentifier("\(Self.id).mergeNotetypes")
            updatePicker("import.apkg.updateNotes", selection: $options.updateNotes, id: "updateNotes")
            updatePicker("import.apkg.updateNotetypes", selection: $options.updateNotetypes, id: "updateNotetypes")
        } header: {
            Text(verbatim: "Existing Notes and Note Types")
        } footer: {
            Text(verbatim: DeckImportText.updatesFooter)
        }
        .disabled(presentation.busy)
    }

    private func updatePicker(_ key: LocalizedStringKey, selection: Binding<DeckImportUpdate>, id: String) -> some View {
        Picker(selection: selection) {
            Text("import.apkg.update.ifNewer", bundle: .module).tag(DeckImportUpdate.ifNewer)
            Text("import.apkg.update.always", bundle: .module).tag(DeckImportUpdate.always)
            Text("import.apkg.update.never", bundle: .module).tag(DeckImportUpdate.never)
        } label: {
            Text(key, bundle: .module)
        }
        .accessibilityIdentifier("\(Self.id).\(id)")
    }

    // MARK: X2.3

    @ViewBuilder private var confirmationSections: some View {
        let preview = presentation.confirmation ?? DeckImportPreview()
        Section {
            heading(DeckImportText.confirmTitle(presentation.fileName), nil)
            ForEach(Array(preview.emptiedFields.enumerated()), id: \.offset) { _, row in
                lossRow(DeckImportText.emptied(row))
            }
            ForEach(Array(preview.removedNotetypes.enumerated()), id: \.offset) { _, row in
                lossRow(DeckImportText.removed(row, names: presentation.notetypeNames))
            }
            ForEach(Array(preview.droppedValues.enumerated()), id: \.offset) { _, row in
                lossRow(DeckImportText.dropped(row, names: presentation.notetypeNames))
            }
        } footer: {
            if presentation.syncParagraph == nil {
                Text(verbatim: DeckImportText.backupLine).accessibilityIdentifier("\(Self.id).backup")
            }
        }
        if let paragraph = presentation.syncParagraph {
            Section {
                let lead = Text(verbatim: paragraph == .replaces ? DeckImportText.syncReplaces : DeckImportText.syncUnreachable).bold()
                let body = paragraph == .replaces ? DeckImportText.syncReplacesBody : DeckImportText.syncUnreachableBody
                Text("\(lead) \(body)")
                    .accessibilityIdentifier("\(Self.id).sync")
                Text(verbatim: DeckImportText.syncOtherDevices)
                    .accessibilityIdentifier("\(Self.id).syncOtherDevices")
            } footer: {
                Text(verbatim: DeckImportText.backupLine).accessibilityIdentifier("\(Self.id).backup")
            }
        }
    }

    private func lossRow(_ text: String) -> some View {
        Label {
            Text(verbatim: text)
        } icon: {
            Image(systemName: "minus.circle.fill").symbolRenderingMode(.multicolor).foregroundStyle(.red)
        }
        .foregroundStyle(.red)
        .accessibilityIdentifier("\(Self.id).loss")
    }

    // MARK: X2.5

    @ViewBuilder private var resultSections: some View {
        if let result = presentation.result {
            Section {
                heading(presentation.undone ? DeckImportText.undone : DeckImportText.importedTitle(presentation.fileName),
                        presentation.undone ? nil : DeckImportText.found(result.log.foundNotes))
                    .accessibilityElement(children: .combine)
                    .accessibilityLabel(Text(verbatim: presentation.undone ? DeckImportText.undone : presentation.resultSummary))
                if !presentation.undone {
                    ForEach(presentation.lines) { line in logLine(line, result: result) }
                }
            } footer: {
                if !presentation.undone {
                    VStack(alignment: .leading) {
                        if result.counts.mediaSkipped > 0 {
                            Text(verbatim: DeckImportText.mediaSkipped(result.counts.mediaSkipped))
                                .accessibilityIdentifier("\(Self.id).mediaSkipped")
                        }
                        if result.log.fieldsOmitted > 0 { Text(verbatim: DeckImportText.fieldsOmitted) }
                        if result.mediaPending {
                            Text(verbatim: DeckImportText.mediaPending).accessibilityIdentifier("\(Self.id).mediaPending")
                        }
                        if presentation.uploadPending { Text(verbatim: DeckImportText.uploadPending) }
                    }
                }
            }
            if !presentation.undone, !presentation.countRows.isEmpty {
                Section {
                    ForEach(presentation.countRows, id: \.0) { row in
                        LabeledContent { Text(verbatim: row.1) } label: { Text(verbatim: row.0) }
                    }
                }
                .accessibilityIdentifier("\(Self.id).counts")
            }
        }
    }

    @ViewBuilder private func logLine(_ line: DeckImportPresentation.Line, result: DeckImportResult) -> some View {
        let entries = line.categories.flatMap { category in
            (presentation.log[category] ?? []).map { (category, $0) }
        }
        DisclosureGroup(isExpanded: Binding(get: { expanded.contains(line.id) },
                                            set: { if $0 { expanded.insert(line.id) } else { expanded.remove(line.id) } })) {
            ForEach(Array(entries.enumerated()), id: \.offset) { _, item in
                entryRow(item.1, category: item.0, showable: line.showable)
            }
            if line.categories.contains(where: presentation.moreLog.contains) {
                Button { line.categories.forEach(actions.loadLog) } label: {
                    Text(verbatim: entries.isEmpty ? "Show Notes" : "Show More")
                }
                .accessibilityIdentifier("\(Self.id).more")
            }
        } label: {
            HStack {
                Text(verbatim: line.text)
                Spacer()
                if line.showable, presentation.canShowInBrowse {
                    Button { actions.show(entries.map(\.1.noteId)) } label: { Text(verbatim: "Show") }
                        .disabled(entries.isEmpty)
                        .help(Text(verbatim: "Show these notes in Browse"))
                        .accessibilityIdentifier("\(Self.id).show")
                }
            }
        }
        .accessibilityIdentifier("\(Self.id).line")
        .onAppear { line.categories.forEach(actions.loadLog) }
    }

    private func entryRow(_ entry: DeckImportLogEntry, category: DeckImportLogCategory, showable: Bool) -> some View {
        HStack {
            VStack(alignment: .leading) {
                if let first = entry.fields.first(where: { !$0.isEmpty }) ?? entry.fields.first {
                    Text(verbatim: first).lineLimit(2)
                } else {
                    Text(verbatim: DeckImportText.textNotShown).foregroundStyle(.secondary)
                }
                Text(verbatim: DeckImportText.reason(category)).font(.caption).foregroundStyle(.secondary)
            }
            Spacer()
            if showable, presentation.canShowInBrowse {
                Button { actions.show([entry.noteId]) } label: {
                    Label { Text(verbatim: "Show in Browse") } icon: { Image(systemName: "magnifyingglass") }
                        .labelStyle(.iconOnly)
                }
                .buttonStyle(.borderless)
                .help(Text(verbatim: "Show in Browse"))
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("\(Self.id).entry")
    }

    // MARK: X2.6

    @ViewBuilder private var refusalSections: some View {
        if let refusal = presentation.refusal {
            Section {
                Label {
                    VStack(alignment: .leading) {
                        Text(verbatim: DeckImportText.refusal(refusal, source: presentation.refusalSource,
                                                              file: presentation.fileName, withScheduling: options.withScheduling))
                        Text(verbatim: DeckImportText.unchanged).foregroundStyle(.secondary)
                    }
                } icon: {
                    Image(systemName: "exclamationmark.triangle.fill").symbolRenderingMode(.multicolor)
                }
                .accessibilityElement(children: .combine)
                .accessibilityIdentifier("\(Self.id).refusal")
            } header: {
                Text(verbatim: DeckImportText.importTitle(presentation.fileName))
            }
            let offers = DeckImportOffer.offers(refusal, source: presentation.refusalSource, withScheduling: options.withScheduling)
            if offers.count > 1 {
                Section {
                    ForEach(offers.dropFirst(), id: \.identifier) { offer in
                        Button { actions.offer(offer) } label: { Text(verbatim: offer.title) }
                            .accessibilityIdentifier("\(Self.id).\(offer.identifier)")
                    }
                }
            }
        }
    }

    private var committedWithoutResultSection: some View {
        Section {
            Label {
                Text(verbatim: DeckImportText.importedWithoutResult)
            } icon: {
                Image(systemName: "checkmark.circle.fill").symbolRenderingMode(.multicolor)
            }
            .accessibilityIdentifier("\(Self.id).committedWithoutResult")
        } header: {
            Text(verbatim: DeckImportText.importTitle(presentation.fileName))
        }
    }

    // MARK: Buttons

    @ToolbarContentBuilder private var toolbar: some ToolbarContent {
        switch presentation.step {
        case .idle, .inspecting, .importing:
            ToolbarItem(placement: .cancellationAction) {
                Button(action: actions.cancelWork) { Text(verbatim: "Cancel") }
                    .keyboardShortcut(.cancelAction)
                    .disabled(!presentation.cancellable || presentation.cancelling)
                    .accessibilityIdentifier("\(Self.id).cancel")
            }
            if presentation.facts != nil, presentation.step == .importing {
                ToolbarItem(placement: .confirmationAction) {
                    Button {} label: { Text(verbatim: "Import") }.disabled(true)
                        .accessibilityIdentifier("\(Self.id).import")
                }
            }
        case .options:
            ToolbarItem(placement: .cancellationAction) {
                Button(action: actions.close) { Text(verbatim: "Cancel") }
                    .keyboardShortcut(.cancelAction)
                    .accessibilityIdentifier("\(Self.id).cancel")
            }
            ToolbarItem(placement: .confirmationAction) {
                Button(action: actions.importPackage) { Text(verbatim: "Import") }
                    .keyboardShortcut(.defaultAction)
                    .accessibilityIdentifier("\(Self.id).import")
            }
        case .confirming:
            ToolbarItem(placement: .cancellationAction) {
                Button(action: actions.cancelConfirmation) { Text(verbatim: "Cancel") }
                    .keyboardShortcut(.cancelAction)
                    .accessibilityIdentifier("\(Self.id).cancel")
            }
            // X2.3, D3: never the default; destructive only with losses.
            ToolbarItem(placement: presentation.hasLosses ? .destructiveAction : .confirmationAction) {
                Button(role: presentation.hasLosses ? .destructive : nil, action: actions.confirm) {
                    Text(verbatim: presentation.syncParagraph == .replaces ? "Import and Upload" : "Import")
                }
                .accessibilityIdentifier("\(Self.id).confirm")
            }
        case .result:
            if presentation.canUndo, !presentation.undone {
                ToolbarItem(placement: .cancellationAction) {
                    Button(action: actions.undo) { Text(verbatim: "Undo Import") }
                        .accessibilityIdentifier("\(Self.id).undo")
                }
            }
            ToolbarItem(placement: .confirmationAction) {
                Button(action: actions.close) { Text(verbatim: "Done") }
                    .keyboardShortcut(.defaultAction)
                    .accessibilityIdentifier("\(Self.id).done")
            }
        case .refused:
            let offers = presentation.refusal.map {
                DeckImportOffer.offers($0, source: presentation.refusalSource, withScheduling: options.withScheduling)
            } ?? []
            ToolbarItem(placement: .cancellationAction) {
                Button(action: actions.close) { Text(verbatim: offers.isEmpty ? "Done" : "Cancel") }
                    .keyboardShortcut(.cancelAction)
                    .accessibilityIdentifier("\(Self.id).cancel")
            }
            if let first = offers.first {
                ToolbarItem(placement: .confirmationAction) {
                    Button { actions.offer(first) } label: { Text(verbatim: first.title) }
                        .keyboardShortcut(.defaultAction)
                        .accessibilityIdentifier("\(Self.id).\(first.identifier)")
                }
            }
        case .importedWithoutResult:
            ToolbarItem(placement: .confirmationAction) {
                Button(action: actions.close) { Text(verbatim: "Done") }
                    .keyboardShortcut(.defaultAction)
                    .accessibilityIdentifier("\(Self.id).done")
            }
        }
    }

    /// Esc on the result closes it, as Done does; elsewhere Cancel owns it.
    private func escape() {
        if presentation.step == .result || presentation.step == .importedWithoutResult { actions.close() }
    }

    private func announce(_ step: DeckImportModel.Step) {
        let text: String? = switch step {
        case .importing: "Importing"
        case .confirming: DeckImportText.confirmTitle(presentation.fileName)
        case .result: presentation.resultSummary
        case .refused: presentation.refusal.map {
            DeckImportText.refusal($0, source: presentation.refusalSource, file: presentation.fileName,
                                   withScheduling: options.withScheduling)
        }
        case .importedWithoutResult: DeckImportText.importedWithoutResult
        default: nil
        }
        if let text { AccessibilityNotification.Announcement(text).post() }
    }
}
