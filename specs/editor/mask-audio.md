# Per-mask audio in image occlusion note types

Spec: editor/mask-audio
Requirements: R1018, R1019; supports R1017, R1015, R1020; AE38 (warning part)
Anki: 26.09.3 for the field text this feature reads and writes and for its effect on cards and Empty Cards (A3, A5, A8); none for the feature, which Anki does not have
Scenarios: conformance/scenarios/editor/image-occlusion/ (inactive, no golden yet; A9)

Some image-occlusion (IO) note types, such as the user's Tobira Occlusion, keep one audio field per mask (`A01` to `A34`) holding `{{cN::[sound:…]}}`, plus fields such as `Words` whose clozes belong to masks by number. Anki has no editor support for them; users write those clozes by hand, and Anki's reviewer plays them through ordinary cloze rendering (`render/image-occlusion-render#I6`). U207 shows, plays, attaches and records that audio from the mask editor and warns when a mask's number changes under it. This spec fixes what the feature reads and writes, so that the result is ordinary cloze text that Anki and every other client handle unchanged. Words are `editor/image-occlusion-strings`; placement is the screen contract's.

## A1. Terms

- **Other fields**: every field of the note type except the Occlusion and Image roles (`render/image-occlusion-format#F2`), in field order. They show in the normal field list (`editor/image-occlusion-notes` O2, R1017).
- **Mask number** N: the number of the selected mask or group, an integer of 1 or more (`render/image-occlusion-format#F1`). A selected text label, a number-0 mask or a selection of several numbers has none: the inspector then shows nothing for it.
- **References to N** in a field: the clozes of the field's cloze tree (`render/cloze#C4`), nested ones included, whose number list contains N. The field **references** N when it has at least one; that is the same as N being one of the field's cloze numbers (`render/cloze#C5`). Fields are read as their session targets (`editor/field-html` S1), so the inspector follows what the user has typed, saved or not.

## A2. Linked content (R1018)

For the selected mask number N, the inspector lists each other field that references N, in field order, with that field's **entries** for N: the back-side entries of `render/cloze#C9` for card N (the plain content of each cloze active on card N, in document order). An entry is shown as the field's HTML would be in the field list, with each sound tag of it (`render/av-tags#A3`) shown as a play control for its file. A field that does not reference N is not listed.

| Field | Text | Entries for N = 3 |
|---|---|---|
| `A03` | `{{c3::[sound:kumo.mp3]}}` | `[sound:kumo.mp3]` (one play control) |
| `Words` | `{{c1::空}}・{{c3::雲}}・{{c4::雨}}` | `雲` |
| `Words` | `{{c2,3::山}}` | `山` |
| `A04` | `{{c4::[sound:ame.mp3]}}` | not listed |

Play uses U67's player for that one file, as the review's replay does (`render/audio-playback`). A file that is not in the media folder shows the play control disabled.

## A3. The audio field of a mask

The **audio field** of N is the first other field, in field order, whose name is the letter `A` (either case) followed by one or more ASCII digits whose decimal value is N, with nothing before or after: `A03`, `A3`, `A003` and `a3` match 3; `A30`, `A03b`, `Audio3`, `Notes3`, `3`, `A 3`, `A-3` and `Ａ３` (full-width) do not. Decided 2026-11-10: R1018 names "the matching A{NN} field", and the plan's broader "letter prefix" would also take fields such as `Notes2` or `Extra1` that hold no audio; whether other prefixes should count is open for Antoine's decision. The rule reads only names, never field descriptions or tags. With no audio field for N, Attach Audio and Record Audio are not offered for that mask; the linked content still shows.

## A4. Attach and record (R1018)

- **The text written.** With NAME the stored name, the new cloze is `{{c` N `::[sound:` NAME `]}}`, N in decimal without leading zeros (`{{c3::` for the field `A03`), and NAME as P3's audio snippet writes it in field HTML (`&` as `&amp;`).
- **Where.** Let C be the top-level clozes of the audio field (`render/cloze#C4`) whose number list is exactly N.
  1. When C is empty (the field does not reference N, or N appears only in a cloze with several numbers or in a nested cloze), the new cloze is appended to the field's text, with nothing between: an empty field becomes the cloze alone; `{{c2::[sound:b.mp3]}}` becomes `{{c2::[sound:b.mp3]}}{{c3::[sound:c.mp3]}}`; the other clozes stay.
  2. When C is not empty, the source of C's first cloze (from the first `{` of its opening marker to the last `}` of its closing marker) is replaced with the new cloze and nothing else changes. That is a replacement, which Meguri confirms first (R1018).
- **Steps**, in this order:
  1. **Ask** (only for a replacement): `io.audio.replace.confirm` names N and the audio field, with Replace and Cancel. Cancel ends the action: nothing is added, nothing is written.
  2. **Choose or record.** Attach Audio opens the system file importer for one file, limited to the audio and video types of `editor/paste` P2's media extensions. Record Audio opens `editor/paste` P20's recorder (its format, `rec<seconds>.m4a` name, permission, states, Save and Cancel, and P21's words); its enabling and target are this spec's, not P20's. Cancelling the importer or the recorder ends the action with nothing added or written.
  3. **Add** the file through P4's importer, with P4's warnings and refusals; a cancelled or refused add ends the action with nothing written. The add is written at once, as P4 says. A stored name whose extension is not an audio or video extension (P2) is not written: the action ends with `io.audio.notAudio` (the file stays in media until Check Media reports it unused).
  4. **Flush and write.** The host makes a held flush of the page (`editor/field-html` S1, B3 `hold`) and waits for it, so a pending commit of the audio field comes first. It then computes the new text from the audio field's session target as it now is, by the Where rule above. The new text becomes the field's session target: in Edit mode the host submits one field patch with that field (S2, its last sent value and failure handling; one "Update Note" step, merged as S2 says), in Add mode it replaces the draft value. The host sends `setField` with the new text, so the page shows it as the field's baseline, and then `release`.
  5. **Play.** The new audio plays once, as a new audio snippet does (`editor/paste` P3, R851).
- **Enabled.** Attach Audio and Record Audio are enabled while the inspector shows a mask number with an audio field (A3), whatever has keyboard focus, and disabled as A6 says. The target is the audio field and the note of the session as they were when the action started; when at step 4 the session no longer holds that note, or its note type no longer has that field, the file stays added and nothing is written (as P20's Target rule keeps a take).
- Examples, N = 3, field `A03`:

  | Before | After |
  |---|---|
  | (empty) | `{{c3::[sound:rec1793102400.m4a]}}` |
  | `{{c3::[sound:old.mp3]}}` | asks; `{{c3::[sound:new.mp3]}}` |
  | `<div>{{c3::[sound:old.mp3]}}</div>` | asks; `<div>{{c3::[sound:new.mp3]}}</div>` |
  | `{{c3::[sound:a.mp3]}} {{c3::[sound:b.mp3]}}` | asks; `{{c3::[sound:new.mp3]}} {{c3::[sound:b.mp3]}}` |
  | `{{c3,4::[sound:both.mp3]}}` | `{{c3,4::[sound:both.mp3]}}{{c3::[sound:new.mp3]}}` (nothing replaced, so no question) |
  | `{{c30::[sound:x.mp3]}}` | `{{c30::[sound:x.mp3]}}{{c3::[sound:new.mp3]}}` (30 is not 3) |

## A5. What the clozes do to cards

These are Anki's rules, which Meguri shares; the feature depends on them:

- A cloze number in any field of the note makes a card (`notes/card-generation` G4; R1017): an audio cloze for a number that no mask has still creates its card. A4 is offered only for a selected mask, so the number has a mask when the audio is written.
- A card stays required, and Empty Cards does not remove it, while any field of the note holds a cloze with its number (`notes/empty-cards` E1). Deleting mask N leaves card N in place while the audio field still holds `{{cN::…}}` (AE38: card 6 still exists).
- On the card, the audio of N plays on the back only, inside the hidden element (`render/image-occlusion-render#I6`).

## A6. Hidden actions and the inspector's states

- No mask selected, or a selection without a mask number (A1): the inspector says so and offers nothing.
- A mask number with no audio field (A3): the linked content only (A2), possibly empty (the stock IO type has no such fields; U207 test scenario).
- Attach and record are disabled while a label is being edited, while a held flush holds the session (`editor/image-occlusion-notes` O6.1), while W5.3's banner holds the audio field (`editor/edit-flows` W5.3), and during an Add screen's running add (`editor/add-flow` A5 step 1).

## A7. Warnings when a number leaves a mask (R1019; AE38)

- A completed change of the mask editor (`editor/mask-editor` M1) **orphans** a number m when m had at least one mask before the change and none after it: deleting the last mask of m (`editor/mask-editor` M4.1), or a Group whose members of number m take the group's smaller number (`render/image-occlusion-format#F12`).
- It **renumbers** a mask from m to k when a mask that had number m before the change has number k ≠ m after it while m keeps other masks: an Ungroup, whose members after the first take new numbers (F12). Example: a group of two masks numbered 5 is ungrouped; the second becomes 6 while A05 still holds `{{c5::…}}`. Only the user knows which of the former members A05's audio was for, so the warning names both numbers.
- Moves, resizes, rotations, fills, label edits, duplicates, mode changes, Undo and Redo neither orphan nor renumber anything here (Undo and Redo restore a state the user has seen, `editor/mask-editor` M8).
- When a change orphans m or renumbers a mask from m, and at least one other field references m (A1), the editor warns. One warning per change, with one line per such number m in ascending order: `io.audio.orphaned` (m, the fields) or `io.audio.renumbered` (m, the new numbers in ascending order, the fields), the fields being those that reference m, in field order. In Edit mode, an orphaned m whose card exists or will exist (`editor/image-occlusion-notes` O9) adds `io.audio.cardStays` with min(m, 500): that card stays while those fields use the number (A5). The change itself is made; the warning offers no undo of its own (the mask editor's Undo restores the mask, `editor/mask-editor` M8).
- Nothing rewrites the other fields' clozes: renumbering them is the user's choice (by hand or with Find and Replace).
- AE38: masks c1 to c7, audio clozes c1 to c7 in A01 to A07. Deleting mask c6 warns, naming A06, with the line that card 6 stays. Grouping masks 4 and 5 (both become 4) warns, naming A05. Moving c4 warns about nothing.

## A8. Adding notes with audio

The Add screen's field check (`editor/image-occlusion-notes` O7 step 2; `notes/field-check` K3) refuses a draft in which a field that the note type's first front does not use with `cloze:` holds a cloze: an audio field that the template does not render with `{{cloze:A03}}` makes `field_not_cloze`, as in Anki, whose add window refuses it the same way. In Edit mode the same check runs after each commit and shows `field_not_cloze`'s hint (`editor/field-html` S3, R812), but nothing refuses the save. Meguri shows A5's line from `editor/add-flow` and nothing more; the inspector does not check the template.

## A9. Scenarios

`editor/image-occlusion/audio-fields` (inactive, no golden yet): a clone of the stock IO type with fields `Words`, `A01`, `A02`, `A03` and a front that renders `A01` and `A02` but not `A03`. It observes that a note whose Occlusion field has masks 1 to 3 and whose A01 and A03 hold audio clozes gets cards of ordinals 0 to 2 (A5); that the field check gives `field_not_cloze` with A03's audio and `normal` without it (A8); that a draft whose Occlusion field holds only a label but whose A01 holds an audio cloze is `normal` (Meguri's R994 check refuses it first); that after masks 2 and 3 are removed from the Occlusion field, Empty Cards lists the card of ordinal 1 only (A03 still holds c3, A5, A7); and that after A03 is emptied it lists ordinals 1 and 2.

## A10. Tests

`MaskFieldLinksTests`, `MaskAudioWriterTests` and `MaskAudioUITests` (U207), with expected values from this spec: A2's table; A3's matching names and non-matches, and the first-match rule with both `A3` and `A03` present; A4's example table, the question only for a replacement, Cancel at the question (nothing added, field unchanged), a cancelled importer, a non-audio stored name refused, the `&amp;` of a name with `&`, a recording into an empty `A12` giving `{{c12::[sound:rec<seconds>.m4a]}}`, and a write while A12 has a pending 600 ms commit (the typed text kept, the cloze appended after it); A6's states on the stock IO type; A7's AE38 cases, the ungroup of a 5-5 group with A05 referencing 5 (`io.audio.renumbered` with 5, 6 and A05), an Undo after a delete (no warning), and a deletion with no referencing field (no warning; O9's notice instead).
