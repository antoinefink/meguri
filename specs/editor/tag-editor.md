# Tag editor

Spec: editor/tag-editor
Requirements: R882, R884, R885, R886, R887, R888, R889, R890, R256; supports R829, R883, R891, R892, R893, R1959; AE37 (editor part)
Anki: 26.09.3
Scenarios: none (Meguri's tests, G14); the stored result of a tag list is `tags/tags` T4 (`conformance/scenarios/tags/canon/`)

The tag area of the note editor (U193), shared by the Add screen, the edit flows and Browse's editor: how tags are shown as chips, what each key does inside the tag being typed, how pasting, leaving a tag, selecting, copying and deleting work, and what list the editor hands to the save. The suggestions' query is `tags/complete` (C5 is the editor's request); how a saved list is normalized, spelled, deduplicated, sorted and stored is `tags/tags` T2 to T4 (R891 to R893). The commit and flush pipeline the list joins is `editor/field-html` S1 and S2. The interface text is `editor/editing-strings` E5. Layout, chip appearance and the touch gestures beyond those named here belong to the screen contracts (`editor.add`, `editor.edit`, `browse.browser`).

## G1. Terms

- **Separator glyph**: `∷` (U+2237 PROPORTION). The editor shows each `::` of a tag as one `∷` and types the separator as that one character.
- **Display form** of a tag: its name with every `::` replaced by `∷`, scanning left to right without overlap, so `a:::b` shows as `a∷:b`. **Sent form**: the display form with every `∷` replaced by `::`. A `∷` that the user types or pastes therefore always becomes `::` (G12 keeps the stored bytes of chips the user never edits).
- **Chip**: one tag of the list, shown with its display form, not being edited.
- **Tag input**: the one tag being edited, a single-line text input in place of its chip. At most one exists. It can be empty.
- **Neighbours**: the previous and next tag in list order, chips or the input.
- **Equal tags** (for R887's duplicate rule): two sent forms that are equal after `tags/tags` T2 normalization of each, compared as `tags/tags` compares tag names everywhere but T5 (`text/unicode-primitives#U2`, full case folding): `Verb` equals `verb` and `VERB`, `Straße` equals `STRASSE`, `German::Verb` equals `german::verb`. This is the save's own equality (T3, T4 step 3), so the editor never keeps two tags that the save merges into one.
- **White space** (for G7 and G5): the Unicode White_Space characters (U+3000 among them) and U+FEFF.
- **Marked text**: text an input method shows in the input while a composition is open and not yet committed (Japanese, Chinese and Korean input, dead keys, the accent menu).

## G2. Showing the list (R882)

- The chips appear in the list's order: the note's stored tags in stored order when the note loads, with tags the user adds placed where they were typed. The editor never re-sorts while the user works; the canonical spelling and order appear when the list is next loaded (G11).
- **Condensed chips.** When the chips, laid out with their full display forms, need more than two rows, every chip of a tag with more than one component shows `…∷` followed by its last component: `Lang∷German∷Verb` shows as `…∷Verb`. A tag with one component is never shortened. The full display form shows on hover (macOS help tag) and on long press (iPhone and iPad); the tag input always shows the full text; VoiceOver always reads the full name with each `∷` spoken as a level break (screen contract). The two-row test uses the full forms, so condensing that brings the list back to two rows does not turn itself off.
- An empty list shows only the Add tag control (G9) and the area that starts a new tag.

## G3. Starting to edit

- A click or tap on a chip, with no modifier, turns it into the tag input with its full display form, caret at the end, and clears any chip selection (G8).
- A click or tap in the empty space after the last chip, the Add tag control and ⇧⌘T start a new tag (G9).
- Tab from the last field enters the tag area at its last tag as G9 does, and Shift-Tab from the tag area returns to the last field (R829; `editor/field-html` F4).

## G4. Keys in the tag input (R884, R885, R886)

The **caret is at the start** when no text is selected and the caret is before the first character; **at the end** likewise after the last. "Selection" is a text selection inside the input.

**Marked text first.** While the input holds marked text, every key goes to the input method: Space, Return, `:`, Backspace, Delete, the arrows, Tab and Esc convert, commit, edit or cancel the composition as the input method decides, and none of the rows below applies. No suggestion query is sent and the suggestions shown stay as they are. When the composition is committed, its text is inserted as typed text and asks for suggestions (G6); committed text that holds a U+0020 or U+3000 is handled as a paste of the input's text (G7 steps 2 to 4), so `東京 大学` committed into an empty input gives the chip `東京` and the input `大学`. Example: with a Japanese input method, typing the reading `にほん` and pressing Space shows the candidate `日本`; Return commits it; the input then holds `日本` and only the next Space ends the tag.

| Key | Condition | Effect |
|---|---|---|
| Return | a suggestion is highlighted (G6) | accepts it: G6 |
| Return | otherwise | **ends the tag** at the selection: the text before the selection stays in this tag, the text after it becomes a new tag right after this one, the selected text is dropped, and the new tag becomes the input with the caret at its start. This tag is then left as G5 says |
| Space (U+0020, or U+3000 from an input method in full-width mode, typed without a modifier outside marked text) | a suggestion is highlighted | accepts it, as Return does (G6) |
| Space | otherwise | the same as Return without a highlighted suggestion, so Space never types a space and U+3000 never lands in a tag (R885; Anki differs, G13) |
| `:` | the text before the caret ends with `∷` | nothing is typed |
| `:` | the text before the caret ends with `:` | that colon and the typed one become one `∷`, which replaces the selection too; the caret follows the `∷` |
| `:` | the text after the selection starts with `:` | the typed colon and that colon become one `∷` in place of the selection; the caret follows the `∷` |
| `:` | otherwise | a `:` is typed |
| Backspace | the caret is at the start, and a previous tag exists | **joins** this tag to the previous one: one input holding the previous tag's text followed by this tag's text, the caret between the two parts. The joined text is the input's; G5 runs when it is left |
| Backspace | the caret is at the start of the first tag | nothing |
| Backspace | otherwise | deletes the selection or the character before the caret; `∷` is one character, so it goes at once |
| Delete (forward) | the caret is at the end, and a next tag exists | joins the next tag to this one: this tag's text followed by the next one's, the caret between them |
| Delete (forward) | the caret is at the end of the last tag | nothing |
| Delete (forward) | otherwise | deletes the selection or the character after the caret; `∷` is one character, so it goes at once |
| ← | the input is empty, and a previous tag exists | removes the empty input and edits the previous tag, caret at its end |
| ← | the caret is at the start (input not empty), and a previous tag exists | leaves this tag (G5) and edits the previous tag, caret at its end |
| → | the input is empty, and a next tag exists | removes the empty input and edits the next tag, caret at its start |
| → | the caret is at the end (input not empty) | leaves this tag (G5) and edits the next tag, caret at its start; on the last tag, starts a new empty tag after it |
| ← or → | at the first or last tag in any case not above | moves the caret only |
| ↑ ↓, Tab, Shift-Tab | suggestions are shown | G6 |
| ↑ ↓ | no suggestions are shown | move the caret to the start or the end of the input, as in any single-line text field; no tag changes |
| Tab, Shift-Tab | no suggestions are shown | leave the tag (G5) and move focus (G3, R829) |
| ⌘A | the input is empty and at least one other tag exists | leaves the input (G5) and selects every chip (G8) |
| ⌘A | otherwise | selects the input's text |
| ⌘C, ⌘X | text is selected in the input | copy, or cut, the selected text as plain text with each `∷` written `::` |
| ⌘V | | G7 |
| Esc | suggestions are shown | hides them; the text stays |
| Esc | otherwise | goes to the host (R259): the Add screen's close (`editor/add-flow` A7), the edit panel's save and close (`editor/edit-flows` W3), Browse's flush and focus to the table (`editor/edit-flows` W2) |

Typing any other text inserts it as in any text input and asks for suggestions (G6). The input has autocorrect, spell checking, auto-capitalization and smart punctuation off, as the fields' HTML mode (`editor/field-html` H6).

Examples (`|` is the caret; tags are shown in display form):

| Before | Keys | After |
|---|---|---|
| `german|` | `:` `:` | `german∷|` |
| `german∷|` | `:` | `german∷|` (nothing typed) |
| `a|:b` | `:` | `a∷|b` |
| `german∷verb∷modal|` | Space | chip `german∷verb∷modal`, then an empty input (AE37) |
| `abc|def` | Return | chip `abc`, then input `|def` |
| chips `a`, then input `|b` | Backspace | input `a|b` |
| input `a|`, then chip `b` | Delete | input `a|b` |
| chips `x`, then input `|` | ← | input `x|` |
| `ab|cd` | Delete | `ab|d` |
| typed `日本`, suggestion `日本語` highlighted, so the input shows `日本語|` | Space | chip `日本語`, then an empty input |

## G5. Leaving a tag (R888)

A tag is **left** when the input loses focus, when Return, Space or a suggestion ends it, and when an arrow key moves to another tag. Leaving applies, in order:

1. **Trim.** Remove white space at both ends. Then, while the text starts with `:` or `∷`, remove that character and the white space after it; while it ends with `:` or `∷`, remove that character and the white space before it. Inner text is untouched.
2. **Empty.** An empty result removes the tag.
3. **Duplicate.** When the result is equal (G1) to another tag of the list, this tag is removed and that other tag's chip flashes (a short highlight, which Reduce Motion replaces with a non-moving highlight).
4. Otherwise the tag becomes a chip with the result.
5. The list is then handed on (G11).

| Text when left | Result |
|---|---|
| `::foo::` | `foo` |
| ` ∷ a ∷ ` | `a` |
| `:` , `∷∷` , ` ` | removed |
| `a::` | `a` |
| `a:::b` | `a:::b` (shown `a∷:b`) |
| `Verb` while a chip `verb` exists | removed; `verb` flashes |
| `Straße` while a chip `STRASSE` exists | removed; `STRASSE` flashes |

## G6. Suggestions (R883, R884)

- While the input is not empty, each change of its text sends `tags/complete` C5's query for its sent form; C5 also gives the colon preparation, the limit of 500, the cancellation of the query in flight and that only the latest reply is shown. A reply with no tag hides the list.
- The list is ordered as C4 returns it and shows display forms. Tags already on the note stay in it (C5).
- **Highlight.** The list starts with nothing highlighted. ↓ and Tab move the highlight: from nothing to the first suggestion, then to the next, and from the last back to nothing. ↑ and Shift-Tab move it the other way: from nothing to the last, and from the first back to nothing. While a suggestion is highlighted the input shows its display form, caret at the end, without accepting it; when the highlight returns to nothing the input shows the text as the user typed it. Moving the highlight changes only what the input shows: it sends no query, and the list and its order stay. Typing (an input of the user's) removes the highlight, goes on from the text the input shows, and sends the query for the new text, so the next ↓ starts again from nothing.
- **Accepting.** Return or Space with a highlighted suggestion, or a click or tap on a suggestion, puts the suggestion's display form into the input and ends the tag once, as Return does with the caret at the end (G4): the tag is left (G5) and a new empty tag input follows it. Return with suggestions shown and none highlighted is G4's plain Return: it ends the tag with the typed text and accepts nothing.
- Example (AE37): with the registered tag `German::Verb`, typing `german`, `::`, `verb`, `::`, `modal` and pressing Space leaves the tag `german::verb::modal` and starts an empty input; the save stores `German::Verb::modal` (`tags/tags` T3).

## G7. Paste (R887)

Paste in the tag input never runs the field paste pipeline (`editor/paste`); it reads only the pasteboard's plain text.

1. The text is inserted at the caret, replacing the selection.
2. The input's whole text is split at runs of white space (G1). Each piece is trimmed as G5 step 1, empty pieces are dropped, and `::` is shown as `∷`.
3. The pieces become tags in order: every piece but the last is inserted as a chip before the input, and the last piece is the input's text, caret at its end. A piece equal (G1) to a tag already in the list, or to an earlier piece of the same paste, is dropped at once, and the chip it equals flashes. When the last piece is dropped, the input is left empty.
4. When nothing is left (only white space and separators were pasted), the input keeps the text it had before the paste.

Example: an empty input, chips none; pasting `a b` TAB `c` LF `A` gives chips `a`, `b`, `c` and an empty input; `a` flashes. Pasting `x::y  z` into the input `w|` gives chip `wx∷y` and the input `z`.

## G8. Selecting chips (R889)

- ⌘-click (iPad with a hardware keyboard: ⌘-tap) toggles one chip's selection and makes it the **anchor**. ⇧-click selects every chip from the anchor to the clicked one, inclusive, adding to the selection; with no anchor it acts as ⌘-click. A click with no modifier edits the chip (G3) and clears the selection. While ⌘ or ⇧ is held, the chips' × badges do nothing. R889's "click" is read as the click of its ⌘- and ⇧-gestures: a plain click edits, as in Anki, since otherwise no pointer gesture would edit a chip.
- Starting a selection leaves the tag input first (G5), so while chips are selected no tag input exists and the tag area itself holds keyboard focus.
- ⌘A in an empty input selects every chip (G4).
- With at least one chip selected, while focus is in the tag area:

  | Key | Effect |
  |---|---|
  | ⌘A | selects every chip |
  | ⌘C | copies the sent forms of the selected chips, in list order, joined by LF with no final LF, as plain text, then clears the selection |
  | ⌘X | copies as ⌘C, then removes them as Backspace does |
  | Backspace, Delete | remove the selected chips and hand the list on (G11); an empty tag input then takes the place of the first removed chip, with focus, so typing goes on there |
  | a printable character, ⌘V | clears the selection, starts a new tag at the end (G9) and types or pastes into it |
  | Return | clears the selection and starts a new tag at the end (G9) |
  | ←, →, ↑, ↓ | clear the selection; nothing else |
  | Esc | clears the selection; it does not reach the host (G4's Esc row) |
  | Tab, Shift-Tab | clear the selection and move focus as G4's Tab with no suggestions |

- Focus leaving the tag area clears the selection.
- On iPhone and iPad, a long press on a chip opens its context menu, whose title is the chip's full display form (so the long press also shows the full name of a condensed chip, G2) and which offers Copy, Delete and Select All; Select All selects every chip, and the same menu on a selected chip acts on the whole selection. The labels are Meguri's (screen contract).
- The × badge on a chip removes that chip alone and hands the list on.

Example: chips `a∷b`, `c`, `d`; ⌘-click `a∷b`, ⌘-click `c`, ⌘C → the pasteboard holds `a::b` LF `c`; Backspace → only the chip `d` remains, with an empty input before it and focus in it.

## G9. Adding a tag (R890, R256)

The Add tag control (`editor.tags.add`, `editor/editing-strings` E5), ⇧⌘T anywhere in the editor, a click in the space after the last chip, and Tab from the last field:

- clear the chip selection;
- when a tag input exists that is not the last tag (a chip in the middle being edited), leave it first (G5), so at most one tag input exists (G1);
- when the list is empty or its last tag is not an empty input, append an empty tag input at the end;
- put the focus in the last tag, which is that empty input.

So ⇧⌘T pressed twice gives one empty input, not two. ⇧⌘T is active while focus is anywhere in an editor page (context `editor.any`, `editor/formatting` F14) and anywhere in the Add screen (`editor/add-flow` A9); it acts on the tag area of that editor.

## G10. Collapsing the area (R890)

The tag area can be collapsed to its header. The state is remembered per screen kind (Add, Edit, Browse), on this device only (not in the collection), and is expanded until first collapsed. Collapsing while the input has focus leaves the tag first (G5). G9's commands expand a collapsed area before adding.

## G11. What the editor hands on

- The **list** is the sent forms of the tags in list order (G12 for chips never edited), the tag input included with its text as G5 step 1 would trim it, an empty input left out. Chips never hold two equal tags (G5, G7); only the input's text at a flush can equal a chip, since a flush does not run G5's duplicate step, and the save then merges the two (`tags/tags` T4 step 3).
- The list is handed on when a tag is left, when a chip or a selection is removed, and at a flush (`editor/field-html` S1). A flush does not leave the input: it hands on the trimmed text and leaves the input, its text and its caret as they are on screen. Typing alone hands nothing on.
- In Add mode the list replaces the draft's tags. In Edit and Browse modes it is the patch's tags member, sent only when it differs from the last list sent (`editor/field-html` S2). Save normalization, spelling, deduplication and sorting are `tags/tags` T4: the editor sends `german::verb::modal` and the note stores `German::Verb::modal` (AE37).
- After a patch with a tags member commits, the chips are replaced by the note's stored tags in stored order, at once when no tag input exists and otherwise when the input is next left. A chip whose sent form is not equal (G1) to any stored tag at that moment is kept after them, so nothing typed meanwhile is lost; since G1 is the save's equality, a chip the save merged into a stored tag is never kept beside it. The replaced list, as stored, becomes the last list sent, so the next flush sends no tags member unless the user changed the list again; a kept chip makes the list differ and is sent at the next flush. Example: with the registered tag `STRASSE`, the user leaves the tag `Straße`; the patch stores `STRASSE`, the chips show `STRASSE`, and the next flush sends no tags. After an add, the next draft's tags are the draft's own (`notes/add-defaults` D6), not the stored ones.

## G12. Chips the user never edits

A chip loaded from the stored note keeps its stored name, byte for byte, and is handed on with it rather than with its sent form, until its text changes: a stored tag that contains a literal `∷` keeps it. Entering the chip, by a click (G3) or an arrow key (G4), does not count: when the input is left with the same text it showed on entry, the chip goes back to its stored name. A changed text, or a join (G4), turns it into the sent form like any other. Example: the stored tag `a∷b` (a literal U+2237) shows as `a∷b`; clicking it and clicking away hands on `a∷b`, so no patch is sent; typing `c` at its end hands on `a::bc`.

## G13. Differences from Anki

Anki 26.09.3's tag editor (the one in its add, edit and browse windows) differs as follows. None reaches the collection except as a different tag list, so none has a `conformance/deviations.yaml` entry.

- **Space.** Anki types the separator for Space, with the colon rules of G4 (after `∷` nothing, after `:` the colon becomes `∷`, before `:` the two become `∷`, otherwise a `∷` is inserted). Meguri ends the tag (R885).
- **Duplicates.** Anki drops a tag only when its display form equals another exactly (case counts) and keeps `Verb` beside `verb`, or `Straße` beside `STRASSE`, until the save merges them. Meguri compares G1's equal tags (R887).
- **Paste.** Anki adds the pasted text after the input's whole text, wherever the caret is. It drops an earlier pasted piece that exactly equals a tag of the list at once, with the flash, and the last piece, which stays in the input, only when the input is left; case counts. Meguri inserts at the caret and drops every duplicate piece at once under G1 (G7).
- **Return.** In Anki, Return in the tag input both ends the tag and runs the suggestion acceptance whenever the list holds suggestions. With a suggestion highlighted, the tag is ended twice, which leaves an extra empty tag after it. With none highlighted, the acceptance stops with an error: the list stays open, and the tag can get its whole text back while the text after the caret also becomes a second tag. Meguri ends the tag once in every case (G4, G6).
- **Highlight after typing.** Anki keeps the highlighted position while the user types, so the next ↓ goes on from it; Meguri starts again from nothing (G6).
- **Forward Delete** away from the end: Anki removes a `∷` that is the second character after the caret in place of the next character; Meguri deletes the next character (G4).
- **Condensed chips** in Anki stay on for the editor page's life once the chips have needed more than two rows; Meguri tests the full layout each time (G2).
- **Untouched chips.** Anki writes every `∷` of every tag back as `::` on each tag save; Meguri keeps untouched chips as stored (G12).
- **Canonical list after a save.** Anki keeps the typed spelling on screen until the note is loaded again; Meguri shows the stored list after the save (G11).
- **Delete with selected chips** and ⌘X: Anki removes selected chips with Backspace only and has no cut; Meguri also takes Delete and ⌘X.
- **Collapse state** is stored in Anki's profile per editor kind; Meguri keeps it on the device per screen kind (G10), with the same default.
- **Keys.** On macOS, Anki's tag keys are the same as Meguri's: Anki writes them with Ctrl (Ctrl+Shift+T, Ctrl+A, Ctrl+C, Ctrl-click), and its editor maps Ctrl to ⌘ on Apple platforms. Meguri adds Delete and ⌘X on selected chips (above) and the keys of G8's table other than ⌘A, ⌘C and Backspace.
- Anki's tag options menu items (Add tag, Select all tags, Copy tags, Remove tags) correspond to G9 and G8; only "Add tag" is Anki's wording in Meguri (`editor/editing-strings` E5).

## G14. Tests

The oracle runs Anki's collection library, not its editor, so these rules are Meguri's tests (`TagEditorRuntimeTests`, `TagEditorModelTests`, `TagEditorUITests`, U193):

- every row of G4's table and its examples, on macOS and the iOS simulator with a hardware keyboard;
- G5's table, including the flash on a case-only duplicate;
- G6's highlight cycle in both directions from each end, the typed text restored at nothing, no query sent by a highlight move, acceptance by Return, Space and click (one new tag each), Return with none highlighted (one tag, the typed text), the highlight reset by typing, and a stale reply ignored after a newer keystroke (C5);
- G7's examples, a paste of only white space, and a paste whose last piece is a duplicate;
- G8's ⌘-click, ⇧-click with and without an anchor, ⌘C's text, Backspace and × with a modifier held;
- G4's marked-text rule with the Japanese (Romaji) and Pinyin input methods: Space converts and Return commits without ending the tag, no query while composing, the commit queries once, committed text with a space splits; U+3000 typed outside composition ends the tag;
- G8's selection keys table, row by row, and the focus after Backspace on a selection;
- G9's double ⇧⌘T and ⇧⌘T while a middle chip is edited (one input), G10's per-screen memory (collapse in Add, Edit stays expanded, reopening Add keeps it collapsed), G11's flush while typing (text and caret unchanged on screen), the canonical list after a commit and a tag typed during the commit kept, G11's `Straße` example (no ghost chip, no second tags member), and G12 with a stored tag holding `∷`: entered and left unchanged sends no patch, changed sends `::`;
- G2 with 40 hierarchical tags over more than two rows, the long-press menu's title on iPhone and VoiceOver's full names.

AE37's stored result is the core's (`tags/tags` T3, scenario `tags/canon/spelling`); the UI test checks the editor half end to end.
