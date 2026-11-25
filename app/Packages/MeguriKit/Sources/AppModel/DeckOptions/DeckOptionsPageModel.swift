import CoreBridge
import Foundation
import Observation

/// A row of the first version's Deck Options page (`sched/deck-options` O17.1).
public nonisolated enum DeckOptionsPageRow: String, Sendable, Hashable, CaseIterable {
    case newPerDay, reviewsPerDay, desiredRetention
}

/// Display Order's preset fields in O3's presentation order. Values and choice
/// order come from the core snapshot, not from a second scheduler definition.
public nonisolated enum DeckOptionsDisplayOrder: String, Sendable, Hashable, CaseIterable {
    case newGatherPriority, newSortOrder, newMix, interdayLearningMix, reviewOrder

    public var field: OptionField {
        switch self {
        case .newGatherPriority: .newCardGatherPriority
        case .newSortOrder: .newCardSortOrder
        case .newMix: .newMix
        case .interdayLearningMix: .interdayLearningMix
        case .reviewOrder: .reviewOrder
        }
    }
}

/// One preset as the page shows it.
public nonisolated struct DeckOptionsPagePreset: Sendable, Hashable, Identifiable {
    public let id: Int64
    public let name: String
    /// Normal decks whose stored preset this is (O2 item 1).
    public let storedUseCount: UInt64
    public var newPerDay: Double
    public var reviewsPerDay: Double
    /// A fraction (0.9), shown as a percentage.
    public var desiredRetention: Double
    public var displayOrder: [DeckOptionsDisplayOrder: Int32]

    public init(id: Int64, name: String, storedUseCount: UInt64, newPerDay: Double, reviewsPerDay: Double, desiredRetention: Double,
                displayOrder: [DeckOptionsDisplayOrder: Int32] = [:]) {
        self.id = id; self.name = name; self.storedUseCount = storedUseCount
        self.newPerDay = newPerDay; self.reviewsPerDay = reviewsPerDay; self.desiredRetention = desiredRetention
        self.displayOrder = displayOrder
    }

    public subscript(row: DeckOptionsPageRow) -> Double {
        get {
            switch row {
            case .newPerDay: newPerDay
            case .reviewsPerDay: reviewsPerDay
            case .desiredRetention: desiredRetention
            }
        }
        set {
            switch row {
            case .newPerDay: newPerDay = newValue
            case .reviewsPerDay: reviewsPerDay = newValue
            case .desiredRetention: desiredRetention = newValue
            }
        }
    }
}

/// What the core read gives the page (O2), as plain values.
public nonisolated struct DeckOptionsPageSnapshot: Sendable, Hashable {
    public let deckID: Int64
    /// The deck's name in human form (`A::B`); views join its components with " › ".
    public let deckName: String
    /// The deck's stored `config_id` (O1), which may name no preset.
    public let assignedPreset: Int64
    public let openingPreset: Int64
    /// O2's order.
    public let presets: [DeckOptionsPagePreset]
    /// A This-deck limit, or an active Today-only limit, is set (O17.1).
    public let ownLimits: Bool
    /// The deck's own desired retention is set (O17.1).
    public let ownRetention: Bool
    /// Each row's editor range (O3), from the core's metadata.
    public let ranges: [DeckOptionsPageRow: ClosedRange<Double>]
    public let displayOrderChoices: [DeckOptionsDisplayOrder: [Int32]]

    public init(deckID: Int64, deckName: String, assignedPreset: Int64, openingPreset: Int64,
                presets: [DeckOptionsPagePreset], ownLimits: Bool, ownRetention: Bool,
                ranges: [DeckOptionsPageRow: ClosedRange<Double>],
                displayOrderChoices: [DeckOptionsDisplayOrder: [Int32]] = [:]) {
        self.deckID = deckID; self.deckName = deckName; self.assignedPreset = assignedPreset
        self.openingPreset = openingPreset; self.presets = presets
        self.ownLimits = ownLimits; self.ownRetention = ownRetention; self.ranges = ranges
        self.displayOrderChoices = displayOrderChoices
    }
}

/// An O4 issue under one of the page's rows.
public nonisolated struct DeckOptionsPageIssue: Sendable, Hashable {
    public let row: DeckOptionsPageRow
    /// The preset the issue belongs to; nil for a row's shared warning (O4.3).
    public let preset: Int64?
    public let code: OptionIssueCode
    public let severity: OptionSeverity
    public let params: [String: String]

    public init(row: DeckOptionsPageRow, preset: Int64?, code: OptionIssueCode, severity: OptionSeverity, params: [String: String] = [:]) {
        self.row = row; self.preset = preset; self.code = code; self.severity = severity; self.params = params
    }

    /// Maps a core issue of one of the page's three fields; others are not shown.
    public init?(_ issue: OptionIssue) {
        let row: DeckOptionsPageRow
        switch issue.field {
        case .newPerDay: row = .newPerDay
        case .reviewsPerDay: row = .reviewsPerDay
        case .desiredRetention: row = .desiredRetention
        default: return nil
        }
        self.init(row: row, preset: issue.preset, code: issue.code, severity: issue.severity, params: issue.params)
    }
}

/// The core draft behind one open page. Production uses `CoreDeckOptionsPageSession`.
@MainActor
protocol DeckOptionsPageSession: AnyObject {
    var snapshot: DeckOptionsPageSnapshot { get }
    func select(_ preset: Int64) async throws
    func set(_ row: DeckOptionsPageRow, preset: Int64, value: Double) async throws
    func setDisplayOrder(_ row: DeckOptionsDisplayOrder, preset: Int64, value: Int32) async throws
    /// Returns the canonical core draft after Add (including its name, order and selection).
    func addPreset(name: String) async throws -> DeckOptionsPageSnapshot
    func issues() async throws -> [DeckOptionsPageIssue]
    /// O17.3's save; returns the generation it committed in.
    func save() async throws -> UInt64
    /// Cancels a running save (background, close); the draft stays usable.
    func cancel() async
}

/// The first version's Deck Options page (`sched/deck-options` O17,
/// `native-interface-contract` N20.9): the draft of the preset and three
/// values, validation from the core, the one save and the leave rules.
/// Held by `ReviewCollection`, so a layout switch keeps the draft.
@Observable @MainActor
public final class DeckOptionsPageModel {
    public nonisolated enum Phase: Equatable, Sendable {
        case closed, loading, editing, saving
        case unavailable(ReadFailure)
    }
    public nonisolated enum ReadFailure: Equatable, Sendable { case notFound, filtered, failed }
    /// Why the page closed; the shell returns to the deck page for each.
    public nonisolated enum Outcome: Equatable, Sendable { case saved, left, ended }
    /// A refused save the page explains in an alert.
    public nonisolated enum SaveFailure: Equatable, Sendable { case busy, failed }

    public private(set) var phase: Phase = .closed
    public private(set) var outcome: Outcome?
    public private(set) var deckID: Int64?
    public private(set) var deckName = ""
    /// The draft's presets in O2's order, with their draft values.
    public private(set) var presets: [DeckOptionsPagePreset] = []
    public private(set) var selectedPreset: Int64 = 0
    public private(set) var issues: [DeckOptionsPageIssue] = []
    public private(set) var ownLimits = false
    public private(set) var ownRetention = false
    /// N11.9's row: the deck or a preset changed elsewhere while the draft differs.
    public private(set) var changedElsewhere = false
    /// "Discard changes?" is showing (N20.9).
    public private(set) var confirmingDiscard = false
    public var saveFailure: SaveFailure?
    /// Kept with the draft across layout changes; reset whenever a new page opens.
    public var advancedExpanded = false
    /// The shell's route owns modal admission, including interactive dismissal.
    public var namingPreset: Bool {
        owner?.navigation.sheet == .newDeckOptionsPreset && owner?.navigation.sheetHost == .main
    }
    public var presetName = ""
    public private(set) var addingPreset = false
    public private(set) var selectingPreset = false
    public private(set) var presetCreationFailed = false
    /// Add succeeded; Save can safely retry the core's validation before writing.
    public private(set) var presetValidationFailed = false
    /// The text of each row's field; committed by `commit(_:)` (O15.3).
    public var fieldText: [DeckOptionsPageRow: String] = [:]
    @ObservationIgnored public var locale = Locale.autoupdatingCurrent

    private weak var owner: ReviewCollection?
    private var opening: DeckOptionsPageSnapshot?
    private var session: (any DeckOptionsPageSession)?
    private var generation: UInt64 = 0
    private var revision: UInt64 = 0
    private var editTask: Task<Void, Never>?
    // Queued numeric edits remain pending through the core's issue read.
    private var pendingNumericValidations = 0
    private var selectionTask: Task<Void, Never>?
    private var task: Task<Void, Never>?
    // Tests replace the core session; production reads through CoreBridge.
    @ObservationIgnored var openSession: @MainActor (Int64) async throws -> any DeckOptionsPageSession

    init(owner: ReviewCollection) {
        self.owner = owner
        openSession = { @MainActor in try await CoreDeckOptionsPageSession.open(deck: $0) }
    }

    // MARK: Opening

    /// Whether the deck's Deck Options… entries are enabled (N20.2, N20.4): a
    /// normal deck, outside review, with no other collection work holding it.
    public func canOpen(deckID: Int64) -> Bool {
        guard let owner, phase != .saving, !addingPreset, !namingPreset, !selectingPreset, owner.deckOptionsAllowed else { return false }
        // The page already open for this deck stays as it is; a changed draft
        // of another deck is never replaced without its own leave (N20.9).
        if phase != .closed, self.deckID == deckID || hasChanges { return false }
        return owner.deck(deckID).map { !$0.filtered } ?? (owner.fullDeckName(deckID) != nil)
    }

    /// Reads the deck's snapshot and starts a draft. The shell shows the page at once.
    public func open(deckID: Int64) {
        guard canOpen(deckID: deckID), let owner else { return }
        revision &+= 1
        let current = revision
        session = nil; opening = nil; presets = []; issues = []; fieldText = [:]
        changedElsewhere = false; confirmingDiscard = false; saveFailure = nil; outcome = nil
        advancedExpanded = false; presetName = ""; addingPreset = false; selectingPreset = false; presetCreationFailed = false
        presetValidationFailed = false
        leftWhileSaving = false; saveCancelled = false; changedWhileLoading = false
        self.deckID = deckID; deckName = owner.fullDeckName(deckID) ?? ""
        generation = owner.generation; phase = .loading
        task = Task { await read(deckID: deckID, revision: current) }
    }

    /// Reads a new snapshot for the open deck: Try Again, and N11.9's Reload.
    public func reload() {
        guard let deckID, phase != .saving, phase != .closed, !addingPreset, !namingPreset, !selectingPreset, let owner,
              owner.generation == generation, owner.deckOptionsAllowed else { return }
        revision &+= 1
        let current = revision
        let previous = session
        session = nil; changedElsewhere = false; saveFailure = nil; phase = .loading; changedWhileLoading = false
        task = Task {
            await previous?.cancel()
            await read(deckID: deckID, revision: current)
        }
    }

    private func read(deckID: Int64, revision current: UInt64) async {
        do {
            let opened = try await openSession(deckID)
            guard current == revision, let owner, owner.generation == generation else { await opened.cancel(); return }
            let issues = try await opened.issues()
            guard current == revision else { await opened.cancel(); return }
            adopt(opened, issues: issues)
        } catch {
            guard current == revision else { return }
            let failure: ReadFailure
            switch error as? BridgeError {
            case .notFound?: failure = .notFound
            case .deckRefused(.filteredDeck)?: failure = .filtered
            default: failure = .failed
            }
            phase = .unavailable(failure)
        }
    }

    private func adopt(_ opened: any DeckOptionsPageSession, issues: [DeckOptionsPageIssue]) {
        let snapshot = opened.snapshot
        session = opened; opening = snapshot
        deckName = snapshot.deckName; presets = snapshot.presets; selectedPreset = snapshot.openingPreset
        ownLimits = snapshot.ownLimits; ownRetention = snapshot.ownRetention
        self.issues = issues
        presetValidationFailed = false
        pendingNumericValidations = 0
        for row in DeckOptionsPageRow.allCases { fieldText[row] = formatted(row) }
        phase = .editing
        if changedWhileLoading { changedWhileLoading = false; reload() }
    }

    // MARK: The draft

    public var selected: DeckOptionsPagePreset? { presets.first { $0.id == selectedPreset } }

    public func value(_ row: DeckOptionsPageRow) -> Double? { selected?[row] }

    /// The picker's count of a preset (O6.1): the stored count, less this deck
    /// for its stored preset, plus this deck for the selected one.
    public func useCount(_ preset: DeckOptionsPagePreset) -> UInt64 {
        // O6.3: a staged preset is shown as unused until Save assigns it.
        guard preset.id >= 0 else { return 0 }
        var count = preset.storedUseCount
        if preset.id == opening?.assignedPreset, count > 0 { count -= 1 }
        if preset.id == selectedPreset { count += 1 }
        return count
    }

    /// N20.9's Preset footer: the selected preset's picker count.
    public var selectedUseCount: UInt64 { selected.map(useCount) ?? 0 }

    public func range(_ row: DeckOptionsPageRow) -> ClosedRange<Double>? { opening?.ranges[row] }

    /// O6.6, for what the page can change: another preset, or a value that
    /// differs from the snapshot's under O1's `f32` equality.
    public var hasChanges: Bool {
        guard let opening else { return false }
        if selectedPreset != opening.openingPreset { return true }
        // Text typed but not committed yet counts (O15.3).
        if DeckOptionsPageRow.allCases.contains(where: { row in fieldText[row].map { $0 != formatted(row) } ?? false }) {
            return true
        }
        return presets.contains { preset in
            guard let base = opening.presets.first(where: { $0.id == preset.id }) else { return true }
            return preset.displayOrder != base.displayOrder
                || DeckOptionsPageRow.allCases.contains { Float(preset[$0]) != Float(base[$0]) }
        }
    }

    public var hasErrors: Bool { issues.contains { $0.severity == .error } }
    /// Another preset can be chosen once numeric edits are validated and the
    /// selected one has no error, so an error never hides behind another preset's rows.
    public var canSwitchPreset: Bool {
        phase == .editing && !addingPreset && !namingPreset && !presetValidationFailed && pendingNumericValidations == 0
            && !issues.contains { $0.severity == .error && ($0.preset == nil || $0.preset == selectedPreset) }
    }

    /// The issues shown under a row: the selected preset's, and the row's shared warnings.
    public func issues(for row: DeckOptionsPageRow) -> [DeckOptionsPageIssue] {
        issues.filter { $0.row == row && ($0.preset == nil || $0.preset == selectedPreset) }
    }

    /// Rows of the selected preset whose field holds text not committed yet.
    private var pendingRows: Set<DeckOptionsPageRow> {
        Set(DeckOptionsPageRow.allCases.filter { row in fieldText[row].map { $0 != formatted(row) } ?? false })
    }

    /// An error blocks Save unless its row's field holds a correction that
    /// Save commits and validates first (the save rechecks after validation).
    public var canSave: Bool {
        let pending = pendingRows
        let blocking = issues.contains { $0.severity == .error
            && !(pending.contains($0.row) && ($0.preset == nil || $0.preset == selectedPreset)) }
        guard phase == .editing, !addingPreset, !selectingPreset, !namingPreset, hasChanges, !blocking, let owner,
              owner.generation == generation else { return false }
        return owner.deckOptionsAllowed
    }

    /// Selecting a preset makes it the target preset (O17.2); edits to the
    /// first one stay in the draft.
    public func selectPreset(_ id: Int64) {
        guard phase == .editing, !addingPreset, !namingPreset else { return }
        selectionRequest &+= 1
        let request = selectionRequest
        commitAll()
        if pendingNumericValidations > 0 {
            // A value typed for the current preset is checked first: its error
            // must show on its own rows, not hide behind another preset's.
            let current = revision, typed = fieldText, pending = editTask
            selectingPreset = true
            selectionTask = Task { [weak self] in
                await pending?.value
                // Text typed while the value was checked cancels the switch,
                // so the new text is never replaced by the other preset's.
                guard let self, self.revision == current, self.selectionRequest == request else { return }
                self.selectingPreset = false
                guard self.fieldText == typed else { return }
                self.switchPreset(to: id)
            }
            return
        }
        selectingPreset = false
        switchPreset(to: id)
    }

    private var selectionRequest: UInt64 = 0

    private func switchPreset(to id: Int64) {
        guard id != selectedPreset, presets.contains(where: { $0.id == id }),
              canSwitchPreset else { return }
        selectedPreset = id
        for row in DeckOptionsPageRow.allCases { fieldText[row] = formatted(row) }
        enqueue { try await $0.select(id) }
    }

    /// Sets a row's value for the selected preset. A value outside the range
    /// stays in the draft, where O4's error disables Save (O17.1).
    public func setValue(_ row: DeckOptionsPageRow, _ value: Double) {
        guard phase == .editing, !addingPreset, !namingPreset, value.isFinite,
              let index = presets.firstIndex(where: { $0.id == selectedPreset }) else { return }
        let preset = selectedPreset
        if presets[index][row] != value {
            presets[index][row] = value
            enqueue(numeric: true) { try await $0.set(row, preset: preset, value: value) }
        }
        fieldText[row] = formatted(row)
    }

    public func choices(_ row: DeckOptionsDisplayOrder) -> [Int32] { opening?.displayOrderChoices[row] ?? [] }

    public func setDisplayOrder(_ row: DeckOptionsDisplayOrder, _ value: Int32) {
        guard phase == .editing, !addingPreset, !namingPreset, choices(row).contains(value),
              let index = presets.firstIndex(where: { $0.id == selectedPreset }),
              presets[index].displayOrder[row] != value else { return }
        let preset = selectedPreset
        presets[index].displayOrder[row] = value
        enqueue { try await $0.setDisplayOrder(row, preset: preset, value: value) }
    }

    public func beginNewPreset() {
        guard phase == .editing, !addingPreset, !namingPreset, !selectingPreset else { return }
        commitAll()
        selectionRequest &+= 1
        if pendingNumericValidations > 0 {
            let current = revision, request = selectionRequest, typed = fieldText, pending = editTask
            selectingPreset = true
            selectionTask = Task { [weak self] in
                await pending?.value
                guard let self, self.revision == current, self.selectionRequest == request else { return }
                self.selectingPreset = false
                guard self.fieldText == typed else { return }
                self.presentNewPreset()
            }
            return
        }
        presentNewPreset()
    }

    private func presentNewPreset() {
        guard canSwitchPreset, !selectingPreset, !changedElsewhere, let owner, owner.generation == generation,
              owner.deckOptionsAllowed else { return }
        presetName = ""; presetCreationFailed = false
        owner.navigation.present(.newDeckOptionsPreset)
    }

    public var canCreatePreset: Bool {
        phase == .editing && namingPreset && !addingPreset && !changedElsewhere
            && !presetName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            && owner?.generation == generation
            && owner?.deckNamesAllowed(host: .main, route: .newDeckOptionsPreset) == true
    }

    public func cancelNewPreset() {
        guard !addingPreset else { return }
        finishNamingPreset()
    }

    private func finishNamingPreset() {
        presetName = ""; presetCreationFailed = false
        if namingPreset { owner?.navigation.dismissSheet() }
    }

    public func createPreset() {
        guard canCreatePreset, let session else { return }
        let name = presetName.trimmingCharacters(in: .whitespacesAndNewlines)
        let current = revision, previous = editTask
        addingPreset = true; presetCreationFailed = false
        selectionRequest &+= 1
        // Reserve editing synchronously before awaiting the pending edit chain.
        // Add returns all canonical values; nothing can overwrite newer input.
        editTask = Task {
            await previous?.value
            guard current == revision else { return }
            if issues.contains(where: { $0.severity == .error && ($0.preset == nil || $0.preset == selectedPreset) }) {
                addingPreset = false; finishNamingPreset()
                return
            }
            let updated: DeckOptionsPageSnapshot
            do { updated = try await session.addPreset(name: name) }
            catch {
                guard current == revision else { return }
                addingPreset = false; presetCreationFailed = true
                // A rejected Add leaves the core draft, including earlier edits, intact.
                handlePresetFailure(error)
                return
            }
            guard current == revision else { return }
            // Add committed its staged draft. Reflect it even if the separate
            // validation read fails, and never offer to retry this Add.
            presets = updated.presets; selectedPreset = updated.openingPreset
            for row in DeckOptionsPageRow.allCases { fieldText[row] = formatted(row) }
            do {
                let checked = try await session.issues()
                guard current == revision else { return }
                issues = checked; presetValidationFailed = false
            } catch {
                guard current == revision else { return }
                presetValidationFailed = true
                handlePresetFailure(error)
            }
            guard current == revision else { return }
            addingPreset = false; finishNamingPreset()
        }
    }

    private func handlePresetFailure(_ error: any Error) {
        switch error as? BridgeError {
        case .staleGeneration?: end(.ended)
        case .stale?, .notFound?: changedElsewhere = true
        case .deckOptionsInvalid(let raw)?:
            issues = raw.compactMap(DeckOptionsPageIssue.init); presetValidationFailed = false
        default: break
        }
    }

    /// The Desired Retention stepper: one whole percent, within the range.
    public func stepRetention(by percent: Int) {
        // A typed value is where the step starts.
        commit(.desiredRetention)
        guard let current = value(.desiredRetention) else { return }
        var next = ((current * 100).rounded() + Double(percent)) / 100
        if let range = range(.desiredRetention) { next = min(max(next, range.lowerBound), range.upperBound) }
        setValue(.desiredRetention, next)
    }

    /// Commits a row's typed text. Text that is not a number changes nothing
    /// and the field shows its value again (O17.1).
    @discardableResult public func commit(_ row: DeckOptionsPageRow) -> Bool {
        guard phase == .editing, !addingPreset else { return false }
        // Text the field shows unchanged is not parsed back: a stored value
        // with more decimals than the field shows stays as it is.
        let shown = formatted(row)
        if let text = fieldText[row], text != shown, let parsed = parse(text, row: row), parsed != value(row) {
            setValue(row, parsed)
            return true
        }
        fieldText[row] = shown
        return false
    }

    /// Commits every row.
    private func commitAll() {
        for row in DeckOptionsPageRow.allCases { commit(row) }
    }

    /// The text a row's field shows for the current value.
    public func formatted(_ row: DeckOptionsPageRow) -> String {
        guard let value = value(row) else { return "" }
        switch row {
        // Values come from `f32`: two decimals show 0.9 as "90%", 0.875 as "87.5%".
        case .newPerDay, .reviewsPerDay: return value.formatted(.number.precision(.fractionLength(0...2)).locale(locale))
        case .desiredRetention: return value.formatted(.percent.precision(.fractionLength(0...2)).locale(locale))
        }
    }

    private func parse(_ text: String, row: DeckOptionsPageRow) -> Double? {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return nil }
        switch row {
        case .newPerDay, .reviewsPerDay:
            return (try? Double(trimmed, format: .number.locale(locale))).flatMap { $0.isFinite ? $0 : nil }
        case .desiredRetention:
            if let percent = try? Double(trimmed, format: .percent.locale(locale)), percent.isFinite { return percent }
            let digits = trimmed.trimmingCharacters(in: CharacterSet(charactersIn: "%\u{066A}\u{FE6A}\u{FF05}").union(.whitespaces))
            return (try? Double(digits, format: .number.locale(locale))).flatMap { $0.isFinite ? $0 / 100 : nil }
        }
    }

    private func enqueue(numeric: Bool = false, _ edit: @escaping @MainActor (any DeckOptionsPageSession) async throws -> Void) {
        guard let session else { return }
        let current = revision, previous = editTask
        if numeric { pendingNumericValidations += 1 }
        editTask = Task {
            defer { if numeric, current == revision { pendingNumericValidations -= 1 } }
            await previous?.value
            guard current == revision else { return }
            do {
                try await edit(session)
                let issues = try await session.issues()
                if current == revision { self.issues = issues; presetValidationFailed = false }
            } catch {
                // The core draft no longer matches this page; read it again.
                if current == revision { changedElsewhere = true }
            }
        }
    }

    // MARK: Saving

    /// O17.3's save, after committing the field being edited (O15.3). On
    /// success the page closes with `.saved`.
    public func save() {
        // Errors are checked after the commits below are validated (in the
        // task): a correction just committed must not meet its old error.
        commitAll()
        guard phase == .editing, !addingPreset, !selectingPreset, !namingPreset, hasChanges, let session, let owner,
              owner.generation == generation, owner.deckOptionsAllowed else { return }
        phase = .saving; saveFailure = nil; saveCancelled = false; leftWhileSaving = false
        let current = revision, expected = generation
        task = Task {
            // The edits committed above reach the core draft first (O15.3).
            await editTask?.value
            guard current == revision else { return }
            guard owner.generation == expected else { end(.ended); return }
            // Moving to the background before the save started: it never starts.
            // The admitted save goes on after its page left the stack; when it
            // does not save, the draft no page shows ends.
            let refuse = { [self] in phase = .editing; if leftWhileSaving { end(.left) } }
            // Moving to the background before the save started: it never starts.
            if saveCancelled { saveCancelled = false; refuse(); return }
            if hasErrors { refuse(); return }
            if changedElsewhere {
                // N20.9 has no reload row: the save is refused, the page says
                // nothing was changed, and the options are read again.
                refuse(); guard phase == .editing else { return }
                reload(); saveFailure = .failed; return
            }
            do {
                let committed = try await session.save()
                guard current == revision else { return }
                guard committed == expected else { end(.ended); return }
                end(.saved)
            } catch {
                guard current == revision else { return }
                phase = .editing
                if leftWhileSaving { end(.left); return }
                switch error as? BridgeError {
                case .deckOptionsInvalid(let raw)?:
                    issues = raw.compactMap(DeckOptionsPageIssue.init); presetValidationFailed = false
                case .stale?, .notFound?: reload(); saveFailure = .failed
                case .staleGeneration?: end(.ended)
                case .cancelled?: break
                case .busy?, .sessionActive?: saveFailure = .busy
                default: saveFailure = .failed
                }
            }
        }
    }

    // MARK: Leaving

    /// Back, Cancel, Esc or another sidebar row (N20.9). Returns true when the
    /// page may close now; otherwise it asks "Discard changes?" first.
    @discardableResult public func requestLeave() -> Bool {
        if phase == .saving || addingPreset || selectingPreset || namingPreset { return false }
        commitAll()
        if phase == .editing, hasChanges {
            confirmingDiscard = true
            return false
        }
        end(.left)
        return true
    }

    public func discardChanges() {
        guard phase != .saving, !addingPreset else { return }
        confirmingDiscard = false
        end(.left)
    }

    public func keepEditing() { confirmingDiscard = false }

    /// The shell acknowledges an outcome once it has returned to the deck page.
    public func acknowledgeOutcome() { outcome = nil }

    private func end(_ outcome: Outcome) {
        revision &+= 1
        let previous = session, closing = deckID
        session = nil; opening = nil; presets = []; issues = []; fieldText = [:]
        changedElsewhere = false; confirmingDiscard = false
        presetValidationFailed = false
        finishNamingPreset(); addingPreset = false; selectingPreset = false
        deckID = nil; phase = .closed; self.outcome = outcome
        if let previous { Task { await previous.cancel() } }
        // The model, not the view, returns to the deck page (a page that is
        // not on screen, after a layout switch, still closes), for every
        // outcome, so a generation that ends never leaves a closed page behind.
        if let closing { owner?.navigation.popDeckOptions(DeckID(closing)) }
    }

    /// The page left its stack (`AppNavigation.onDeckOptionsLeft`): the draft
    /// ends in whatever phase, without asking; a running save completes.
    func pageLeft() {
        guard phase != .closed else { return }
        // An admitted save finishes; if it fails, the draft that no page
        // shows ends then (save()).
        if phase == .saving { leftWhileSaving = true; return }
        end(.left)
    }
    private var leftWhileSaving = false
    /// Background asked to cancel the save in progress (cancelAndDrain).
    private var saveCancelled = false
    /// A relevant change arrived while the snapshot was being read.
    private var changedWhileLoading = false

    // MARK: Collection events

    /// A committed change set (`ReviewCollection.receive`). A change to the
    /// decks, presets or config reloads silently while the draft is unchanged,
    /// and otherwise shows N11.9's row.
    func changesCommitted(_ changes: ChangeSet, generation eventGeneration: UInt64) {
        guard eventGeneration == generation, let deckID,
              changes.wholeCollection || changes.decks.contains(where: { $0.id == deckID }) || !changes.deckConfigs.isEmpty
        else { return }
        // The read in flight may predate this change: read again once it lands.
        if phase == .loading { changedWhileLoading = true; return }
        guard phase == .editing else { return }
        if hasChanges || namingPreset || addingPreset || selectingPreset || !(owner?.deckOptionsAllowed ?? false) { changedElsewhere = true } else { reload() }
    }

    /// The collection's generation ended or it is no longer ready.
    func invalidate() {
        guard phase != .closed else { return }
        end(.ended)
    }

    func cancelAndDrain() async {
        if phase == .saving { saveCancelled = true }
        let current = session
        await current?.cancel()
        await selectionTask?.value
        await editTask?.value
        await task?.value
    }

    public func waitForCompletion() async {
        await selectionTask?.value
        await editTask?.value
        await task?.value
    }
}

/// The production session: the core draft with the five deck values kept (O17.3).
@MainActor
final class CoreDeckOptionsPageSession: DeckOptionsPageSession {
    private let editor: CoreDeckOptionsEditor
    private(set) var snapshot: DeckOptionsPageSnapshot
    private var saveJob: CoreDeckOptionsSaveJob?

    private init(editor: CoreDeckOptionsEditor, snapshot: DeckOptionsPageSnapshot) {
        self.editor = editor; self.snapshot = snapshot
    }

    static func open(deck: Int64) async throws -> CoreDeckOptionsPageSession {
        let editor = try await CoreDeckOptionsService().startRead(deck: deck).value()
        let raw = try await editor.edit(.keep(fields: DeckOptionsEdit.deckValues))
        return CoreDeckOptionsPageSession(editor: editor, snapshot: Self.page(raw))
    }

    func select(_ preset: Int64) async throws { _ = try await editor.edit(.select(preset: preset)) }

    func set(_ row: DeckOptionsPageRow, preset: Int64, value: Double) async throws {
        _ = try await editor.edit(.set(preset: preset, field: Self.field(row), value: .number(value)))
    }

    func setDisplayOrder(_ row: DeckOptionsDisplayOrder, preset: Int64, value: Int32) async throws {
        _ = try await editor.edit(.set(preset: preset, field: row.field, value: .number(Double(value))))
    }

    func addPreset(name: String) async throws -> DeckOptionsPageSnapshot {
        let raw = try await editor.edit(.add(name: name))
        snapshot = Self.page(raw)
        return snapshot
    }

    func issues() async throws -> [DeckOptionsPageIssue] {
        try await editor.validate().compactMap(DeckOptionsPageIssue.init)
    }

    func save() async throws -> UInt64 {
        saving = true; cancelRequested = false
        defer { saving = false; saveJob = nil }
        let job = try await editor.startSave()
        saveJob = job
        // A cancel that arrived while the handle was on its way.
        if cancelRequested { try? await job.job.cancel() }
        return try await job.value().summary.generation
    }

    /// Cancels a running save, also one whose job handle has not arrived
    /// yet; the draft stays usable and a save that committed reports so.
    func cancel() async {
        if saving { cancelRequested = true }
        try? await saveJob?.job.cancel()
    }
    private var saving = false, cancelRequested = false

    static func field(_ row: DeckOptionsPageRow) -> OptionField {
        switch row {
        case .newPerDay: .newPerDay
        case .reviewsPerDay: .reviewsPerDay
        case .desiredRetention: .desiredRetention
        }
    }

    /// O2's snapshot as the page's values. The core's counts already apply
    /// O6.1 for its selection; the page keeps the stored count and applies it
    /// itself, so a selection shows at once.
    static func page(_ raw: DeckOptionsSnapshot) -> DeckOptionsPageSnapshot {
        func number(_ value: OptionValue?) -> Double? { if case .number(let v)? = value { v } else { nil } }
        let presets = raw.presets.map { preset in
            var stored = preset.useCount
            if preset.key >= 0, preset.key == raw.selectedPreset, stored > 0 { stored -= 1 }
            if preset.key == raw.assignedPreset { stored += 1 }
            return DeckOptionsPagePreset(id: preset.key, name: preset.name, storedUseCount: stored,
                newPerDay: number(preset.values[.newPerDay]) ?? 0,
                reviewsPerDay: number(preset.values[.reviewsPerDay]) ?? 0,
                desiredRetention: number(preset.values[.desiredRetention]) ?? 0,
                displayOrder: Dictionary(uniqueKeysWithValues: DeckOptionsDisplayOrder.allCases.compactMap { row in
                    number(preset.values[row.field]).flatMap(Int32.init(exactly:)).map { (row, $0) }
                }))
        }
        var ranges: [DeckOptionsPageRow: ClosedRange<Double>] = [:]
        for row in DeckOptionsPageRow.allCases {
            if let meta = raw.metadata.first(where: { $0.field == field(row) }),
               let minimum = meta.minimum, let maximum = meta.maximum, minimum <= maximum {
                ranges[row] = minimum...maximum
            }
        }
        let set = { (field: OptionField) in number(raw.deckValues[field]) != nil }
        return DeckOptionsPageSnapshot(deckID: raw.deck, deckName: raw.name, assignedPreset: raw.assignedPreset,
            openingPreset: raw.selectedPreset, presets: presets,
            ownLimits: set(.newLimit) || set(.reviewLimit) || set(.newLimitToday) || set(.reviewLimitToday),
            ownRetention: set(.deckDesiredRetention), ranges: ranges,
            displayOrderChoices: Dictionary(uniqueKeysWithValues: DeckOptionsDisplayOrder.allCases.map { row in
                (row, raw.metadata.first { $0.field == row.field }?.choices ?? [])
            }))
    }
}
