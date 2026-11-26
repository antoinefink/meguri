import CoreBridge
import Foundation
import Observation

/// F1–F6: drafts and operation admission, never scheduling or card membership.
@Observable @MainActor
public final class FilteredStudyModel {
    public enum Failure: Equatable { case load, write, refresh }
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
    public private(set) var editingDeck: Int64?
    public private(set) var sourceDeck: Int64?
    private weak var owner: ReviewCollection?
    private let service: any FilteredStudyService
    private var route: SheetRoute?
    private var generation: UInt64 = 0
    private var revision: UInt64 = 0
    private var intent: UInt64 = 0
    private var savedDeck: Int64?
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

    /// A quoted literal operand; the Core owns interpretation of the Browse grammar.
    public static func deckSearch(_ name: String) -> String {
        // N3.2's two lowercase keywords are special even inside quotes.
        let literalName = name == "current" ? "Current" : name == "filtered" ? "Filtered" : name
        let literal = literalName.replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "\"", with: "\\\"")
            .replacingOccurrences(of: "*", with: "\\*")
            .replacingOccurrences(of: "_", with: "\\_")
        return "deck:\"\(literal)\""
    }
    public func create(deck: Int64? = nil, search: String? = nil) {
        guard canPresent else { return }
        definition = Self.blankDefinition
        secondTerm = .init(search: "", limit: 20, order: 6); usesSecondTerm = false
        if let deck, let name = owner?.fullDeckName(deck), owner?.deck(deck)?.filtered == false {
            definition.terms[0].search = Self.deckSearch(name) + " is:due"
            secondTerm.search = Self.deckSearch(name) + " is:new"
        }
        if let search { definition.terms[0].search = search }
        readNumericDraft()
        begin(.filteredStudy, deck: nil, source: nil)
        loaded = true
    }
    public func createFromBrowse() {
        guard canPresent, let owner else { return }
        // Freeze the whole active card query before the asynchronous serialization.
        let query = owner.browse.query, expected = owner.generation, intent = owner.navigation.intentRevision
        busy = true
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
        operationFailed = false; self.route = route
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
        guard !busy, admitted else { return }
        busy = true; failure = nil
        let revision = revision
        task = Task {
            defer { busy = false; task = nil }
            do {
                if let deck = editingDeck {
                    let value = try await service.definition(deck: deck)
                    guard current(revision) else { return }
                    definition = value
                    usesSecondTerm = value.terms.count > 1
                    secondTerm = value.terms.dropFirst().first ?? .init(search: "", limit: 20, order: 6)
                    definition.terms = [value.terms.first ?? .init(search: "", limit: 100, order: 1)]
                    readNumericDraft()
                } else if let deck = sourceDeck {
                    let value = try await service.customDefaults(deck: deck)
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
        guard !busy, loaded, admitted, savedDeck == nil else { return false }
        if sourceDeck != nil {
            guard Int32(amountText) != nil else { return false }
            if modeIndex < 2 { return true }
            return amount > 0 && (modeIndex != 2 || amount <= 30)
        }
        guard UInt32(firstLimitText) != nil, (!usesSecondTerm || UInt32(secondLimitText) != nil),
              definition.reschedule || (UInt32(againText) != nil && UInt32(hardText) != nil && UInt32(goodText) != nil) else { return false }
        return !definition.name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty &&
            definition.terms.count == 1 && definition.terms[0].limit > 0 && (!usesSecondTerm || secondTerm.limit > 0)
    }
    public var needsRefresh: Bool { savedDeck != nil && !busy }
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
    public func submit() {
        guard canSubmit else { return }
        busy = true; failure = nil
        let revision = revision
        var value = definition
        if usesSecondTerm { value.terms.append(secondTerm) }
        let request = customRequest
        task = Task {
            defer { busy = false; task = nil }
            guard admitted else { return }
            do {
                let selected: Int64
                if let sourceDeck { selected = try await service.custom(deck: sourceDeck, request: request) }
                else { selected = try await service.save(deck: editingDeck, definition: value) }
                guard current(revision) else { return }
                savedDeck = selected
                await publish(revision)
            } catch { if current(revision) { failure = .write } }
        }
    }
    private func publish(_ revision: UInt64) async {
        guard let owner, let savedDeck, current(revision), owner.foreground, !owner.resuming else { return }
        owner.deckWriteFinished()
        do {
            guard try await owner.refreshDeckNames(generation: generation), current(revision) else {
                if current(revision) { failure = .refresh }; return
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
            owner.refreshCounts()
        } catch { if current(revision) { failure = .refresh } }
    }
    public func refresh() {
        guard !busy, savedDeck != nil, let owner, generation == owner.generation,
              owner.navigation.sheet == route else { return }
        intent = owner.navigation.intentRevision
        busy = true
        let revision = revision
        task = Task { defer { busy = false; task = nil }; await publish(revision) }
    }
    public func cancel() {
        guard !busy else { return }
        if let owner, owner.navigation.sheet == route { owner.navigation.dismissSheet() }
        route = nil; revision &+= 1; savedDeck = nil
    }
    /// Rebuild may legitimately produce an empty deck. No Swift-side search or scheduler.
    public func rebuild(_ deck: Int64) { mutate(deck, empty: false) }
    public func empty(_ deck: Int64) { mutate(deck, empty: true) }
    private func mutate(_ deck: Int64, empty: Bool) {
        guard canEdit(deck), let owner else { return }
        busy = true; operationFailed = false
        let expected = owner.generation, intent = owner.navigation.intentRevision
        task = Task {
            defer { busy = false; task = nil }
            do {
                if empty { try await service.empty(deck: deck) } else { try await service.rebuild(deck: deck) }
                guard expected == owner.generation else { return }
                owner.deckWriteFinished()
                guard try await owner.refreshDeckNames(generation: expected) else { return }
                if intent == owner.navigation.intentRevision { owner.refreshCounts() }
            } catch {
                if expected == owner.generation, intent == owner.navigation.intentRevision,
                   owner.navigation.sheet == nil, owner.foreground, !owner.resuming { operationFailed = true }
            }
        }
    }
    func invalidate() {
        revision &+= 1; loaded = false; failure = nil; savedDeck = nil; operationFailed = false
        if let owner, route != nil, owner.navigation.sheet == route { owner.navigation.dismissSheet() }
        route = nil
    }
    public func waitForOperation() async { await task?.value }
}
