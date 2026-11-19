import Foundation
import Observation

/// Presentation admission is separate from the collection-owned field/save
/// lifetime. In particular, an unsuccessful open cannot replace another modal.
@Observable @MainActor
public final class AddNotePresentation {
    public enum Issue: Equatable {
        case unavailable, otherPresentation, reviewDeferred
        case failed(String)
    }
    public private(set) var issue: Issue?
    public private(set) var opening = false
    @ObservationIgnored private weak var owner: ReviewCollection?
    @ObservationIgnored private var prepare: (@MainActor (Int64?) async -> Bool)?

    init(owner: ReviewCollection) { self.owner = owner }

    /// Editor installs its actual prepareAddNoteEditor adapter once. Neither
    /// an appear nor a layout remount opens or disposes a field runtime.
    public func configure(prepare: @escaping @MainActor (Int64?) async -> Bool) {
        if self.prepare == nil { self.prepare = prepare }
        owner?.navigation.addSheetRequest = { [weak self] route in self?.request(route) }
    }

    public func canPresent(presetDeck: Int64? = nil) -> Bool {
        guard let owner, prepare != nil, !opening, owner.canBeginAddAction,
              owner.review == nil, owner.navigation.review == nil,
              owner.navigation.sheet == nil, owner.reviewIsActive else { return false }
        // A normal empty deck need not be in the tree. The real preparation
        // validates its persisted ID; no name-derived or invented ID is used.
        if let presetDeck, owner.deck(presetDeck)?.filtered == true { return false }
        return true
    }

    public func clearIssue() { issue = nil }

    /// N17.1: an iPhone at regular width (the Duo opened wide) shows Add Note
    /// over the whole screen. Every other layout keeps the system sheet.
    nonisolated public static func coversScreen(phone: Bool, regularWidth: Bool) -> Bool { phone && regularWidth }

    private func request(_ route: SheetRoute?) {
        guard let owner else { return }
        let navigation = owner.navigation
        guard case .add(let deck) = route else {
            guard route == nil else { issue = .otherPresentation; return }
            // A dismissal while open is preparing cannot cancel its runtime
            // halfway through start; the close control is disabled until then.
            guard !opening else { issue = .unavailable; return }
            Task {
                if await owner.addNoteFlow.requestClose(), navigation.sheet.isAddNote {
                    issue = nil
                    navigation.commitAddSheet(nil)
                }
            }
            return
        }
        guard navigation.sheet == nil || navigation.sheet.isAddNote else {
            issue = .otherPresentation; return
        }
        guard owner.review == nil, navigation.review == nil else { issue = .reviewDeferred; return }
        guard let prepare, !opening, owner.canBeginAddAction,
              navigation.sheet.isAddNote || owner.reviewIsActive else { issue = .unavailable; return }
        opening = true; issue = nil
        let prior = navigation.sheet, priorHost = navigation.sheetHost
        let generation = owner.generation
        Task {
            let opened = await prepare(deck?.rawValue)
            defer { opening = false }
            guard navigation.sheet == prior, navigation.sheetHost == priorHost,
                  owner.generation == generation, owner.canBeginAddAction,
                  owner.review == nil, navigation.review == nil,
                  prior.isAddNote || owner.reviewIsActive else {
                issue = .unavailable
                // Preparation has no user input yet when no Add was showing.
                // Its ordinary guard still owns any cleanup, including failure.
                if prior == nil, owner.addNoteFlow.draft?.unchangedSinceLastAdd == true {
                    _ = await owner.addNoteFlow.requestClose()
                }
                return
            }
            guard opened else {
                issue = owner.addNoteFlow.error.map(Issue.failed) ?? .unavailable
                return
            }
            navigation.commitAddSheet(route)
        }
    }
}

extension ReviewCollection {
    public var addNotePresentation: AddNotePresentation {
        if let retained = navigation.retainedAddPresentation { return retained }
        let presentation = AddNotePresentation(owner: self)
        navigation.retainedAddPresentation = presentation
        return presentation
    }
}
