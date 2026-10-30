/// A sheet the shell presents over the current screen (R25). Closing it
/// returns to exactly that screen: the screen underneath is never replaced
/// while a sheet is up. Units add cases for their sheets (deck dialogs,
/// import and export, login); nothing opens a second window (R6).
nonisolated public enum SheetRoute: Hashable, Identifiable, Sendable {
    /// Add (U195), with `deck` preselected when given (R48).
    case add(deck: DeckID?)
    case goToDeck
    case fsrsAdmission
    case collectionEntry
    case collectionExport
    case deckName(DeckActionsModel.Action)
    /// Explicit server-transfer preparation, decisions and retained history.
    case manualTransfer
    case cloudSync(CloudSyncEntry)
    /// Account & Sync over mobile review; closing preserves the retained card.
    case accountSync

    public var id: Self { self }
}
