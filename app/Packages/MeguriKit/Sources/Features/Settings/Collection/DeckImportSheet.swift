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
        // MP7.1: after these two the sync status owns the next step; a retry
        // here could import the structural change without Meguri Sync.
        case .sync(.fullSyncRequired), .sync(.uploadPending): return []
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
    /// The learning progress option of the apply a refusal answered.
    var appliedScheduling: Bool?

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
        appliedScheduling = model.appliedOptions?.withScheduling
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
    /// The amount done, in words, for a stage that counts files.
    var progressDetail: String? {
        guard progressStage == .media, let (done, total) = progressFraction else { return nil }
        return "\(done.formatted()) of \(DeckImportText.count(total, "file", "files"))"
    }

    /// The sheet's title, in the window's title bar: the file it is about.
    var title: String {
        switch step {
        case .confirming: DeckImportText.confirmTitle(fileName)
        case .result: undone ? DeckImportText.undoneTitle : DeckImportText.importedTitle(fileName)
        case .importedWithoutResult: DeckImportText.importedTitle(fileName)
        default: DeckImportText.importTitle(fileName)
        }
    }
    var subtitle: String? {
        switch step {
        case .options, .importing: facts.map(DeckImportText.summary)
        case .result: undone ? nil : result.map { DeckImportText.found($0.log.foundNotes) }
        default: nil
        }
    }

    /// X2.3's rows, in the preview's order, each with its own id.
    var lossRows: [(id: String, text: String)] {
        guard let preview = confirmation else { return [] }
        return preview.emptiedFields.enumerated().map { ("emptied\($0.offset)", DeckImportText.emptied($0.element)) }
            + preview.removedNotetypes.enumerated().map { ("removed\($0.offset)", DeckImportText.removed($0.element, names: notetypeNames)) }
            + preview.droppedValues.enumerated().map { ("dropped\($0.offset)", DeckImportText.dropped($0.element, names: notetypeNames)) }
    }

    /// X2.5's lines, in their order, each only when not empty.
    struct Line: Identifiable {
        let category: DeckImportLogCategory?
        let text: String
        /// The categories whose entries the line lists.
        let categories: [DeckImportLogCategory]
        let showable: Bool
        var id: String { category.map { "\($0)" } ?? "notImported" }
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
    var countRows: [(id: String, label: String, value: String)] {
        guard let counts = result?.counts else { return [] }
        var rows: [(id: String, label: String, value: String)] = []
        func add(_ label: String, _ value: UInt64) {
            guard value > 0 else { return }
            let words = label.split(separator: " ").map(String.init)
            let id = (words.first?.lowercased() ?? "") + words.dropFirst().map(\.capitalized).joined()
            rows.append((id, label, value.formatted()))
        }
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
    var showLine: ([DeckImportLogCategory]) -> Void = { _ in }

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
        showLine = model.showCategories
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

    private static let id = "settings.deckImport"

    var body: some View {
        NavigationStack {
            Form { content }
                .formStyle(.grouped)
                .accessibilityIdentifier("\(Self.id).content")
                .navigationTitle(Text(verbatim: presentation.title))
                .navigationSubtitle(Text(verbatim: presentation.subtitle ?? ""))
                #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
                #endif
                .toolbar { toolbar }
                .disabled(presentation.undoing)
        }
        // The sheet is as tall as what it shows, up to the window's room.
        .presentationSizing(.form.fitted(horizontal: false, vertical: true))
        .interactiveDismissDisabled(presentation.busy || presentation.step == .confirming || presentation.undoing)
        .onChange(of: presentation.step) { _, step in announce(step) }
        #if os(macOS)
        .onExitCommand { escape() }
        #endif
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

    @ViewBuilder private var progressSection: some View {
        Section {
            Group {
                if let (done, total) = presentation.progressFraction {
                    ProgressView(value: Double(done), total: Double(total)) {
                        Text(verbatim: presentation.progressTitle)
                    } currentValueLabel: {
                        if let detail = presentation.progressDetail { Text(verbatim: detail) }
                    }
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
            if let facts {
                LabeledContent {
                    Text(verbatim: DeckImportText.decks(facts.decks)).lineLimit(1).truncationMode(.middle)
                } label: {
                    Text(verbatim: facts.decks.count == 1 ? "Deck" : "Decks")
                }
                .help(Text(verbatim: facts.decks.map(DeckImportText.deckName).joined(separator: "\n")))
                .accessibilityIdentifier("\(Self.id).decks")
                LabeledContent {
                    Text(verbatim: DeckImportText.list(facts.notetypes, shown: 1)).lineLimit(1).truncationMode(.middle)
                } label: {
                    Text(verbatim: facts.notetypes.count == 1 ? "Note Type" : "Note Types")
                }
                .help(Text(verbatim: facts.notetypes.joined(separator: "\n")))
                .accessibilityIdentifier("\(Self.id).notetypes")
                LabeledContent { Text(verbatim: DeckImportText.media(facts)) } label: { Text(verbatim: "Media") }
                    .accessibilityIdentifier("\(Self.id).media")
            }
        } footer: {
            let notes = [facts.flatMap { DeckImportText.repairs($0.repaired) },
                         presentation.otherFilesGiven ? DeckImportText.otherFiles(presentation.fileName) : nil,
                         presentation.cancelledNotice ? DeckImportText.cancelled : nil].compactMap { $0 }
            if !notes.isEmpty {
                Text(verbatim: notes.joined(separator: " ")).accessibilityIdentifier("\(Self.id).notes")
            }
        }
        Section {
            Toggle(isOn: Binding(get: { options.withScheduling && facts?.hasScheduling != false },
                                 set: { options.withScheduling = $0 })) {
                Text("import.apkg.withScheduling", bundle: .module)
                Text(verbatim: DeckImportText.schedulingHelp(facts))
            }
            .disabled(facts?.hasScheduling == false)
            .accessibilityIdentifier("\(Self.id).withScheduling")
            Toggle(isOn: Binding(get: { options.withDeckConfigs && facts?.hasPresets != false },
                                 set: { options.withDeckConfigs = $0 })) {
                Text("import.apkg.withDeckConfigs", bundle: .module)
                Text(verbatim: DeckImportText.presetsHelp(facts))
            }
            .disabled(facts?.hasPresets == false)
            .accessibilityIdentifier("\(Self.id).withDeckConfigs")
        }
        .disabled(presentation.busy)
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
        let rows = presentation.lossRows
        if !rows.isEmpty {
            Section {
                ForEach(rows, id: \.id) { row in
                    Label {
                        Text(verbatim: row.text)
                    } icon: {
                        Image(systemName: "minus.circle.fill").symbolRenderingMode(.multicolor).foregroundStyle(.red)
                    }
                    .foregroundStyle(.red)
                    .accessibilityIdentifier("\(Self.id).loss.\(row.id)")
                }
            } header: {
                Text(verbatim: "Deleted by This Import")
            } footer: {
                if presentation.syncParagraph == nil {
                    Text(verbatim: DeckImportText.backupLine).accessibilityIdentifier("\(Self.id).backup")
                }
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

    // MARK: X2.5

    @ViewBuilder private var resultSections: some View {
        if let result = presentation.result {
            if presentation.undone {
                Section {
                    Text(verbatim: DeckImportText.undone).accessibilityIdentifier("\(Self.id).undone")
                }
            } else {
                Section {
                    ForEach(presentation.lines) { line in logLine(line) }
                } footer: {
                    let notes = [result.counts.mediaSkipped > 0 ? DeckImportText.mediaSkipped(result.counts.mediaSkipped) : nil,
                                 result.log.fieldsOmitted > 0 ? DeckImportText.fieldsOmitted : nil,
                                 result.mediaPending ? DeckImportText.mediaPending : nil,
                                 presentation.uploadPending ? DeckImportText.uploadPending : nil].compactMap { $0 }
                    if !notes.isEmpty {
                        Text(verbatim: notes.joined(separator: " ")).accessibilityIdentifier("\(Self.id).resultNotes")
                    }
                }
                .accessibilityElement(children: .contain)
                .accessibilityLabel(Text(verbatim: presentation.resultSummary))
                if !presentation.countRows.isEmpty {
                    Section {
                        ForEach(presentation.countRows, id: \.id) { row in
                            LabeledContent { Text(verbatim: row.value) } label: { Text(verbatim: row.label) }
                                .accessibilityIdentifier("\(Self.id).count.\(row.id)")
                        }
                    }
                }
            }
        }
    }

    @ViewBuilder private func logLine(_ line: DeckImportPresentation.Line) -> some View {
        let entries = line.categories.flatMap { category in
            (presentation.log[category] ?? []).map { (category, $0) }
        }
        DisclosureGroup(isExpanded: Binding(get: { expanded.contains(line.id) },
                                            set: { if $0 { expanded.insert(line.id) } else { expanded.remove(line.id) } })) {
            ForEach(Array(entries.enumerated()), id: \.offset) { index, item in
                entryRow(item.1, category: item.0, showable: line.showable, index: index, line: line.id)
            }
            if line.categories.contains(where: presentation.moreLog.contains) {
                Button { line.categories.forEach(actions.loadLog) } label: {
                    Text(verbatim: entries.isEmpty ? "Show Notes" : "Show More")
                }
                .buttonStyle(.borderless)
                .accessibilityIdentifier("\(Self.id).more.\(line.id)")
            }
        } label: {
            // The Mac merges a disclosure label's children: one identifier for the row.
            HStack {
                Text(verbatim: line.text)
                Spacer()
                if line.showable, presentation.canShowInBrowse {
                    Button { actions.showLine(line.categories) } label: { Text(verbatim: "Show in Browse") }
                        .buttonStyle(.borderless)
                }
            }
            .accessibilityIdentifier("\(Self.id).line.\(line.id)")
        }
        .onChange(of: expanded.contains(line.id), initial: true) { _, open in
            if open { line.categories.forEach(actions.loadLog) }
        }
    }

    private func entryRow(_ entry: DeckImportLogEntry, category: DeckImportLogCategory, showable: Bool, index: Int,
                          line: String) -> some View {
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
        .accessibilityIdentifier("\(Self.id).entry.\(line)\(index)")
    }

    // MARK: X2.6

    private var refusalScheduling: Bool { presentation.appliedScheduling ?? options.withScheduling }

    @ViewBuilder private var refusalSections: some View {
        if let refusal = presentation.refusal {
            Section {
                Label {
                    VStack(alignment: .leading) {
                        Text(verbatim: DeckImportText.refusal(refusal, source: presentation.refusalSource,
                                                              file: presentation.fileName, withScheduling: refusalScheduling))
                        Text(verbatim: DeckImportText.unchanged).foregroundStyle(.secondary)
                    }
                } icon: {
                    Image(systemName: "exclamationmark.triangle.fill").symbolRenderingMode(.multicolor)
                }
                .accessibilityElement(children: .combine)
                .accessibilityIdentifier("\(Self.id).refusal")
            }
            let offers = DeckImportOffer.offers(refusal, source: presentation.refusalSource, withScheduling: refusalScheduling)
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
        }
    }

    // MARK: Buttons

    /// X2.3, D3: the confirmation's button is never the default, and
    /// destructive only with losses.
    private var confirmButton: some View {
        Button(role: presentation.hasLosses ? .destructive : nil, action: actions.confirm) {
            Text(verbatim: presentation.syncParagraph == .replaces ? "Import and Upload" : "Import")
        }
        .accessibilityIdentifier("\(Self.id).confirm")
    }

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
            #if os(macOS)
            ToolbarItem(placement: .cancellationAction) {
                Button(action: actions.cancelConfirmation) { Text(verbatim: "Cancel") }
                    .keyboardShortcut(.cancelAction)
                    .accessibilityIdentifier("\(Self.id).cancel")
            }
            // A confirmation-placed button is the Mac's default button; this one
            // never is. It sits at the leading edge, as the Mac's "Don't Save" does.
            ToolbarItem(placement: .destructiveAction) { confirmButton }
            #else
            ToolbarItem(placement: .cancellationAction) {
                Button(action: actions.cancelConfirmation) { Text(verbatim: "Cancel") }
                    .accessibilityIdentifier("\(Self.id).cancel")
            }
            ToolbarItem(placement: .confirmationAction) { confirmButton }
            #endif
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
                DeckImportOffer.offers($0, source: presentation.refusalSource, withScheduling: refusalScheduling)
            } ?? []
            ToolbarItem(placement: offers.isEmpty ? .confirmationAction : .cancellationAction) {
                Button(action: actions.close) { Text(verbatim: offers.isEmpty ? "Done" : "Cancel") }
                    .keyboardShortcut(offers.isEmpty ? .defaultAction : .cancelAction)
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
                                   withScheduling: refusalScheduling)
        }
        case .importedWithoutResult: DeckImportText.importedWithoutResult
        default: nil
        }
        if let text { AccessibilityNotification.Announcement(text).post() }
    }
}
