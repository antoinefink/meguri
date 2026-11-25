/// A sheet the shell presents over the current screen (R25). Closing it
/// returns to exactly that screen: the screen underneath is never replaced
/// while a sheet is up. Units add cases for their sheets (deck dialogs,
/// import and export, login); nothing opens a second window (R6).
nonisolated public enum SheetRoute: Hashable, Identifiable, Sendable {
    /// Add (U195), with `deck` preselected when given (R48).
    case add(deck: DeckID?)
    case collectionEntry
    /// The deck package import sheet (U162, `package/transfer-sheets` X2).
    case deckImport
    case collectionExport
    case deckName(DeckActionsModel.Action)
    /// Names a preset in the collection-owned Deck Options draft (N20.9).
    case newDeckOptionsPreset
    /// Explicit server-transfer preparation, decisions and retained history.
    case manualTransfer
    case cloudSync(CloudSyncEntry)
    /// Account & Sync over mobile review; closing preserves the retained card.
    case accountSync
    /// The Note Types section's Save sheet (`notetypes/draft-save` D4), for
    /// the question `ReviewCollection.noteTypes` holds.
    case noteTypeSave

    public var id: Self { self }
}
