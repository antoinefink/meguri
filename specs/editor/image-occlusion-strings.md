# Image occlusion strings

Spec: editor/image-occlusion-strings
Requirements: R993, R994, R995, R996, R1000, R1015, R1016, R1018, R1019, R195; supports R991, R277
Anki: 26.09.3 for the Anki column (its English, for comparison only); none for Meguri's wording
Scenarios: none (interface text; S9)

The interface text of the image-occlusion screens (U205, U206, U207): the mask editor's tools and commands, the image picker, the Mask Editor toggle, Change Image, the add refusal, the notices, the per-mask audio inspector and the accessibility text of the canvas. The mode names are `editor/editing-strings` E9's (`io.mode.hideAll`, `io.mode.hideOne`) and are not repeated here. Words that a Product Contract requirement quotes are kept exactly; the rest is Meguri's own wording for the platform (the design objective in `docs/design/native-interface-contract.md`). The Anki column gives Anki desktop's English where it has an equivalent; it is never shown.

## S1. Rules

- Every string is an entry of the Image Occlusion String Catalog (`app/Packages/MeguriKit/Sources/Features/ImageOcclusion/Resources/Localizable.xcstrings`), the catalog of `editor/editing-strings` E9's mode names, with the key given here. Only that feature's views read these keys. Keys never change once shipped; a changed meaning gets a new key. English is the value given; other languages are Meguri's own translations (R195).
- Arguments follow `editor/editing-strings` E1: `%lld` is an integer, `%@` a text; an entry with two or more arguments numbers them (`%1$lld`, `%2$@`), so a translation can reorder them. A plural entry has the catalog's plural variations on the integer argument the row names.
- A list argument is the items joined with the system's list format (`ListFormatter`, "A05", "A05 and A06", "A05, A06, and A07" in English): field names each as stored, numbers in decimal. No sentence makes its verb agree with a list, so one entry fits every list length.
- A key's English is also the accessibility label of the control it names, unless a row says otherwise. Keys are shown where the platform shows them: in the menu items of the commands (macOS menu bar, iPad's ⌘ overlay); a control's help tag is its label alone.
- Typography follows `editor/editing-strings` E1: "…" for a control that asks for more before acting, typographic quotes and apostrophes.

## S2. Mask editor tools and commands (U205; R995, R996, R1000)

| Key | English | Anki | Where |
|---|---|---|---|
| `io.tool.select` | Select | Select | tool (S) |
| `io.tool.rectangle` | Rectangle | Rectangle | tool (R) |
| `io.tool.ellipse` | Ellipse | Ellipse | tool (E) |
| `io.tool.polygon` | Polygon | Polygon | tool (P) |
| `io.tool.text` | Text Label | Text | tool (T); R995's "Text label" |
| `io.tool.fill` | Fill Color | Fill with colour | tool (C); R995's "Fill color" |
| `io.command.zoomOut` | Zoom Out | Zoom Out | `[` |
| `io.command.zoomIn` | Zoom In | Zoom In | `]` |
| `io.command.zoomToFit` | Zoom to Fit | Reset Zoom | F; R996's "Fit" |
| `io.command.translucent` | Translucent Masks | Toggle Translucency | L; a toggle, R996's words |
| `io.command.delete` | Delete | Delete | Delete |
| `io.command.duplicate` | Duplicate | Duplicate | D |
| `io.command.group` | Group | Group Selection | G |
| `io.command.ungroup` | Ungroup | Ungroup Selection | U |
| `io.command.selectAll` | Select All | Select All | A, ⌘A |
| `io.command.align` | Align | Alignment | the menu of the six alignments |
| `io.command.alignLeft` | Align Left | Align Left | ⇧L |
| `io.command.alignCenter` | Align Center | Align Horizontal Centers | ⇧H |
| `io.command.alignRight` | Align Right | Align Right | ⇧R |
| `io.command.alignTop` | Align Top | Align Top | ⇧T |
| `io.command.alignMiddle` | Align Middle | Align Vertical Centers | ⇧V |
| `io.command.alignBottom` | Align Bottom | Align Bottom | ⇧B |
| `io.command.closePolygon` | Close Polygon | none | Return (`editor/mask-editor` M3.3) |
| `io.command.mode` | Mode | Image Occlusion Mode | the menu of E9's two modes (R1000) |

Undo and Redo use the system's Undo and Redo names, with the step's name (`review/reviewer-strings` S5 for the label form); the mask editor's steps have no names of their own (`editor/mask-editor` M8), so they read "Undo" and "Redo" while they act on the mask editor (`editor/mask-editor` M7.1).

## S3. Image picker and the Mask Editor toggle (U206; R991, R993)

| Key | English | Anki | Where |
|---|---|---|---|
| `io.picker.title` | Choose an Image | none | the picker's heading |
| `io.picker.photoLibrary` | Photo Library | none | source (iPhone, iPad) |
| `io.picker.takePhoto` | Take Photo | none | source (iPhone, iPad, with a camera) |
| `io.picker.chooseFile` | Choose File… | Select Image | source |
| `io.picker.paste` | Paste Image | Paste Image from Clipboard | source |
| `io.picker.dropHint` | Drop an image here. | none | the drop area's text |
| `io.picker.noImageOnPasteboard` | There’s no image to paste. | No image found on clipboard. | Paste Image found nothing usable (`editor/image-occlusion-notes` O4.1) |
| `io.picker.notImage` | “%@” isn’t an image. | none | a file that is not an image (O4.1); `%@` the file's name |
| `io.picker.unreadable` | “%@” can’t be opened. | none | an image that cannot be decoded or measured (O4.1); `%@` the file's name |
| `io.maskEditor.toggle` | Mask Editor | Toggle Mask Editor | the toggle (⌥⇧⌘M; R993's words) |
| `io.changeImage` | Change Image… | Reset Image Occlusion | the action (R993's words) |
| `io.changeImage.confirm.title` | Change the image? | Are you sure you want to reset this image occlusion? | the question before the picker (O8) |
| `io.changeImage.confirm.message` | The masks keep their positions on the new image. | none | its message (R1014) |
| `io.changeImage.confirm.change` | Change Image | (OK) | its confirming button |

Cancel buttons use the system's Cancel. The fetch progress of a pasted web URL is `editor/paste` P21's "Getting Media…" with Cancel, and its failure alert P21's.

## S4. Add, open, save and delete (U205, U206; R994, R1015, R1016)

| Key | English | Anki | Where |
|---|---|---|---|
| `io.add.noOcclusions` | No occlusions created | Unable to add. Either you have not added any occlusions, or the first field is empty. | Add refused (O7 step 1); R994 quotes it |
| `io.editor.noRoles` | This note type has too few fields for image occlusion. You can still edit its other fields. | (an error asking to check the database) | O2, above the fields |
| `io.editor.noImage` | This note has no image. | No image to show. (its reviewer's message) | O5 step 1, no image name |
| `io.editor.imageMissing` | “%@” isn’t in the media folder. | none | O5 step 1, `%@` the image name |
| `io.editor.imageUnusable` | “%@” can’t be shown in the mask editor. | none | O5 step 1, another extension or an image that cannot be decoded; `%@` the image name |
| `io.editor.unreadable` | This note’s masks can’t be read (%1$@ in shape %2$lld). You can still edit its other fields. | none | O5 step 3, Edit mode (R1016); `%1$@` is S5's text for the error kind, `%2$lld` the shape index plus one |
| `io.editor.unreadable.noShape` | This note’s masks can’t be read (%@). You can still edit its other fields. | none | the same, for `extra_content`, which has no shape |
| `io.editor.unreadable.copy` | The masks of this note can’t be read (%@). Draw new masks to add it. | none | O5 step 3, Add mode; `%@` S5's text |
| `io.editor.drawNewMasks` | Draw New Masks | none | its action (O5 step 3) |
| `io.editor.unwritable` | This change can’t be saved, because the Occlusion field can’t store one of the masks. | none | `editor/mask-editor` M9, a refused write |
| `io.delete.cardRemains` | Card %lld stays, without a mask of its own, until you use Empty Cards. | none | O9's one-time notice; `%lld` is min(N, 500) |
| `io.label.braces` | Labels can’t contain “}}”. | none | `editor/mask-editor` M3.4 |

## S5. Reading errors (R1016)

The text for each error kind of `render/image-occlusion-format#F7.2`, used in the `io.editor.unreadable` entries:

| Key | English |
|---|---|
| `io.error.extraContent` | text that isn’t a mask |
| `io.error.multipleNumbers` | a mask with several numbers |
| `io.error.nestedCloze` | a cloze inside a mask |
| `io.error.malformedShape` | a damaged mask |
| `io.error.unknownShape` | an unknown shape |
| `io.error.duplicateKey` | a repeated property |
| `io.error.invalidValue` | an invalid value |
| `io.error.missingKey` | a missing property |

## S6. Per-mask audio (U207; R1018, R1019)

| Key | English | Where |
|---|---|---|
| `io.audio.title` | Mask %lld | the inspector's heading; `%lld` is N |
| `io.audio.noMask` | Select a mask to see its linked content. | A6, no mask number |
| `io.audio.linked` | Linked Content | the list of A2 |
| `io.audio.linked.empty` | No other field uses this mask’s number. | A2 with no field listed |
| `io.audio.play` | Play | a play control (A2); its accessibility label is `io.audio.play.label` |
| `io.audio.play.label` | Play %@ | `%@` the file name |
| `io.audio.attach` | Attach Audio… | A4 |
| `io.audio.record` | Record Audio… | A4; the recorder's own text is `editor/paste` P21's |
| `io.audio.replace.confirm.title` | Replace the audio for mask %1$lld in %2$@? | A4 step 1; `%1$lld` N, `%2$@` the audio field's name |
| `io.audio.replace.confirm.replace` | Replace | its confirming button |
| `io.audio.notAudio` | “%@” isn’t an audio or video file. | A4 step 3; `%@` the stored name |
| `io.audio.warning.title` | Other Fields Use the Old Numbers | A7's warning, its title |
| `io.audio.orphaned` | No mask has number %1$lld now. Fields that still use it: %2$@. | A7, an orphaned number; `%2$@` the field list |
| `io.audio.renumbered` | New numbers for masks of number %1$lld: %2$@. Fields that still use %1$lld: %3$@. | A7, an ungroup; `%2$@` the list of new numbers, `%3$@` the field list |
| `io.audio.cardStays` | Card %lld stays while those fields use its number. | A7 in Edit mode, after an orphaned number's line; `%lld` is min(m, 500) |

Anki has no equivalent of S6.

## S7. Accessibility of the canvas (U205; plan U205 "VoiceOver elements per mask")

Each item of the document (`editor/mask-editor` M1) is one accessibility element, and so are the selection handles and an unfinished polygon. Order, identifiers and gestures are the screen contract's (the plan names the identifiers `io.mask.<n>`). An element's selected state is the system's selected trait; its custom actions are the S2 commands that apply to it (Delete, Duplicate, Group, Ungroup, the alignments), with S2's names.

| Key | English | Element |
|---|---|---|
| `io.a11y.canvas` | Image and Masks | the canvas as a container |
| `io.a11y.rectangle` | Rectangle %lld | a rectangle mask; `%lld` its number |
| `io.a11y.ellipse` | Ellipse %lld | an ellipse mask |
| `io.a11y.polygon` | Polygon %lld | a polygon mask |
| `io.a11y.group` | one: `Group %1$lld, %2$lld mask` other: `Group %1$lld, %2$lld masks` | a group; `%1$lld` its number, plural on `%2$lld`, the member count |
| `io.a11y.label` | Text Label: %@ | a text label; `%@` its text, line feeds read as spaces |
| `io.a11y.label.empty` | Empty Text Label | a label whose text is empty |
| `io.a11y.polygon.unfinished` | one: `Unfinished Polygon, %lld point` other: `Unfinished Polygon, %lld points` | the polygon being placed (M3.3) |
| `io.a11y.handle.resize` | Resize | a resize handle |
| `io.a11y.handle.rotate` | Rotate | the rotation handle |

## S8. Strings not in this file

- The mode names: `editor/editing-strings` E9.
- The undo step names: `specs/conformance/undo-ops.md` V4 (the mask editor's own history has none, S2).
- The field check's refusal lines and the Add toast: `editor/add-flow` A5 and its screen contract.
- The recorder, the size warning, the fetch progress and the add failure: `editor/paste` P21.

## S9. Tests

`OcclusionDocumentTests` (U205) and `IOAddEditUITests`, `MaskAudioUITests` (U206, U207) look up each key they show in the catalog and compare its English with this file: `io.add.noOcclusions` exactly as R994 quotes it, the two tool names of R995, R996's command words, E9's mode names (`editor/editing-strings` E12), the substitutions of `io.editor.unreadable` for every S5 kind, `io.audio.orphaned` and `io.audio.renumbered` with one, two and three fields, `io.delete.cardRemains` with 600 (shows 500), and the S7 labels read by VoiceOver for one element of each kind. The catalog test also checks that every entry with two or more arguments numbers them.
