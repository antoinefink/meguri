import Foundation

/// Edit › Undo and Redo (R173, R174, N20.4): one item each, titled after what
/// it undoes. A text input that owns undo keeps it; during review the answers
/// are the target while the card owns the keys; while Note Types shows and its
/// draft has steps, the draft's own undo (`notetypes/workspace` W13); otherwise
/// the collection's history.
public nonisolated enum EditUndoRouting {
    public enum Target: Equatable, Sendable { case text, review, draft, collection }
    public enum Title: Equatable, Sendable {
        /// The text input's own title ("Undo Typing").
        case text(String)
        /// "Undo <step>" with the step's name.
        case named(String)
        /// Plain "Undo", disabled.
        case plain
    }
    public struct Route: Equatable, Sendable {
        public let target: Target
        public let title: Title
        public let enabled: Bool
    }
    /// One direction's facts: whether its side can act and the step's name.
    public struct Inputs: Equatable, Sendable {
        public var textOwnsUndo = false
        public var textAvailable = false
        public var textTitle: String?
        public var reviewShown = false
        public var reviewOwnsKeys = false
        /// The review's newest step while it can be undone (or redone).
        public var reviewStep: String?
        /// The Note Types draft's newest step while Note Types shows (W13).
        public var draftStep: String?
        /// The collection's newest step while it can be undone (or redone).
        public var collectionStep: String?
        public init(textOwnsUndo: Bool = false, textAvailable: Bool = false, textTitle: String? = nil,
                    reviewShown: Bool = false, reviewOwnsKeys: Bool = false, reviewStep: String? = nil,
                    collectionStep: String? = nil) {
            self.textOwnsUndo = textOwnsUndo; self.textAvailable = textAvailable; self.textTitle = textTitle
            self.reviewShown = reviewShown; self.reviewOwnsKeys = reviewOwnsKeys; self.reviewStep = reviewStep
            self.collectionStep = collectionStep
        }
    }

    public static func route(_ inputs: Inputs) -> Route {
        if inputs.textOwnsUndo {
            let title = inputs.textTitle.map(Title.text) ?? .plain
            return Route(target: .text, title: inputs.textAvailable ? title : .plain, enabled: inputs.textAvailable)
        }
        if inputs.reviewShown {
            guard inputs.reviewOwnsKeys, let step = inputs.reviewStep else {
                return Route(target: .review, title: .plain, enabled: false)
            }
            return Route(target: .review, title: .named(step), enabled: true)
        }
        if let step = inputs.draftStep { return Route(target: .draft, title: .named(step), enabled: true) }
        guard let step = inputs.collectionStep else { return Route(target: .collection, title: .plain, enabled: false) }
        return Route(target: .collection, title: .named(step), enabled: true)
    }
}
