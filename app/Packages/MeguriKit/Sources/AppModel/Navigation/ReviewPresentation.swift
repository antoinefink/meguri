/// An open review session as the shell presents it (R21, R23): full screen
/// over the tab bar in the tab layout, the whole detail area in the sidebar
/// layout. The session model (U175) is held here, by the navigation, never
/// by a view: a layout switch rebuilds the views around the same session, on
/// the same card and side (R5).
public struct ReviewPresentation: Identifiable {
    public let deck: DeckID
    /// The review session's model.
    public let session: AnyObject

    public init(deck: DeckID, session: AnyObject) {
        self.deck = deck
        self.session = session
    }

    public var id: ObjectIdentifier { ObjectIdentifier(session) }
}

extension ReviewPresentation: Equatable {
    public static func == (lhs: ReviewPresentation, rhs: ReviewPresentation) -> Bool {
        lhs.deck == rhs.deck && lhs.session === rhs.session
    }
}
