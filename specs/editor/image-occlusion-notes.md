# Image occlusion notes: picking the image, adding, editing, changing the image

Spec: editor/image-occlusion-notes
Requirements: R991, R992, R993, R994, R1013 (screen side), R1014, R1015 (notice), R1016 (screen side); supports R990, R913, R1012, R1017; AE33 (image-occlusion part), AE38 (mask part)
Anki: 26.09.3 for the field writes, the image naming and the add check that O4 to O7 match; none for the picker's sources, Change Image's kept masks (R1014) and the deletion notice, which are Meguri's
Scenarios: conformance/scenarios/editor/image-occlusion/ (inactive, no golden yet; O12)

How U206 turns the Add and Edit flows into image-occlusion (IO) flows: which fields the editor hides and how the session writes them, how the image is chosen and stored, what Add checks and what it leaves for the next note, how an existing note opens, what Change Image does, and the notice for a deleted mask. The mask editor itself is `editor/mask-editor`; the field format, the strict reading, the writer and the numbers are `render/image-occlusion-format`; the Add screen around it is `editor/add-flow`; the field editor's commits and patches are `editor/field-html`; foreign changes are `editor/edit-flows` W5; per-mask audio is `editor/mask-audio`; the words are `editor/image-occlusion-strings`.

## O1. Terms

- **IO note type**: a note type that `render/image-occlusion-format#F2` detects: `original_stock_kind` 6 and field roles (R990). Its **roles** are F2's Occlusion, Image, Header and Back Extra fields.
- **IO session**: the editor session (U190) of a note of an IO note type, in Add mode (a draft) or Edit mode (a stored note, in Edit Current, Browse's editor pane or the review edit panel), with the mask editor's document (`editor/mask-editor` M1).
- **Image name**: `render/image-occlusion-format#F13`'s name found in the Image field.
- **Usable image**: an image name whose extension (after the last `.`, ASCII case-insensitive) is one of `jpg`, `jpeg`, `png`, `gif`, `svg`, `webp`, `ico`, `avif`, naming a file that exists in the media folder. Anki opens an IO note's image only in that case (F13), so every image Meguri stores for an IO note has one of these extensions (O4). A usable image is **shown** when the system decodes it and reports a pixel size (an SVG: the size its `width` and `height` give, or else its `viewBox`); the mask editor needs that size for its reference size (`editor/mask-editor` M1).

## O2. Which note types get the IO flow (R990)

- Meguri's IO flow runs for IO note types only (F2's detection). The fields the field list leaves out are `editor/field-html` F1's: for an IO note type with all four tags the Occlusion and Image roles, and otherwise fields 0 and 1, which are then also F2's roles. Every other field (Header, Back Extra, Comments and any extra field such as A01 to A34 or Words, R1017) shows in the normal field list, in the note type's field order.
- A note type with `original_stock_kind` 6 and fewer than four fields has no roles and gets no IO flow: no mask editor and no toggle. Its notes open in the normal field editor, which leaves field 0 out as `editor/field-html` F1 says (decided 2026-10-27 there), and shows `io.editor.noRoles` above the fields. Anki's editors show the mask editor for any note type of stock kind 6 and then fail on such a note type with a request to check the database; Check Database restores the tags (`maintenance/check-database-notes`), not the missing fields.
- Within an IO session only the mask editor writes the Occlusion field and only O4.2 and O8 write the Image field; a note type change maps them as O10 says.

## O3. The Mask Editor toggle (R993)

- An IO session has two views: the mask editor and the fields (the normal field list and tags). The mask editor view shows the picker (O4) while an Add draft has no image chosen yet (a new draft, O7 step 4, a change into an IO note type, O10), the canvas once it has one, and O5's states for a stored note or a Create Copy whose image or Occlusion field cannot be used. The toggle `Mask Editor` (⌥⇧⌘M, `editor/mask-editor` M4) switches between the two views; Anki names it "Toggle Mask Editor" with the same keys.
- **Which view opens.** The Add screen and the review edit panel open an IO session on the mask editor, as Anki does each time it loads an IO note. Decided 2026-11-10: Browse's editor pane opens it on the mask editor too (R1013), inside the pane, so that the table and its row navigation stay available while a row change loads another note; showing the canvas larger is a request the screen contract places (`browse.browser`, `imageOcclusion.editor`), and it never covers the table while a row change can load another note. A row change flushes the session as any editor load does (`editor/field-html` S1).
- Switching views keeps the document, its undo history and the zoom (`editor/mask-editor` M8); it writes nothing, except that it ends a label edit (`editor/mask-editor` M3.4) and discards an unfinished polygon (M3.3).
- The other fields keep their normal editor behavior in the fields view: sticky pins in Add mode (`editor/add-flow` A4), field checks, formatting and media.

## O4. Choosing the image (R991, R992)

### O4.1 Sources

The picker offers: Photo Library and Take Photo (iPhone and iPad; Take Photo only on a device with a camera, as `editor/paste` P5 offers them), Choose File, Paste Image (also ⌘V while the picker has focus), and dropping an image onto the picker (R991). Anki desktop offers a file chooser, Paste Image from Clipboard and dropping onto its picker; the photo sources are Meguri's. Each source gives exactly one image: an IO note has one.

| Source | What is added to media (`editor/paste` P4: one importer, `media/media-files` M3's naming) |
|---|---|
| Choose File (the system file importer, image types only, one file), or a file dropped | a file with a usable-image extension (O1): its own bytes, desired name its last path component (P4). Any other image file the system can decode (HEIC, TIFF, BMP, …): decoded with its orientation applied and encoded by P7 (`paste-<sha1>.jpg`, or `.png` when `pasteImagesAsPng` is `true`), so that the note stays openable in Anki (O1). Decided 2026-11-10. A file that is not an image: nothing is added and the picker shows `io.picker.notImage` |
| Photo Library (the system photo picker, one image), Take Photo (one still photo) | `editor/paste` P6 (orientation, the image max size setting, P7's encoding and `paste-<sha1>` name) |
| Paste Image | the pasteboard's first usable item, in this order, as Anki tries them: (1) a file URL: as Choose File's file (a HEIC file URL is converted too); (2) a web URL whose text, lowercased, ends with `.` and a usable-image extension: fetched by `editor/paste` P10.2's rules and added with P10.3's name; (3) image data, encoded and named by P7. When none is there: nothing is added and the picker shows `io.picker.noImageOnPasteboard` (Anki: "No image found on clipboard.") |
| image dropped from another app | as Paste Image's three steps, on the dropped items |

- **Fetching.** While a web URL is fetched, the picker shows P10.2's progress with Cancel (`editor/paste` P21's "Getting Media…") and takes no other source. Cancel stops it and adds nothing. A failure shows P10.2's alert and adds nothing; there is no link fallback, since the picker has no field to put one in.
- **Not shown.** Before anything is added, the image is decoded. One that cannot be decoded or measured (O1, a damaged file, an SVG the system cannot render) is not added, and the picker shows `io.picker.unreadable`.
- The add is written at once (P4 "Written at once"): an image chosen and then abandoned stays in media until Check Media reports it unused.
- P4's size warning and refusals apply; a refused or cancelled add leaves the picker as it was.

### O4.2 The Image field

- The Image field (its role) becomes exactly the P3 image snippet of the stored name, `<img src="NAME">`, replacing its whole content; the stored field holds the raw name (P3): `diagram.png` → `<img src="diagram.png">`, `a&b c.png` → `<img src="a&b c.png">`. This is what Anki's editor writes, and the same snippet as its other image insertions.
- It is written at once as a host-field change of the Image field (O6.1), before any mask exists; Anki also commits it at once.
- The mask editor then opens on the image (R992) with Rectangle (`editor/mask-editor` M2), and the reference size is set from the image's pixel size and the editor's area (`editor/mask-editor` M1).

## O5. Opening an existing note (R1013, R1016)

When an IO session loads a stored note (Edit Current, Browse's editor pane, review edit), a Create Copy draft (`editor/add-flow` A8) or a draft changed from another IO note type (O10):

1. **Image.** The image name is read from the Image field (F13). When it is not a shown usable image (O1), the mask editor shows, in place of the canvas, `io.editor.noImage` (no image name), `io.editor.imageMissing` (no such file in the media folder) or `io.editor.imageUnusable` (another extension, or an image that cannot be decoded), with Change Image (O8) and the toggle to the fields. Step 2 still runs, so that Change Image keeps the shapes (O8); with no image there is no reference size, so nothing can be drawn and nothing is written until an image is set. Anki shows an empty editor in that case.
2. **Shapes.** The strict reading of the Occlusion field (`render/image-occlusion-format#F7`). On success the document holds its shapes in field order, with their numbers and sources; masks that share a number are one group, text labels are separate (F7.4); the mode is the field's (F5). The tool is Select (M2), or Rectangle for an Add-mode draft.
3. **Reading error** (R1016). The canvas does not open and nothing is written: the Occlusion field keeps its text byte for byte.
   - **Edit mode.** The session shows the fields view with `io.editor.unreadable` (or `io.editor.unreadable.noShape`) naming the error kind and shape index (F7.2), and the toggle is disabled until the field reads again (O6.2). Only the shown fields can be edited (the hidden roles stay hidden, `editor/field-html` F1). Anki's editor loads such fields leniently instead and rewrites them on the next change (`render/image-occlusion-format#F15`).
   - **Add mode** (a Create Copy or a change from another IO note type). The mask editor view shows `io.editor.unreadable.copy` naming the error with Draw New Masks (`io.editor.drawNewMasks`). Draw New Masks starts an empty document (no shapes, no sources, history empty) on the draft's image, or the picker when the draft has no shown usable image; the first completed change then writes the writer's output for that document, which replaces the unreadable text in the draft. Until then O7 step 1 refuses the add (nothing has been drawn), so the user is never left with a hidden field that blocks every add.

Opening and closing with no completed change writes nothing and runs no update (R1012; F11 rule 1; `editor/field-html` S2's no-op patch).

## O6. Saving (R1012)

### O6.1 The hidden fields are host fields

The Occlusion and Image roles are never loaded into the page (`editor/field-html` F1). Decided 2026-11-10: they are **host fields** of the session, written as follows; everything else `editor/field-html` S1 and S2 say about a field applies to them unchanged.

- A host-field change is the mask editor's output (`editor/mask-editor` M9) for the Occlusion field, or O4.2's snippet for the Image field. It is that field's commit: its value is the field's session target as it stands (there is no page HTML to serialize, so S1's computation of a target from a page commit is skipped).
- **Edit mode**: the target is compared with the field's last sent value (S1). When they differ the host submits at once one field patch (S2) with that field alone; its last sent value, last committed value, failure handling, the resend at the next flush, `Stale`'s reload (the field matched by its `id`, or its name) and the merge of consecutive patches into one "Update Note" step (R821) are S2's. A patch that fails shows S2's message; the document stays as drawn, since it is the field's session target.
- **Add mode**: the target replaces the field's value in the draft (S1).
- **Flush**: a flush, held or not, includes every host field whose session target differs from its last sent value. While a held flush holds the page (B3 `hold`), the mask editor and the picker take no input either, until `release` or the next load.
- Untouched shapes keep their source text byte for byte, changed and new ones are written by F10 (R1012, AE38), and a save that ends with the document read writes the original text (F11 rule 1), which S1's comparison then sends as no change.
- The Header, Back Extra and other fields are saved by the field editor as usual. Anki's editors do the same; its one-call IO add and update, which wrap Header and Back Extra in `<div>` and record the undo step "Image Occlusion", belong to its standalone page, which Meguri does not reproduce.

### O6.2 Changes made elsewhere

The host fields follow `editor/edit-flows` W5 like the shown fields: after W5.2's flush and reload, or W5.3's Reload or Keep mine, or S2's `Stale` reload, each host field takes the value the reload gives it.

- When the Occlusion field's new value differs from the text the document was last read from or written to, the document is rebuilt from it by O5 step 2 (or step 3 on a reading error): the selection and the undo history are cleared, an unfinished polygon and a label edit end, the tool and the zoom stay. When it is the same, the document and its history stay.
- When the Image field's new value names another image, O5 step 1 runs again and the reference size is set again (`editor/mask-editor` M1); the document is kept as for Change Image (O8).
- While W5.3's banner holds the Occlusion field (its session target differs from its last committed value), the mask editor keeps drawing on the held document and its writes stay held, like typing in a held field; Keep mine sends them, Reload rebuilds the document from the stored value.

## O7. Adding (R994; AE33)

Add (`editor/add-flow` A5) on an IO draft, after the flush:

1. **Occlusion check** (R994). The Occlusion text the add would store is the draft's Occlusion value, the writer's last output for the document (`editor/mask-editor` M9), or the read text while nothing has changed. When it holds no mask with a number of 1 or more (no shape; only text labels; only number-0 masks; or only shapes the writer leaves out, such as an empty label), Add is refused with `io.add.noOcclusions` ("No occlusions created"): nothing is written, the mask editor shows the line, `release` is sent. An Add-mode draft whose Occlusion field cannot be read (O5 step 3) is refused the same way until masks are drawn.
2. **Field check.** Otherwise A5's field check and its table apply to the draft. With the stock templates, a draft that passes step 1 is `normal` or `duplicate`, except: `empty` when the note type's first field is not its Occlusion field and is empty (a Header moved to position 0); `field_not_cloze` when a field that the first template's front does not use with `cloze:` holds a cloze (for example `{{c1::x}}` typed in Header, or an audio cloze in an extra field the front does not use, `editor/mask-audio` A8). Those states refuse with A5's lines. `missing_cloze` reaches A5's confirmation only when the note type's front does not use the Occlusion field with `cloze:`. (Scenario `editor/image-occlusion/add-check`.)
3. **The add** is A5's: the note's fields as the draft holds them, the selected deck, one step "Add Note"; the cards are those of `notes/card-generation` G4 (AE33: two masks and one label give two cards), and the toast counts them.
4. **The next draft** (`notes/add-defaults` D6 with these additions). Decided 2026-11-10: the Occlusion and Image fields are empty whatever their sticky options, the mask editor returns to the picker, and its document and undo history are cleared; the other fields follow D6 (sticky values kept, others emptied, tags kept); the mode and translucency carry over (`editor/mask-editor` M2). Anki means to make the same reset after an add, but its add window copies sticky fields from the added note before its editor page clears the canvas, so a sticky Occlusion or Image field can survive into its next note.

Anki's add runs only the field check: an empty Occlusion field (or empty first field) is refused with "Unable to add. Either you have not added any occlusions, or the first field is empty.", and a draft with only text labels is `missing_cloze`, which asks to proceed and then adds a note with one card of ordinal 0 (`notes/card-generation` G6). Meguri refuses that draft (R994).

## O8. Change Image (R993, R1014)

- Change Image is available in the mask editor once a note has an image, and in O5 step 1's states. It asks first (`io.changeImage.confirm`, buttons Change Image and Cancel). Cancel changes nothing. Change Image shows the picker (O4.1).
- When a new image is chosen: the Image field is set by O4.2, as one host-field change. The document keeps every shape with its fractions, numbers and sources unchanged (R1014), so on a new image of another size or shape the masks keep their relative positions. The reference size is set from the new image (`editor/mask-editor` M1); the undo history is cleared; the tool stays. Since the shapes are unchanged, the Occlusion field is not rewritten (F11 rule 1): its text stays byte for byte.
- Cancelling the picker after confirming keeps the old image and the document.
- The old image file is not removed from media; Check Media later reports it unused when no note uses it.
- Anki's corresponding control, "Reset Image Occlusion" ("Are you sure you want to reset this image occlusion?"), clears every mask and returns to its picker in both modes. In its Add window it also empties the Image field. When editing a note, clearing the masks writes an empty Occlusion field, which the note's normal save keeps; the Image field stays until a new image replaces it, and the editor then loads the masks the stored note now holds. Meguri keeps the masks in both modes (R1014).

## O9. Deleting a numbered mask (R1015; AE38)

- Removing the last mask of a number N ≥ 1 from the document (Delete, or a Group that gives N's masks the group's smaller number, `editor/mask-audio` A7) never deletes a card. In Edit mode, when the note has, or will have, a card of ordinal min(N − 1, 499), that card stays: it no longer has a mask to guess (in Hide All mode it still shows the other masks), and Empty Cards removes it once no field of the note holds a cloze with number N (`notes/empty-cards` E1; `notes/card-generation` G4). The note will have it when this session has sent an Occlusion value holding a mask numbered N, whose patch generates the card (`notes/note-records` U3).
- The first time this happens in an IO session for a number that no other field references (`editor/mask-audio` A1), the mask editor shows `io.delete.cardRemains` with min(N, 500), the card's number as the user sees it. It does not show again in that session. In Add mode (no cards yet) it does not show. Undo and Redo never show it (`editor/mask-editor` M8).
- When another field of the note still holds a cloze with number N, `editor/mask-audio` A7's warning shows instead, with its card line in Edit mode, since Empty Cards will not remove that card; it does not use up this notice.
- A later new mask takes the lowest unused number (`render/image-occlusion-format#F12`), which may be N again; it then uses card N.

## O10. Note type changes and the stock type on demand (R913)

- **Choosing Image Occlusion in the Add screen's note type chooser** when the collection has no IO note type (`notetypes/records` N9's test): the chooser lists one more row, labelled with the name N9 would store (`Image Occlusion`, or `Image Occlusion+` when another note type has that name), that stands for the missing type. It sorts and filters with the stored names by that label (`editor/add-flow` A2). Choosing it ensures the type (N9: added as the stock kind, `curModel` unchanged) and then selects it as `editor/add-flow` A3 says. Nothing is written until it is chosen. When an IO note type exists, the chooser lists the stored note types only.
- **Changing the draft's note type to an IO note type from another kind** (A3). Decided 2026-11-10: A3.1's mapping runs with the IO type's fields other than its Occlusion and Image roles as the new fields; those two start empty. The picker shows, and the document starts empty. Example: Basic Front `a`, Back `b` → Image Occlusion Header `a`, Back Extra `b`, Occlusion and Image empty. Anki maps into the hidden fields as well (Front `a` would become its Occlusion text) and shows its picker.
- **Changing from one IO note type to another.** The Occlusion and Image texts go to the new type's Occlusion and Image roles byte for byte; A3.1 maps the other fields onto the new type's other fields. Then O5 runs on the draft as for a Create Copy (Rectangle; O5 step 3's Add-mode state on a reading error).
- **Changing away from an IO note type** to one that is not an IO note type: A3.1's mapping over every field, the Occlusion and Image texts included, byte for byte, as Anki does. The mask editor closes.

## O11. Differences from Anki

The oracle runs Anki's collection library, not its editors, so none of these is in `conformance/deviations.yaml`; Meguri's tests (O13) assert them.

| Item | Anki 26.09.3 | Meguri |
|---|---|---|
| Stock kind 6 with fewer than 4 fields (O2) | mask editor, then an error | normal field editor with field 0 left out, and a notice |
| Image types (O4.1) | any file with a usable-image extension; other types are not offered | also converts other decodable images to P7's JPEG or PNG |
| Photo Library, Take Photo | none on desktop | P6, one image |
| A web URL on the pasteboard that fails | (its fetch error) | P10.2's alert; nothing added |
| A note whose image is missing (O5) | an empty editor | a message with Change Image |
| Unreadable Occlusion field (O5, R1016) | loaded leniently and rewritten on the next change | Edit: not opened, the field byte-identical; Add: Draw New Masks |
| Labels-only Add (O7) | asks to add without a cloze, then adds one card | refused, "No occlusions created" (R994) |
| Sticky Occlusion or Image after an add (O7) | may carry over | always emptied |
| Change Image (O8) | clears the masks in both modes | keeps the masks with their fractions (R1014) |
| Deleted mask (O9) | no message | a one-time notice |
| Change into an IO type (O10) | maps text into the hidden fields | maps onto the shown fields; between IO types, the roles map to the roles |
| Browse's editor (O3) | the mask editor fills Browse's editor area | the mask editor in the editor pane; the table stays available |

## O12. Scenarios

Inactive, with no golden yet (not in `conformance/scenarios/editor/area.yaml`), using only existing ops and observations.

| Scenario | Covers |
|---|---|
| `editor/image-occlusion/add-check` | O7 step 2 and Anki's check: on the stock IO type (`ensure_io_notetype`), masks give `normal`, labels only `missing_cloze`, an empty Occlusion field `empty`, a cloze in Header `field_not_cloze`; on a clone whose Header is moved to position 0, an empty Header gives `empty` even with masks; then an add of two masks and a label gives cards of ordinals 0 and 1 (AE33) |
| `editor/image-occlusion/audio-fields` | `editor/mask-audio` A8 and O9: extra fields' clozes make cards, an audio field the front does not use is `field_not_cloze`, and Empty Cards keeps the card of a deleted mask while an audio field still holds its number |

## O13. Tests

`IOSaveAdapterTests`, `IOFieldRolesTests` and `IOAddEditUITests` (U206) take expected values from this spec and from `render/image-occlusion-format`: O2's detection with three, four and five fields and with stock kind 1, and the three-field note type opening with field 0 left out and `io.editor.noRoles`; O3's toggle and ⌥⇧⌘M, the Add screen and Browse's pane opening on the mask editor, and a Browse row change flushing a mask edit; O4.1's sources, including a pasted bitmap stored as `paste-<sha1>.jpg`, a HEIC file and a HEIC file URL stored as `paste-<sha1>.jpg`, Photo Library limited to one image, a damaged PNG refused with nothing added, and a pasted web URL cancelled; O4.2's Image field text; O5's outcomes (each image state, a reading error in Edit mode and in a Create Copy with Draw New Masks) and the untouched open and close (no update op, field byte-identical); O6.1's single patch per change, in Add mode two masks drawn, then an immediate switch to the fields view and a keystroke in Header, then Add: the added note's Occlusion field holds both masks (M9, O6.1), a failed patch resent at the next flush, and a held flush refusing mask input; O6.2's rebuild after a sync changes the Occlusion field and the kept history after a sync that changes only Header; O7's refusal for labels only, for no shape and for an empty label alone, the `field_not_cloze` refusal, the two-card add of AE33, and the next draft's empty Occlusion and Image with a sticky Header and a sticky Occlusion field; O8's kept fractions on a twice-as-wide image, and Cancel at the question and at the picker; O9's notice once per session, with min(N, 500) for N = 600, its absence in Add mode and after an Undo; O10's on-demand type created once (a second choice adds none), Basic to Image Occlusion (Header `a`, Back Extra `b`), and one IO type to another (masks kept). The plan's AE38 case ("the deletion notice appeared" on a Tobira note whose A06 holds c6) is met by `editor/mask-audio` A7's warning with its card line once U207 is in, and by O9's notice before.
