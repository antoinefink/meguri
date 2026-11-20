# Edit flows

Spec: editor/edit-flows
Requirements: R819, R822, R824, R863, R245 (Edit), R257, R259 (Edit), R1793 (editor side), R1550 (editor side); supports R818, R820, R821, R823, R776, R847, R938, R2048 (editor side); AE40, AE80 (editor part)
Anki: 26.09.3 for W2, W5 and W8's comparison; none for W1's matching, W5.3, W6, W9 and W11 (Meguri's design)
Scenarios: none of its own; the core writes are `conformance/scenarios/editor/field-html/patch/` (`editor/field-html` T3)

How an editor bound to an existing note behaves in its hosts (U196): Browse's editor pane, the review screen's edit panel (U184, which owns its placement and the card's re-render on close) and any other host of the editor in Edit or Browse mode. It covers when the pane shows and saves, how the panel closes, and what the editor does when another writer changes, replaces or deletes its note. The editor's commits, its field patch and the patch's failures are `editor/field-html` S1 and S2; the no-op guarantee is `editor/field-html` H2 with `notes/note-records` U2; the tag area is `editor/tag-editor`. Undo of the editor's saves is `engine/undo` (R821).

## W1. Terms

- **Host**: the screen that shows the editor bound to note N: Browse's pane, the review edit panel.
- **Own patch**: a patch this host submitted for N (`editor/field-html` S2). The host keeps the seq of each own patch from the op's result (`engine/op-model` O4) for as long as the editor stays bound to N. A patch is **in flight** from its submission until its result arrives, a refusal or failure included (it always arrives: S2's patch is refused, never parked, while the collection is unavailable).
- **Matching** (`editor/field-html` S2's "matched by op seq"). The `Changes` event of a patch and the patch's result reach the host on different paths (`engine/change-stream` S3 through U6's dispatcher; the op's completion through the interactive queue), in either order. So while any own patch for N is in flight, the host holds every `Changes` event that lists N or N's note type, in seq order, without acting on it; when no own patch is in flight any more, it handles the held events in seq order, skipping those whose seq is an own patch's. An event that arrives while nothing is in flight is handled at once. Holding never delays typing or a commit, only the host's response to a change.
- **Unsaved edit**: a field with input since its last commit (an idle timer running), a field whose session target differs from its last committed value (a patch submitted and not committed yet, or one that failed), or a tag edit not handed on yet (`editor/tag-editor` G11).
- **Foreign change** to N: a `Changes` event (`engine/change-stream` S3) whose seq is not an own patch's and whose change set lists N in `notes` (`Updated` or `Removed`), or lists N's note type in `notetypes` (W5).

## W2. Browse's editor pane (R819, R257, R863)

- The pane shows the editor when exactly one row is selected: in cards mode the note of that card, in notes mode that note. With no row, or more than one, the pane shows no editor (screen contract). The card the editor refers to (for Cards… and preview) is the selected row's card in cards mode and, in notes mode, the selected note's card with the lowest `ord` (the rule `browser/bulk-actions` B2 gives for a note's card, applied to the selected row, not to the focused one, which can differ).
- **Flush first.** Before the selection changes, a search runs, the mode switches, the sort changes, any Browse action or menu command runs, the pane is hidden and Browse closes, Browse asks the pane to flush with `hold` (`editor/field-html` B3) and waits for the reply. The reply waits for the flush's patches (S2). Then the change proceeds. When the pane then shows another note, its `openNote` ends the hold; when it keeps the same note (a sort, a search that keeps the row selected, an action on that row such as Change Deck) or hides the editor, Browse sends `release` once the change is done, so the editor never stays held.
- **A failed flush.** When a patch of that flush fails (except `NotFound`), the change does not proceed: the table keeps the previous selection (the row being edited stays selected), the pane keeps the note and its text, the failure shows as `editor/field-html` S2 says, and `release` is sent. `NotFound` proceeds as W5.4.
- **Preview** (⇧⌘P, R257) is Browse's preview of the editor's card, opened after a flush; a host without a preview disables the command (the preview's own rules: `browser/preview`; Browse's Go commands, which flush first and stop on a failed flush: `browser/inspector` I4).
- ⌘↩ in the pane flushes and keeps focus where it is; Esc flushes and moves focus to the table. Neither closes Browse. While the mask editor shows in the pane, Esc first goes to it as `editor/mask-editor` M7.1 says; an image-occlusion note opens on the mask editor inside the pane (`editor/image-occlusion-notes` O3).

## W3. Saving and closing (R820, R824, R863, R245, R259)

- Every change is saved as `editor/field-html` S1 and S2 say: per field after 600 ms, on blur, before any other action, when the host closes and when the app moves to the background (R820), as "Update Note" steps merged per note within 60 s (R821).
- **No unsaved-changes prompt** (R824): closing never asks, because closing flushes first.
- **The review's Edit Note form** keeps its changes as a draft until Done and asks before Cancel drops one (`review/edit-current#E2`, E7; decided 2026-11-20). This section's saves while typing and its no-prompt rule apply to Browse's pane only, and so do the flushes of W5.2 (before a reload) and W6 (Sync Now): the review's form never flushes. A Sync Now press while it is open syncs without the draft, which is not in the collection; a foreign change while it is open is E2's (the draft keeps its text and Done patches only the fields the user changed).
- **Exclusive phase** (KTD17): the host flushes, then closes; an editor never stays bound across a collection replacement (W5.5).
- **Cards…** opens the card template editor (U200) with the editor's current session values for its preview (R847, R938), after a flush.

## W4. Opening and closing without typing (R823, R776, AE40)

Binding the editor to a note writes nothing. A host that closes, or a pane that loads another note, with no unsaved edit sends no patch; a field the user never touched is never sent; a touched field whose final target equals its stored HTML is not sent (`editor/field-html` H2, S1); and a patch whose result equals the stored row writes nothing (`notes/note-records` U2). AE40: a Migaku note opened from review and closed at once leaves `mod` and `usn` unchanged; editing only Translation leaves Sentence's bytes unchanged.

## W5. When another writer changes the note (R822, R1793)

In an image-occlusion session the hidden Occlusion and Image fields are host fields that follow W5 like the shown fields; what a reload does to the mask editor's document and its history is `editor/image-occlusion-notes` O6.2.

The host handles the change stream in seq order, holding events while an own patch is in flight and skipping own patches (W1). For each foreign change:

### W5.1 What a foreign change asks for

| The change set lists | Asks for |
|---|---|
| N `Removed` | W5.4 |
| N `Updated`, and its stored fields, tags or note type now differ from what the editor last loaded | a reload (W5.2 or W5.3) |
| N `Updated` with only `mod`, `usn` or other columns the editor does not show changed | nothing shown changes; the editor's baseline takes the new row silently |
| N's note type, `schema_changed` true | a reload, which takes the `Stale` path of `editor/field-html` S2 for any unsaved edit, whatever the source; with `LocalApi` and an unsaved edit, see the end of W5.3 |
| N's note type, `schema_changed` false | the field options the editor shows refresh (font, size, direction and description, `editor/field-html` F1 and F2); collapsed and HTML mode follow F3, so the session's memory wins while the field count is unchanged; no field text reloads |
| only N's cards | nothing; the review panel's card is U184's |

Several foreign changes that arrive while a reload waits for its flush give one reload.

### W5.2 Every source but `LocalApi`, and `LocalApi` with no unsaved edit (R822, R1793)

Sync and CloudKit applies (`SyncIncoming`, `CloudIncoming`, `CloudRekey`), other user operations (another screen's edit, Find and Replace, Change Note Type, undo and redo, of source `UserUndoable` or `UserNonUndoable`), `Maintenance` and `Rollover`, and `LocalApi` while the editor has no unsaved edit:

1. flush (`editor/field-html` S1), and wait for the reply and its patches;
2. read N and its note type again and load them into the editor: every field unchanged with the stored HTML as its baseline, the tag list as stored (`editor/tag-editor` G11);
3. keep the focused field and, when that field's text is the same as before the reload, its caret.

Since the flush's patches list only the fields the user changed (S2), those fields keep the user's text and every other field shows the new stored value. Example: an unsaved edit in field 1 and a sync that changes field 3: field 1's patch commits first, and the reloaded editor shows the user's field 1 and the sync's field 3. A patch of the flush that fails is handled as S2 says (its text stays, the failure shows); a `Stale` refusal takes S2's reload.

### W5.3 `LocalApi` with an unsaved edit (R1793, AE80)

Decided 2026-11-10, as R1793 and AE80 ask:

- The fields without an unsaved edit reload at once with the stored values, and so does the tag list when its edit is not unsaved.
- The fields with an unsaved edit keep the user's text on screen, and their commits are held: no idle, blur or flush commit sends them while the banner shows.
- The banner "Changed by an external tool" shows with Reload and Keep mine (W9). The user can go on typing; new input in another field makes it a field with an unsaved edit too.
- **Reload** drops the held edits: every field and the tag list load the stored note as W5.2 step 2.
- **Keep mine** sends one patch with the fields that hold an unsaved edit, their session targets as values, and the tag list when its edit is held, with N's current note type and layout; every other field keeps the stored value. Its failures are S2's.
- A flush made because the editor is about to leave N while the banner shows (the app moving to the background, the host closing or loading another note, the exclusive phase) acts as Keep mine, so typed text is never dropped without the user choosing Reload. A Sync Now flush does not (W6).
- A further `LocalApi` change while the banner shows reloads the fields without an unsaved edit again and keeps the banner. A foreign change of another source while the banner shows acts as Keep mine and then as W5.2.
- A `LocalApi` change set that lists N's note type with `schema_changed` true while an edit is unsaved (W5.1): the host reloads N on the new layout as S2's `Stale` path says, matching each field with an unsaved edit to its new field, but sends no patch; the matched fields keep the user's text and are held, and the banner shows. Keep mine then sends the patch on the new layout. A field with no match is shown with its text as S2 says, banner or not.

Example (AE80): unsaved text in field 2, then `updateNoteFields` on N writes fields 1 to 3. The editor shows the API's fields 1 and 3, the user's field 2 and the banner. Keep mine stores the user's field 2; fields 1 and 3 keep the API's values. What ⌘Z then reports is `engine/undo` M7.

### W5.4 The note is deleted (R822)

When N is listed `Removed`, or a patch is refused with `NotFound`: the host stops the editor's timers, drops its unsaved edits and closes the editor without an alert. The review edit panel closes; Browse's pane shows no editor (the row leaves the table with Browse's own refresh). When unsaved text was dropped, a short notice says that the note was deleted (W9); a delete with nothing unsaved shows nothing.

### W5.5 The collection is replaced

A `CollectionReplaced` event (`engine/change-stream` S3) ends the binding: the editor closes as W5.4, without the notice. Every flush the user could lose has already run in the exclusive phase before the replacement (W3, KTD17).

## W6. Unsaved edits and sync (R1550)

iCloud is the only sync the app offers. Its automatic sends are scheduled by the engine and the policy (`sync/policy` P10), from committed changes only.

- **Automatic sends never wait for an editor.** Only committed changes reach sync tracking (`sync/policy#P8.1`): an editor's unsaved text is not in the collection, so it is not sent, it arms no flush timer (P10.2 arms on a committed local change), and it never holds back unrelated committed records (R1550). Each commit of the editor (`editor/field-html` S1: 600 ms idle, blur, flush) is an ordinary local change from then on, which the engine sends on its own schedule and the 60 s flush target bounds.
- **Sync Now flushes first.** The editor publishes whether it has an unsaved edit (W1). When the user presses Sync Now, U151 first asks every editor with an unsaved edit to flush (without `hold`, so typing goes on), waits for the results of that flush's patches, and only then hands the press to the policy as `SyncNowRequested`, so those commits come before the barrier that records S_c (`sync/policy` P12.2, at P12.3 step 2) and are selected (R2048). A patch that fails does not hold the press back: its text stays in the editor (S2) and the press goes on. Text typed after that flush is a change after the press and syncs later. Example: typing `x` and pressing Sync Now 100 ms later: the patch holding `x` commits with a seq at or below S_c, and the exchange's result counts that note.
- **Background and quit** are S1's background flush, waited for (R820, `sync/policy` P11.2, P11.4).
- While the W5.3 banner shows, Sync Now's flush sends nothing of the held fields; the held text is not in the collection, so it does not sync until the user chooses (W5.3).

## W7. Several hosts on one note

Two hosts may be bound to the same note (the review panel and Browse on a Mac). Each sees the other's patches as foreign changes and reloads as W5.2; neither prompts. The patches' field lists keep each host's typed fields.

## W8. Differences from Anki

Anki 26.09.3 differs as follows. None reaches a scenario, which observes only the collection (the field patch's core half is `editor/field-html` T3), so none has a `conformance/deviations.yaml` entry.

- **Reload.** When another operation changes the note's text, Anki's Browse editor and its Edit Current window load the stored note again without saving first: text typed since the last timer save is lost, and a field the user is in loses its caret. Meguri flushes first (W5.2), so the user's typed fields win.
- **External tools.** Anki has no source distinction: an AnkiConnect write reloads like any other change. Meguri's banner is R1793's (W5.3).
- **Deleted note.** Anki's Edit Current window closes and its Browse editor goes empty, as Meguri's do; Anki shows no notice.
- **Options-only note type changes** reload the whole editor in Anki; Meguri refreshes the options only (W5.1).
- **Closing.** Anki's Edit Current closes with Ctrl+Enter or its Close button after saving, with no prompt, as Meguri's panel does with ⌘↩, Esc and its close control. Anki's Browse saves before a row change as W2 does, but a failed save there has no recovery path.
- **Esc in Browse.** Anki's Esc anywhere in Browse, its editor included, saves the note and closes Browse; Meguri's Esc in the pane flushes and moves focus to the table, and never closes Browse (W2).
- **Keys.** Anki's Ctrl+Shift+P (preview) and Ctrl+Return are ⇧⌘P and ⌘↩ on macOS, as in Meguri.

## W9. Strings

These texts are Meguri's own, in the Editor String Catalog (`editor/editing-strings` E1's rules, typographic apostrophes); R1793 fixes the banner's words. They are the wording `editor/field-html` S2's failures use ("U196's wording"). Where each shows (banner, line, notice) and how VoiceOver announces it are the screen contracts' (`editor.edit`, `review.editCurrent`); W11 lists what those contracts need.

| Key | English | Shown when |
|---|---|---|
| `edit.externalChange.message` | Changed by an external tool | W5.3's banner |
| `edit.externalChange.reload` | Reload | its first choice |
| `edit.externalChange.keepMine` | Keep mine | its second choice |
| `edit.noteDeleted.notice` | This note was deleted, so your last changes couldn’t be saved. | W5.4, only when unsaved text was dropped; also S2's `NotFound` failure, which W5.4 handles |
| `edit.saveFailed.checkDatabase` | This note can’t be saved until you run Check Database. | S2's `InvalidArgument` failure (U128); the text stays in the editor |
| `edit.saveFailed.message` | Your changes couldn’t be saved. | every other S2 failure (`CollectionUnavailable`, `StaleGeneration`, a storage error, a panic) and a failed Keep mine; the text stays in the editor, and the next flush sends it again (S2) |
| `edit.unmatchedField.message` | “%@” is no longer in this note type. Copy its text to keep it. | S2's `Stale` reload, for each field with no match (`%@` the old field name, as stored) |

## W10. Meguri tests

`EditNoteModelTests`, `ExternalChangePolicyTests` and `EditFlowUITests` (U196) cover every row of W5.1 and every source value of `engine/op-model` O1 through W5.2 and W5.3, and:

- AE40 (W4) through the UI, with the stamps read back;
- AE80: W5.3's example, Reload and Keep mine, a flush while the banner shows, a second `LocalApi` change, and Sync Now while the banner shows (the held fields not sent);
- W5.2's example (a pending field 1 and a sync of field 3), the caret kept, and several foreign changes coalesced into one reload;
- W5.4: a sync deleting the open note closes the panel without an alert; with unsaved text, the notice;
- W1: a patch's `Changes` event delivered before its result, and after it: neither reloads the editor; a foreign change arriving while a patch is in flight is handled once the result arrives;
- W2: in Browse, typing into row A's note and pressing ↓ within 100 ms commits A's save before row B loads; a failed flush keeps row A selected; a sort change and a Change Deck on the edited row leave the editor accepting input (release sent); ⇧⌘P after a flush; in notes mode, Cards… previews the selected note's lowest-`ord` card while another row is focused;
- W5.1: an options-only note type change keeps a field the user collapsed in this session collapsed; a `LocalApi` field rename with an unsaved edit shows the banner on the new layout;
- W6: typing and pressing Sync Now within 100 ms: the typed text is in the exchange's selection; an automatic send never waits for the editor;
- W7 with two hosts on one note, each typing in another field;
- AE31 through the UI: on a "Basic (optional reversed card)" note in German::Mining, typing "x" into Add Reverse in the panel creates card 2 in German::Mining (U27's rule; this checks the editor path).

## W11. What the screen contracts need

`editor.edit` and `review.editCurrent` (`docs/design/screen-inventory.yaml`) must place, for U196: W5.3's banner and its two buttons; W9's failure line and the notice; the unmatched-field text with a Copy control; Browse's pane with no editor (no row, or more than one, W2); the states typical, overflow and error. Until both contracts name them, U196's UI work does not start (`docs/design/workflow.md`).
