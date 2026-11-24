import CoreBridge
import Foundation
import Observation

// U277: Find and Replace… (`browser/workspace` V9, `browser/find-replace`
// F2 to F11) and Find Duplicates… (V10), each a sheet on the rows it opened
// on, at that revision: a list that moved on is refused by the core and
// nothing is written (`browser/bulk-framework` K1). Every count, preview and
// group comes from the core; nothing here loops over notes.

/// A note type as the two sheets' field menus list it.
public nonisolated struct BrowseNotetypeFields: Hashable, Sendable {
    public let id: Int64
    public let name: String
    /// The field names in field order.
    public let fields: [String]
    /// The Sort Field's ordinal.
    public let sortField: Int

    public init(id: Int64, name: String, fields: [String], sortField: Int) {
        self.id = id
        self.name = name
        self.fields = fields
        self.sortField = sortField
    }

    /// The Sort Field's name, when the note type has that field.
    public var sortFieldName: String? { fields.indices.contains(sortField) ? fields[sortField] : nil }
}

/// One entry of Find and Replace's In menu or Find Duplicates' Field menu.
public nonisolated struct BrowseFieldEntry: Identifiable, Hashable, Sendable {
    public enum Kind: Hashable, Sendable {
        case allFields, tags
        case field(String)

        /// How the device remembers it (F2).
        var memory: String {
            switch self {
            case .allFields: "allFields"
            case .tags: "tags"
            case .field(let name): "field:" + name
            }
        }

        var scope: FindReplaceScope {
            switch self {
            case .allFields: .allFields
            case .tags: .tags
            case .field(let name): .field(name)
            }
        }
    }

    /// Its position in the menu.
    public let id: Int
    public let kind: Kind
    /// The note type it is listed under when the menu groups its fields
    /// (more than one note type), nil otherwise.
    public let group: String?

    /// The menu's entries: All Fields first and Tags last when `ends`, and
    /// between them each note type's fields in field order, under the note
    /// type's name when there are several (V9, V10).
    static func menu(_ types: [BrowseNotetypeFields], ends: Bool) -> [BrowseFieldEntry] {
        var kinds: [(Kind, String?)] = ends ? [(.allFields, nil)] : []
        for type in types {
            for field in type.fields { kinds.append((.field(field), types.count > 1 ? type.name : nil)) }
        }
        if ends { kinds.append((.tags, nil)) }
        return kinds.enumerated().map { BrowseFieldEntry(id: $0.offset, kind: $0.element.0, group: $0.element.1) }
    }
}

/// The preview's state (V9): counting, the count and list, or why there is
/// none.
public nonisolated enum BrowseFindReplacePreviewState: Equatable, Sendable {
    /// Not read yet for the inputs shown; the last preview stays shown.
    case counting
    case ready(FindReplacePreview)
    /// The inputs cannot run: an invalid pattern, or a Tags replacement
    /// holding a space (F10), shown inline; the button is disabled.
    case invalid(BrowseFindReplaceInvalid)
    /// A note the target holds is damaged (F6 step 7): Check Database
    /// repairs it.
    case damaged
    /// The rows moved on since the sheet opened (K1).
    case rowsChanged
    case failed
}

/// Why the inputs cannot run (`browser/browse-strings` W2.1).
public nonisolated enum BrowseFindReplaceInvalid: Hashable, Sendable {
    /// F4 step 5 with a reason the crate gave on an `error: ` line.
    case pattern(reason: String)
    /// F4 step 5 without one: the compiled-size limit.
    case tooLarge
    /// F7 step 1.
    case tagSeparator

    /// W2.1: the reason is the part after `error: ` on the last line that
    /// starts with it.
    static func of(message: String) -> BrowseFindReplaceInvalid {
        let line = message.split(whereSeparator: \.isNewline).last { $0.hasPrefix("error: ") }
        guard let line else { return .tooLarge }
        return .pattern(reason: String(line.dropFirst("error: ".count)))
    }
}

/// Find and Replace… (V9): Find, Replace With, In, Notes and the three
/// options; the core's live preview of the notes that change; one run, one
/// undo step "Find and Replace". The preview is the question (V8.2): its
/// count is what the run confirms (F11), so a run that would write more
/// notes than it showed is refused and the preview shown again.
@Observable @MainActor
public final class BrowseFindReplaceSheet: Identifiable {
    public enum Notes: Hashable, Sendable { case selected, allFound }

    public let id: UInt64
    /// The selected rows the sheet opened on (V9's Selected), nil with none.
    public let selected: BrowseTarget?
    /// Every row of the results (All Found): taken once the list is whole.
    public private(set) var allFound: BrowseTarget?
    /// k and m of V9, once counted.
    public private(set) var selectedNotes: UInt64?
    public private(set) var foundNotes: UInt64?

    public var find: String { didSet { if find != oldValue { inputChanged() } } }
    public var replace: String { didSet { if replace != oldValue { inputChanged() } } }
    public var matchCase: Bool { didSet { if matchCase != oldValue { inputChanged() } } }
    public var wholeWords: Bool { didSet { if wholeWords != oldValue { inputChanged() } } }
    public var regex: Bool { didSet { if regex != oldValue { inputChanged() } } }
    /// Selected first and the default with a selection (V9).
    public var notes: Notes {
        didSet {
            guard notes != oldValue else { return }
            updateEntries()
            inputChanged()
        }
    }
    /// The In menu for the chosen notes' note types.
    public private(set) var entries: [BrowseFieldEntry] = BrowseFieldEntry.menu([], ends: true)
    /// The In entry chosen (its position).
    public var inEntry: Int = 0 {
        didSet {
            guard inEntry != oldValue else { return }
            inTouched = true
            inputChanged()
        }
    }

    public private(set) var preview: BrowseFindReplacePreviewState = .counting
    /// The last preview read, kept while a newer one counts (no flash).
    public private(set) var shownPreview: FindReplacePreview?
    public private(set) var phase: BrowseSheet.Phase = .editing
    public private(set) var failure: BrowseSheetFailure?

    @ObservationIgnored private weak var commands: BrowseCommands?
    @ObservationIgnored private let reads: any BrowseSheetReads
    @ObservationIgnored private var previews: (any BrowseFindReplacePreviewing)?
    @ObservationIgnored private var previewTask: Task<Void, Never>?
    @ObservationIgnored private var previewToken: UInt64 = 0
    /// The inputs and target the ready preview was read for.
    @ObservationIgnored private var previewed: (request: FindReplaceRequest, target: BrowseTarget)?
    @ObservationIgnored private var selectedTypes: [Int64]?
    @ObservationIgnored private var foundTypes: [Int64]?
    @ObservationIgnored private var types: [Int64: BrowseNotetypeFields] = [:]
    /// The note types' order (A2's), for the In menu's groups.
    @ObservationIgnored private let typeOrder: [Int64]
    @ObservationIgnored private let remembered: (entry: String, position: Int)?
    @ObservationIgnored private var inTouched = false
    @ObservationIgnored private var running: BrowseRunning<FindReplaceResult>?
    @ObservationIgnored private var tasks: [Task<Void, Never>] = []
    /// The sheet went: nothing more is read for it.
    @ObservationIgnored private var isClosed = false

    init(id: UInt64, selected: BrowseTarget?, commands: BrowseCommands, reads: any BrowseSheetReads,
         memory: BrowseFindReplaceDevice?, prefill: (find: String, replace: String)?, typeOrder: [Int64]) {
        self.id = id
        self.selected = selected
        self.commands = commands
        self.reads = reads
        self.typeOrder = typeOrder
        let memory = memory ?? BrowseFindReplaceDevice()
        matchCase = memory.matchCase
        wholeWords = memory.wholeWords
        regex = memory.regex
        // F2: within a launch, the last Replace's texts while they are still the newest entries.
        find = prefill.flatMap { memory.finds.first == $0.find ? $0.find : nil } ?? ""
        replace = prefill.flatMap { memory.replaces.first == $0.replace ? $0.replace : nil } ?? ""
        remembered = memory.inEntry.flatMap { entry in memory.inPosition.map { (entry, $0) } }
        notes = selected == nil ? .allFound : .selected
        restoreRemembered()
    }

    /// The histories the fields offer (F2), newest first.
    public private(set) var findHistory: [String] = []
    public private(set) var replaceHistory: [String] = []

    // MARK: Inputs

    /// The target of the chosen notes, once known.
    public var target: BrowseTarget? { notes == .selected ? selected : allFound }

    /// Selected is disabled with no selection (V9); All Found waits for
    /// the whole results.
    public var canChooseSelected: Bool { selected != nil }

    public var chosenEntry: BrowseFieldEntry? { entries.first { $0.id == inEntry } }

    /// The sheet's inputs as the core takes them (Match Case off ignores
    /// case, V9).
    public var request: FindReplaceRequest {
        FindReplaceRequest(find: find, replace: replace, regex: regex, ignoreCase: !matchCase, wholeWords: wholeWords,
                           scope: chosenEntry?.kind.scope ?? .allFields)
    }

    /// The preview shown is the one read for these inputs and this target.
    private var previewIsCurrent: Bool {
        guard let previewed, let target else { return false }
        return previewed.request == request && previewed.target == target
    }

    /// The count the button names and the run confirms (V9's c).
    public var confirmCount: UInt64? {
        guard case .ready(let preview) = preview, previewIsCurrent else { return nil }
        return preview.changed
    }

    /// Replace runs: the preview of these inputs found notes that change.
    public var canConfirm: Bool {
        guard phase == .editing, let count = confirmCount else { return false }
        return count > 0
    }

    public var isRunning: Bool { phase != .editing }

    public var canCancel: Bool {
        switch phase {
        case .editing: true
        case .running(let progress): progress?.cancellable ?? true
        case .cancelling: false
        }
    }

    private func inputChanged() {
        failure = nil
        refreshPreview()
    }

    // MARK: The In menu

    private func updateEntries() {
        // Until the chosen notes' note types are read, the menu and its choice stay as they are.
        guard let ids = notes == .selected ? selectedTypes : foundTypes else { return }
        let ordered = ids.sorted { (typeOrder.firstIndex(of: $0) ?? .max, $0) < (typeOrder.firstIndex(of: $1) ?? .max, $1) }
        let next = BrowseFieldEntry.menu(ordered.compactMap { types[$0] }, ends: true)
        let chosen = chosenEntry?.kind
        guard next != entries else { return }
        entries = next
        // Until the person chooses, the remembered entry when the menu offers it (F2); otherwise
        // the same field (by name) stays chosen, and a name the menu no longer offers falls to All Fields.
        let position: Int
        if !inTouched, let remembered, next.indices.contains(remembered.position),
           next[remembered.position].kind.memory == remembered.entry {
            position = remembered.position
        } else {
            position = chosen.flatMap { kind in next.first { $0.kind == kind }?.id } ?? 0
        }
        if position != inEntry {
            let touched = inTouched
            inEntry = position
            inTouched = touched
        }
    }

    /// F2: the remembered entry when the menu offers its text at its position,
    /// until the person chooses one.
    private func restoreRemembered() {
        guard !inTouched, let remembered, entries.indices.contains(remembered.position),
              entries[remembered.position].kind.memory == remembered.entry else { return }
        inEntry = remembered.position
        inTouched = false
    }

    // MARK: Reading

    /// The reads the sheet opens with: the two targets' counts and note
    /// types, their fields, and the first preview.
    func load(session: any BrowseSearching, results: BrowseResults, history: BrowseFindReplaceDevice?,
              waitFirst: Task<BrowseTargetFacts?, Never>?) {
        findHistory = history?.finds ?? []
        replaceHistory = history?.replaces ?? []
        let opened = results.list?.handle
        tasks.append(Task { [weak self] in
            self?.previews = try? await session.findReplacePreviews()
            self?.refreshPreview()
        })
        tasks.append(Task { [weak self, selected] in
            // Two counts reads of a session at once cancel the older one: one after the other.
            _ = await waitFirst?.value
            if let selected, let counts = try? await Self.counts(session, selected) {
                guard let self else { return }
                selectedNotes = counts.notes
                selectedTypes = counts.notetypes
                await readTypes(counts.notetypes)
            }
            // All Found once the results are whole (they may still stream).
            while !Task.isCancelled, let self, allFound == nil {
                if let all = results.allRowsTarget(), all.handle == opened {
                    allFound = all
                    // Its preview runs now, whatever its counts read gives.
                    if notes == .allFound { refreshPreview() }
                    break
                }
                if results.list?.handle != opened || results.list?.released != false
                    || (results.list?.isComplete == true && results.list?.count == 0) { break }
                try? await Task.sleep(for: .milliseconds(100))
            }
            guard !Task.isCancelled, let self, let all = allFound, let counts = try? await Self.counts(session, all) else { return }
            foundNotes = counts.notes
            foundTypes = counts.notetypes
            await readTypes(counts.notetypes)
            if notes == .allFound { refreshPreview() }
        })
    }

    /// A newer counts read of the session cancels the older one in the core
    /// (Browse's own reads after a change): read again, a few times, until
    /// the caller itself is cancelled.
    static func counts(_ session: any BrowseSearching, _ target: BrowseTarget) async throws -> BrowseSelectionCounts {
        var attempt = 0
        while true {
            do { return try await session.counts(of: target) } catch BridgeError.cancelled {
                attempt += 1
                guard attempt < 5, !Task.isCancelled else { throw BridgeError.cancelled }
                try? await Task.sleep(for: .milliseconds(50 * attempt))
            }
        }
    }

    /// The note types' fields not read yet; a failed read leaves that note
    /// type's fields out of the menu (All Fields still reaches them).
    private func readTypes(_ ids: [Int64]) async {
        for id in ids where types[id] == nil {
            if let fields = try? await reads.notetypeFields(id) { types[id] = fields }
        }
        updateEntries()
    }

    /// V9: the core's preview of the inputs on the target, each newer one
    /// superseding the older.
    func refreshPreview() {
        previewToken &+= 1
        previewTask?.cancel()
        guard phase == .editing, !isClosed else { return }
        preview = .counting
        guard let target, let previews else { return }
        let token = previewToken, request = request, previous = previewTask
        previewTask = Task { [weak self] in
            // One preview at a time, in order: the core supersedes by the order starts reach it, so a
            // start overtaken by an older one would be cancelled by it and leave nothing shown.
            await previous?.value
            guard !Task.isCancelled, let self, previewToken == token else { return }
            let state: BrowseFindReplacePreviewState
            do {
                state = .ready(try await previews.preview(request, on: target))
            } catch {
                guard let next = Self.state(error) else { return }
                state = next
            }
            guard previewToken == token, !Task.isCancelled else { return }
            if case .ready(let read) = state {
                previewed = (request, target)
                shownPreview = read
            } else {
                previewed = nil
            }
            preview = state
        }
    }

    /// A preview's or a run's failure as the sheet shows it; nil when it was
    /// superseded or cancelled.
    static func state(_ error: any Error) -> BrowseFindReplacePreviewState? {
        guard let error = error as? BridgeError else { return .failed }
        switch error {
        case .cancelled: return nil
        case .findReplaceRefused(.invalidRegex(let message)): return .invalid(.of(message: message))
        case .findReplaceRefused(.separatorInReplacement): return .invalid(.tagSeparator)
        case .findReplaceRefused(.fieldCountMismatch), .findReplaceRefused(.notetypeMissing): return .damaged
        case .browse(.staleRevision), .browse(.unknownHandle), .browse(.outOfRange), .invalidArgument: return .rowsChanged
        default: return .failed
        }
    }

    // MARK: Running

    /// Replace in <c> Notes: the run on the target, confirming the count
    /// the preview showed (F11).
    public func confirm() {
        guard canConfirm, let target, let count = confirmCount, let commands else { return }
        let request = request
        commands.rememberFindReplace(request: request, entry: chosenEntry, sheet: self)
        failure = nil
        phase = .running(nil)
        commands.runFindReplace(self, request: request, target: target, confirmed: count)
    }

    /// Cancel or Esc: closes the sheet; while the run goes, asks it to stop
    /// (until its commit).
    public func cancel() {
        switch phase {
        case .editing:
            commands?.closeFindReplace(self)
        case .running(let progress):
            guard progress?.cancellable ?? true else { return }
            phase = .cancelling
            if let running { Task { await running.cancel() } }
        case .cancelling:
            return
        }
    }

    func started(_ running: BrowseRunning<FindReplaceResult>) {
        self.running = running
        if phase == .cancelling { Task { await running.cancel() } }
    }

    func progressed(_ progress: BrowseActionProgress) {
        if case .running = phase { phase = .running(progress) }
    }

    /// The run wrote nothing: the sheet stays, says why and reads the
    /// preview again (F11: a larger count is shown again).
    func failed(_ failure: BrowseSheetFailure) {
        running = nil
        phase = .editing
        self.failure = failure
        refreshPreview()
    }

    func closed() {
        isClosed = true
        previewTask?.cancel()
        for task in tasks { task.cancel() }
    }
}

/// Find Duplicates… (V10): a field, All Notes or All <m> Found, and Find.
/// The groups replace the query with one Duplicates token; with none the
/// sheet stays and says so.
@Observable @MainActor
public final class BrowseFindDuplicatesSheet: Identifiable {
    public enum Notes: Hashable, Sendable { case allNotes, allFound }

    public let id: UInt64
    public private(set) var allFound: BrowseTarget?
    public private(set) var foundNotes: UInt64?
    /// All Notes is the default (V10).
    public var notes: Notes = .allNotes { didSet { if notes != oldValue { found = nil; failure = nil } } }
    /// Every field name, by note type (V10).
    public private(set) var entries: [BrowseFieldEntry] = []
    public var fieldEntry: Int? { didSet { if fieldEntry != oldValue { touched = true; found = nil; failure = nil } } }
    public private(set) var phase: BrowseSheet.Phase = .editing
    /// The last Find found no group: "No duplicates found."
    public private(set) var found: FindDuplicatesResult?
    public private(set) var failure: BrowseSheetFailure?

    @ObservationIgnored private weak var commands: BrowseCommands?
    @ObservationIgnored private let reads: any BrowseSheetReads
    @ObservationIgnored private let notetypes: [NotetypeName]
    /// The row whose note type's Sort Field the menu starts on.
    @ObservationIgnored private let startRow: BrowseTarget?
    @ObservationIgnored private var touched = false
    @ObservationIgnored private var running: BrowseRunning<FindDuplicatesResult>?
    @ObservationIgnored private var tasks: [Task<Void, Never>] = []

    init(id: UInt64, startRow: BrowseTarget?, commands: BrowseCommands, reads: any BrowseSheetReads, notetypes: [NotetypeName]) {
        self.id = id
        self.startRow = startRow
        self.commands = commands
        self.reads = reads
        self.notetypes = notetypes
    }

    public var field: String? {
        guard let fieldEntry, let entry = entries.first(where: { $0.id == fieldEntry }), case .field(let name) = entry.kind
        else { return nil }
        return name
    }

    /// The last Find found no duplicates.
    public var noneFound: Bool { found?.groups.isEmpty == true }

    public var target: BrowseTarget? { notes == .allFound ? allFound : nil }

    /// Find runs once a field is chosen (V10), and All Found once its rows
    /// are known.
    public var canFind: Bool { phase == .editing && field != nil && (notes == .allNotes || allFound != nil) }

    public var isRunning: Bool { phase != .editing }

    public var canCancel: Bool {
        switch phase {
        case .editing: true
        case .running(let progress): progress?.cancellable ?? true
        case .cancelling: false
        }
    }

    /// The reads it opens with: every note type's fields, the start row's
    /// note type and All Found's count.
    func load(session: any BrowseSearching, results: BrowseResults, waitFirst: Task<BrowseTargetFacts?, Never>?) {
        let opened = results.list?.handle
        tasks.append(Task { [weak self, reads, notetypes, startRow] in
            var types: [BrowseNotetypeFields] = []
            for name in notetypes {
                if let fields = try? await reads.notetypeFields(name.id) { types.append(fields) }
            }
            guard let self else { return }
            entries = BrowseFieldEntry.menu(types, ends: false)
            // The Sort Field of the start row's note type (V10).
            _ = await waitFirst?.value
            if let startRow, !touched, let type = try? await BrowseFindReplaceSheet.counts(session, startRow).notetypes.first,
               let fields = types.first(where: { $0.id == type }), let name = fields.sortFieldName,
               let entry = entries.first(where: { $0.kind == .field(name) && ($0.group == nil || $0.group == fields.name) }),
               !touched {
                fieldEntry = entry.id
                touched = false
            }
            while !Task.isCancelled, allFound == nil {
                if let all = results.allRowsTarget(), all.handle == opened {
                    allFound = all
                    break
                }
                if results.list?.handle != opened || results.list?.released != false
                    || (results.list?.isComplete == true && results.list?.count == 0) { break }
                try? await Task.sleep(for: .milliseconds(100))
            }
            if let all = allFound, let counts = try? await BrowseFindReplaceSheet.counts(session, all) {
                foundNotes = counts.notes
            }
        })
    }

    /// Find: the core's groups for the field among the chosen notes.
    public func confirm() {
        guard canFind, let field, let commands else { return }
        failure = nil
        found = nil
        phase = .running(nil)
        commands.runFindDuplicates(self, field: field, target: target)
    }

    public func cancel() {
        switch phase {
        case .editing:
            commands?.closeFindDuplicates(self)
        case .running(let progress):
            guard progress?.cancellable ?? true else { return }
            phase = .cancelling
            if let running { Task { await running.cancel() } }
        case .cancelling:
            return
        }
    }

    func started(_ running: BrowseRunning<FindDuplicatesResult>) {
        self.running = running
        if phase == .cancelling { Task { await running.cancel() } }
    }

    func progressed(_ progress: BrowseActionProgress) {
        if case .running = phase { phase = .running(progress) }
    }

    /// Find ended with no group: the sheet stays and says so (V10).
    func finished(_ result: FindDuplicatesResult) {
        running = nil
        phase = .editing
        found = result
    }

    func failed(_ failure: BrowseSheetFailure?) {
        running = nil
        phase = .editing
        self.failure = failure
    }

    func closed() {
        for task in tasks { task.cancel() }
        if let running { Task { await running.cancel() } }
    }
}

extension BrowseFindReplaceSheet {
    /// A refused or failed run as the sheet's line says it (K4, F11).
    static func failure(_ error: any Error) -> BrowseSheetFailure? {
        if case BridgeError.findReplaceRefused(.countChanged(let notes))? = error as? BridgeError { return .countChanged(notes) }
        switch state(error) {
        case nil: return nil
        case .rowsChanged: return .rowsChanged
        case .invalid(let invalid): return .findReplace(invalid)
        case .damaged: return .damaged
        default: return .failed
        }
    }
}
