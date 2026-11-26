import CoreBridge
import Foundation
import Observation

/// F1–F6: drafts and operation admission, never scheduling or card membership.
@Observable @MainActor
public final class FilteredStudyModel {
    public enum Failure: Equatable { case load, write, noMatches, refresh, saved }
    public var definition = FilteredStudyModel.blankDefinition
    public var secondTerm = FilteredStudyTerm(search: "", limit: 20, order: 6)
    public var usesSecondTerm = false
    public var modeIndex = 0 {
        didSet {
            switch modeIndex {
            case 0: amount = defaults?.extendNew ?? 1
            case 1: amount = defaults?.extendReview ?? 1
            case 5: amount = 100
            default: amount = 1
            }
        }
    }
    public var stateIndex = 0
    public var amount: Int32 = 1 {
        didSet { if amountText != String(amount) { amountText = String(amount) } }
    }
    public var amountText = "1" {
        didSet { if let value = Int32(amountText), value != amount { amount = value } }
    }
    public var firstLimitText = "100" {
        didSet { if let value = UInt32(firstLimitText), !definition.terms.isEmpty { definition.terms[0].limit = value } }
    }
    public var secondLimitText = "20" {
        didSet { if let value = UInt32(secondLimitText) { secondTerm.limit = value } }
    }
    public var againText = "60" {
        didSet { if let value = UInt32(againText) { definition.againSeconds = value } }
    }
    public var hardText = "600" {
        didSet { if let value = UInt32(hardText) { definition.hardSeconds = value } }
    }
    public var goodText = "0" {
        didSet { if let value = UInt32(goodText) { definition.goodSeconds = value } }
    }
    private func readNumericDraft() {
        firstLimitText = String(definition.terms.first?.limit ?? 100)
        secondLimitText = String(secondTerm.limit)
        againText = String(definition.againSeconds); hardText = String(definition.hardSeconds)
        goodText = String(definition.goodSeconds)
    }
    /// One literal tag per line: whitespace and search punctuation belong to the tag.
    public var includeTags = ""
    public var excludeTags = ""
    public private(set) var defaults: CustomStudyDefaults?
    public private(set) var busy = false
    public private(set) var loaded = false
    public private(set) var failure: Failure?
    public var operationFailed = false
    public private(set) var operationNeedsRefresh = false
    public private(set) var editingDeck: Int64?
    public private(set) var sourceDeck: Int64?
    private weak var owner: ReviewCollection?
    private let service: any FilteredStudyService
    private var route: SheetRoute?
    private var generation: UInt64 = 0
    private var revision: UInt64 = 0
    private var intent: UInt64 = 0
    private var scopeDeck: Int64?
    private var emptyCandidate: FilteredStudyDefinition?
    private var savedDeck: Int64?
    private var writeCommitted = false
    private var committedMutationGeneration: UInt64?
    private var task: Task<Void, Never>?

    public init(owner: ReviewCollection, service: any FilteredStudyService = CoreFilteredStudyService()) {
        self.owner = owner; self.service = service
    }
    public static var blankDefinition: FilteredStudyDefinition {
        .init(name: "", terms: [.init(search: "", limit: 100, order: 1)], reschedule: true,
              againSeconds: 60, hardSeconds: 600, goodSeconds: 0)
    }
    public var sourceName: String { sourceDeck.flatMap { owner?.fullDeckName($0) } ?? "" }
    public var canPresent: Bool {
        guard let owner else { return false }
        return !busy && owner.deckWriteAllowed(host: .main)
    }
    public func canEdit(_ deck: Int64) -> Bool { canPresent && owner?.deck(deck)?.filtered == true }
    public func canCustom(_ deck: Int64) -> Bool { canPresent && owner?.deck(deck)?.filtered == false }

    public func create(deck: Int64? = nil, search: String? = nil) {
        guard canPresent else { return }
        definition = Self.blankDefinition
        secondTerm = .init(search: "", limit: 20, order: 6); usesSecondTerm = false
        if let search { definition.terms[0].search = search }
        readNumericDraft()
        begin(.filteredStudy, deck: nil, source: nil)
        if search == nil, let deck, owner?.deck(deck)?.filtered == false {
            scopeDeck = deck
            load()
        } else { loaded = true }
    }
    public func browseCards(_ deck: Int64) {
        guard canPresent, let owner, owner.deckImportBrowseAvailable else { return }
        revision &+= 1
        let expected = owner.generation, intent = owner.navigation.intentRevision, revision = revision
        busy = true; operationFailed = false; operationNeedsRefresh = false
        task = Task {
            defer { busy = false; task = nil }
            // Content can be absent while a read runs. Only the operation that
            // still owns the foreground route may navigate or publish an alert.
            @MainActor func current() -> Bool {
                self.revision == revision && !Task.isCancelled && busy &&
                    owner.generation == expected && owner.navigation.intentRevision == intent &&
                    owner.foreground && !owner.resuming && owner.browseReadable &&
                    owner.deckImportBrowseAvailable && owner.navigation.sheet == nil
            }
            do {
                let search = try await service.deckScope(deck: deck, generation: expected)
                guard current() else { return }
                owner.navigation.browseSearch = search
                owner.navigation.select(.section(.browse))
            } catch {
                if current() { operationFailed = true }
            }
        }
    }
    public func createFromBrowse() {
        guard canPresent, let owner else { return }
        // Freeze the whole active card query before the asynchronous serialization.
        let query = owner.browse.query, expected = owner.generation, intent = owner.navigation.intentRevision
        busy = true; operationFailed = false; operationNeedsRefresh = false
        task = Task {
            defer { busy = false; task = nil }
            do {
                let search = try await CoreBrowseQuery.runText(query)
                guard expected == owner.generation, intent == owner.navigation.intentRevision else { return }
                busy = false
                create(search: search)
            } catch {
                if expected == owner.generation, intent == owner.navigation.intentRevision,
                   owner.foreground, !owner.resuming, owner.navigation.sheet == nil { operationFailed = true }
            }
        }
    }
    public func edit(_ deck: Int64) {
        guard canEdit(deck) else { return }
        begin(.filteredStudy, deck: deck, source: nil)
        load()
    }
    public func custom(_ deck: Int64) {
        guard canCustom(deck) else { return }
        begin(.customStudy(deck), deck: nil, source: deck)
        modeIndex = 0; stateIndex = 0; includeTags = ""; excludeTags = ""; defaults = nil
        load()
    }
    private func begin(_ route: SheetRoute, deck: Int64?, source: Int64?) {
        guard let owner else { return }
        revision &+= 1; generation = owner.generation
        editingDeck = deck; sourceDeck = source; failure = nil; loaded = false; savedDeck = nil
        writeCommitted = false
        operationFailed = false; operationNeedsRefresh = false
        scopeDeck = nil; emptyCandidate = nil; self.route = route
        owner.navigation.present(route)
        intent = owner.navigation.intentRevision
    }
    private func current(_ revision: UInt64) -> Bool {
        guard let owner else { return false }
        return revision == self.revision && generation == owner.generation &&
            intent == owner.navigation.intentRevision && owner.navigation.sheet == route &&
            owner.navigation.sheetHost == .main && owner.state == .ready
    }
    private var admitted: Bool {
        guard let owner, let route else { return false }
        return current(revision) && owner.deckNamesAllowed(host: .main, route: route) &&
            (editingDeck.map { owner.deck($0)?.filtered == true } ?? true) &&
            (sourceDeck.map { owner.deck($0)?.filtered == false } ?? true)
    }
    public func load() {
        guard !busy, !writeCommitted, admitted else { return }
        busy = true; failure = nil
        let revision = revision, generation = generation
        task = Task {
            defer { busy = false; task = nil }
            do {
                if let deck = scopeDeck {
                    let scope = try await service.deckScope(deck: deck, generation: generation)
                    guard current(revision) else { return }
                    definition.terms[0].search = scope + " is:due"
                    secondTerm.search = scope + " is:new"
                } else if let deck = editingDeck {
                    let value = try await service.definition(deck: deck, generation: generation)
                    guard current(revision) else { return }
                    definition = value
                    usesSecondTerm = value.terms.count > 1
                    secondTerm = value.terms.dropFirst().first ?? .init(search: "", limit: 20, order: 6)
                    definition.terms = [value.terms.first ?? .init(search: "", limit: 100, order: 1)]
                    readNumericDraft()
                } else if let deck = sourceDeck {
                    let value = try await service.customDefaults(deck: deck, generation: generation)
                    guard current(revision) else { return }
                    defaults = value; amount = value.extendNew
                    includeTags = value.includeTags.joined(separator: "\n")
                    excludeTags = value.excludeTags.joined(separator: "\n")
                }
                loaded = true
            } catch { if current(revision) { failure = .load } }
        }
    }
    public var canSubmit: Bool {
        guard !busy, loaded, admitted, !writeCommitted else { return false }
        if sourceDeck != nil {
            guard Int32(amountText) != nil else { return false }
            if modeIndex < 2 { return (-99_999...99_999).contains(amount) }
            return amount > 0 && (modeIndex != 2 || amount <= 30)
        }
        guard UInt32(firstLimitText) != nil, (!usesSecondTerm || UInt32(secondLimitText) != nil),
              definition.reschedule || (UInt32(againText) != nil && UInt32(hardText) != nil && UInt32(goodText) != nil) else { return false }
        return !definition.name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty &&
            definition.terms.count == 1 && definition.terms[0].limit > 0 && (!usesSecondTerm || secondTerm.limit > 0)
    }
    public var needsRefresh: Bool { writeCommitted && failure != .saved && !busy }
    public var customRequest: CustomStudyRequest {
        let modes: [CustomStudyMode] = [.increaseNew, .increaseReview, .forgotten, .ahead, .previewNew, .byState]
        let states: [CustomStudyState] = [.new, .due, .review, .all]
        return .init(mode: modes[max(0, min(modeIndex, 5))], amount: amount,
                     state: states[max(0, min(stateIndex, 3))],
                     includeTags: Self.tags(includeTags), excludeTags: Self.tags(excludeTags))
    }
    public static func tags(_ text: String) -> [String] {
        text.components(separatedBy: .newlines).filter { !$0.isEmpty }
    }
    private var draftDefinition: FilteredStudyDefinition {
        var value = definition
        if usesSecondTerm { value.terms.append(secondTerm) }
        return value
    }
    public var canSaveEmpty: Bool {
        canSubmit && sourceDeck == nil && emptyCandidate == draftDefinition
    }
    public func submit() { submit(allowEmpty: false) }
    public func confirmEmpty() {
        guard canSaveEmpty else { return }
        submit(allowEmpty: true)
    }
    private func submit(allowEmpty: Bool) {
        guard canSubmit else { return }
        busy = true; failure = nil; emptyCandidate = nil
        let revision = revision, generation = generation
        let value = draftDefinition, request = customRequest
        let source = sourceDeck, editing = editingDeck
        task = Task {
            defer { busy = false; task = nil }
            guard current(revision), admitted else { return }
            do {
                let selected: Int64
                if let source { selected = try await service.custom(deck: source, request: request, generation: generation) }
                else { selected = try await service.save(deck: editing, definition: value, allowEmpty: allowEmpty, generation: generation) }
                guard revision == self.revision, generation == owner?.generation else { return }
                writeCommitted = true
                savedDeck = selected
                guard current(revision) else { return }
                failure = .refresh
                await publish(revision)
            } catch BridgeError.commitCompleted {
                // The mutation is durable even though its result (including a
                // newly created deck's identity) is unavailable. Never replay it.
                guard revision == self.revision, generation == owner?.generation else { return }
                writeCommitted = true
                guard current(revision) else { return }
                failure = .refresh
                await publish(revision)
            } catch {
                guard current(revision) else { return }
                if case BridgeError.deckRefused(.noMatches) = error {
                    failure = .noMatches
                    if source == nil { emptyCandidate = value }
                } else { failure = .write }
            }
        }
    }
    private func publish(_ revision: UInt64) async {
        guard let owner, writeCommitted, current(revision), owner.foreground, !owner.resuming else { return }
        owner.deckWriteFinished()
        do {
            guard try await owner.refreshDeckNames(generation: generation), current(revision) else {
                if current(revision) { failure = .refresh }; return
            }
            guard let savedDeck else {
                // Keep the refreshed workspace's selection. The saved result
                // may have created a deck; neither its name nor source is an id.
                failure = .saved
                owner.refreshCounts()
                return
            }
            // Selection writes through the normal collection route; its own stale-intent fence wins.
            let selection = owner.openDeckPage(deckID: savedDeck)
            intent = owner.navigation.intentRevision
            guard await selection.value else {
                if current(revision) { failure = .refresh }
                return
            }
            guard revision == self.revision, generation == owner.generation, owner.navigation.sheet == route,
                  owner.navigation.homeDeckPage?.rawValue == savedDeck else { return }
            owner.navigation.dismissSheet(); self.route = nil; self.savedDeck = nil
            writeCommitted = false; failure = nil
            owner.refreshCounts()
        } catch { if current(revision) { failure = .refresh } }
    }
    public func refresh() {
        guard needsRefresh, let owner, generation == owner.generation,
              owner.navigation.sheet == route else { return }
        intent = owner.navigation.intentRevision
        busy = true
        let revision = revision
        task = Task { defer { busy = false; task = nil }; await publish(revision) }
    }
    public func cancel() {
        guard !busy else { return }
        if let owner, owner.navigation.sheet == route { owner.navigation.dismissSheet() }
        route = nil; revision &+= 1; savedDeck = nil; writeCommitted = false; emptyCandidate = nil
    }
    /// Rebuild may legitimately produce an empty deck. No Swift-side search or scheduler.
    public func rebuild(_ deck: Int64) { mutate(deck, empty: false) }
    public func empty(_ deck: Int64) { mutate(deck, empty: true) }
    private func mutate(_ deck: Int64, empty: Bool) {
        guard canEdit(deck), let owner else { return }
        // An acknowledged write must never be replayed as refresh recovery,
        // even after the alert was dismissed or the foreground route changed.
        if committedMutationGeneration != nil { refreshOperation(); return }
        revision &+= 1
        busy = true; operationFailed = false; operationNeedsRefresh = false
        let revision = revision
        let expected = owner.generation, intent = owner.navigation.intentRevision
        task = Task {
            defer { busy = false; task = nil }
            do {
                do {
                    if empty { try await service.empty(deck: deck, generation: expected) } else { try await service.rebuild(deck: deck, generation: expected) }
                } catch BridgeError.commitCompleted {
                    // As with a saved definition, recover by reading committed state.
                }
                guard revision == self.revision, expected == owner.generation else { return }
                committedMutationGeneration = expected
                owner.deckWriteFinished()
            } catch {
                if operationCurrent(revision, generation: expected, intent: intent) { operationFailed = true }
                return
            }
            await publishMutation(revision, generation: expected, intent: intent)
        }
    }
    private func operationCurrent(_ revision: UInt64, generation: UInt64, intent: UInt64) -> Bool {
        guard let owner else { return false }
        return self.revision == revision && !Task.isCancelled && owner.generation == generation &&
            owner.navigation.intentRevision == intent && owner.navigation.sheet == nil &&
            owner.foreground && !owner.resuming && owner.state == .ready
    }
    private func publishMutation(_ revision: UInt64, generation: UInt64, intent: UInt64) async {
        guard let owner, committedMutationGeneration == generation,
              operationCurrent(revision, generation: generation, intent: intent) else { return }
        do {
            let refreshed = try await owner.refreshDeckNames(generation: generation)
            guard operationCurrent(revision, generation: generation, intent: intent) else { return }
            if refreshed {
                committedMutationGeneration = nil; operationNeedsRefresh = false
                owner.refreshCounts()
                return
            }
        } catch {
            guard operationCurrent(revision, generation: generation, intent: intent) else { return }
        }
        operationNeedsRefresh = true; operationFailed = true
    }
    /// The native alert's recovery command reads only; it cannot Empty or Rebuild.
    public func refreshOperation() {
        guard !busy, let owner, let expected = committedMutationGeneration,
              expected == owner.generation, owner.deckWriteAllowed(host: .main) else { return }
        busy = true; operationFailed = false; operationNeedsRefresh = false
        let revision = revision, intent = owner.navigation.intentRevision
        task = Task {
            defer { busy = false; task = nil }
            await publishMutation(revision, generation: expected, intent: intent)
        }
    }
    func invalidate() {
        revision &+= 1; loaded = false; failure = nil; savedDeck = nil; writeCommitted = false; emptyCandidate = nil; operationFailed = false
        committedMutationGeneration = nil; operationNeedsRefresh = false
        if let owner, route != nil, owner.navigation.sheet == route { owner.navigation.dismissSheet() }
        route = nil
    }
    public func waitForOperation() async { await task?.value }
}
