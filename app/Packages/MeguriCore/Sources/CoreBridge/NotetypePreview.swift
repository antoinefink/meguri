import Foundation
import Instrumentation
internal import MeguriCoreBindings

// The Cards tab's live preview (U283, `notetypes/workspace` W9;
// `render/preview` V2 to V11): one card of a draft, unsaved texts and
// styling included, for a stored note of the type or a new empty note, and
// the type's note ids for the note menu. Both read one snapshot and write
// nothing; they follow typing, so they run on the interactive lane.

/// One preview render and the note it rendered.
public struct NotetypeDraftRender: Sendable, Hashable {
    public let card: RenderedCard
    /// The stored note rendered; nil for a new empty note.
    public let noteID: Int64?
    /// A cloze draft's card numbers, ascending; empty otherwise.
    public let clozeNumbers: [UInt32]

    init(_ raw: MeguriCoreBindings.NotetypeDraftRender) {
        card = RenderedCard(raw.card)
        noteID = raw.noteId
        clozeNumbers = raw.clozeNumbers
    }
}

extension CoreNotetypeService {
    /// V2 for a draft: `target` is a card type's position in the draft, or
    /// a cloze number minus one for a cloze note type.
    public func renderNotetypeDraft(
        _ draft: NotetypeStructureDraft, note: Int64?, target: UInt32, fillEmpty: Bool, platform: CardRenderPlatform
    ) async throws(BridgeError) -> NotetypeDraftRender {
        let request = draft.raw
        let rawPlatform: MeguriCoreBindings.RenderPlatform = platform == .mac ? .mac : .ios
        return try await InteractiveLane.shared.call(.renderNotetypeDraft) {
            NotetypeDraftRender(try MeguriCoreBindings.renderNotetypeDraft(
                draft: request, noteId: note, target: target, fillEmpty: fillEmpty, platform: rawPlatform))
        }
    }

    /// W9's note menu: the type's note ids, ascending.
    public func notetypeNoteIds(_ notetypeId: Int64) async throws(BridgeError) -> [Int64] {
        try await InteractiveLane.shared.call(.notetypeNoteIds) {
            try MeguriCoreBindings.notetypeNoteIds(notetypeId: notetypeId)
        }
    }
}
