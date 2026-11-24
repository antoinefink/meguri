import CoreBridge
import Foundation

/// W10's Card Text row over the styling (`notetypes/workspace` W10 to
/// W10.2; N22.4): what its five controls show, read from the draft's CSS,
/// and one control's change, written into the CSS as ordinary text through
/// the styling's text view (so its own undo takes it back). The CSS is
/// never reformatted: the core edits only the one value.
extension NoteTypesModel {
    /// The row for `css`, or nil when the row is absent (no single plain
    /// `.card` rule, or CSS that is not well formed).
    public func cardTextRow(_ css: String, rightToLeft: Bool) async -> CardTextRow? {
        (try? await backend.cardText(css: css, rightToLeft: rightToLeft)) ?? nil
    }

    /// One control's change on the shown note type's styling. Nothing is
    /// written when the styling changed while the edit was worked out, or
    /// when the core refuses it.
    public func changeCardText(_ id: Int64, card: Int, _ change: CardTextChange, rightToLeft: Bool) async {
        guard editable(id), let css = draft(id)?.css else { return }
        guard let edited = try? await backend.editCardText(css: css, change: change, rightToLeft: rightToLeft),
              editable(id), draft(id)?.css == css, edited.css != css else { return }
        let start = edited.css.utf16.distance(from: edited.css.startIndex, to: edited.value.lowerBound)
        let end = edited.css.utf16.distance(from: edited.css.startIndex, to: edited.value.upperBound)
        request(id, card: card, part: .styling, .replaceHighlighting(edited.css, highlight: start..<end))
    }
}
