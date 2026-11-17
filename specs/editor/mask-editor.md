# Image occlusion mask editor: tools, commands and editing rules

Spec: editor/mask-editor
Requirements: R995, R996, R997, R998, R1000, R1001, R277; supports R999, R1002, R1009, R1010, R1011, R1836
Anki: 26.09.3 for the behavior of Anki desktop's mask editor that M2 to M8 reproduce; none for the layout, input gestures and rendering strategy, which are U205's and the screen contract's
Scenarios: none (the oracle runs Anki's collection library, never its mask editor; M10, M11)

What Meguri's mask editor (U205) does with the shapes of one image-occlusion (IO) note: the starting state, the tools, the commands and their keys, transforms, zoom and pan, the editor's own undo history, and when the Occlusion field is written. The field format, the strict reading, the writer and the number rules are `render/image-occlusion-format` (F3 to F12); this spec only says when the editor calls them. The note flows around the editor (picker, Add, Edit, Change Image, notices, how the session stores the hidden fields) are `editor/image-occlusion-notes`; per-mask audio is `editor/mask-audio`; every label, message and accessibility text is `editor/image-occlusion-strings`. The screen's appearance and placement are the screen contract's (`imageOcclusion.editor`, `docs/design/screens.md`).

## M1. Terms

- **Document**: the editor's model of one note's shapes, in the editor's order (bottom to top), with each shape's number, the mode, and for shapes read from the field their source (`render/image-occlusion-format#F7.3`). Groups are `render/image-occlusion-format#F7.4`'s.
- **Item**: a mask that is not in a group, a group, or a text label.
- **Reference size** W × H: the writer's reference size (`render/image-occlusion-format#F11`), in points (CSS pixels in Anki). It is set each time an image loads into the editor (the editor opens a note, or a new image is chosen, `editor/image-occlusion-notes` O4.2, O8), from the editor's area at that moment, as Anki sets it:
  1. the image's pixel size is scaled to fit the area, keeping its aspect ratio, enlarging small images as well as shrinking large ones;
  2. when that fitted size, in device pixels (points × the screen's scale), has more than 4096 × 4096 = 16,777,216 pixels, it is scaled down, keeping its aspect ratio, to that many pixels;
  3. each side is rounded down to a whole number of device pixels and converted back to points.

  Example, at a screen scale of 2: a 4000 × 3000 image in an area of 800 × 500 points fits as 666.67 × 500 points, 1333.33 × 1000 device pixels, rounded down to 1333 × 1000, so W × H = 666.5 × 500 points. The reference size then stays fixed while that image is loaded: resizing the window or the split view changes the zoom (M6.1), never W × H.
- **Reference points**: lengths at the reference size. Every pixel constant of this spec (5, 10, 40) is in reference points, which Anki's editor measures in its scene units, so it does not change with zoom. Fractions (`render/image-occlusion-format#F1`) are reference points divided by W (horizontal) or H (vertical). **Screen points** are lengths as shown, at the current zoom.
- **Inside the image**: a point whose fractions x and y are both in [0, 1].
- **Selection**: zero or more items.
- **Bounding box**: an item's axis-aligned box as drawn, rotation included; a group's is the box of its members; a selection's is the box of its items.
- **Completed change**: the end of one user action that changes the document: a shape added, a drag, resize or rotation ended, a delete, a duplicate, a group or ungroup, an alignment, a fill, the end of a label edit, a mode change. A drag that is still in progress is not one. Undo and Redo are not completed changes (M8, M9).

## M2. Opening the editor

| Item | When adding a note (including Create Copy, U195) | When editing a note (Edit, Browse, review edit) |
|---|---|---|
| shapes | none, or for Create Copy and a change between IO note types the strict reading of the Occlusion field (`editor/image-occlusion-notes` O5, O10) | the strict reading of the Occlusion field (`render/image-occlusion-format#F7`); a reading error opens no editor (`editor/image-occlusion-notes` O5) |
| starting tool (R995) | Rectangle | Select |
| mode (R1000) | Hide All, Guess One for the first note of an Add screen; then the mode the previous note of the same Add screen ended with (`editor/image-occlusion-notes` O7); for Create Copy, the copied field's mode | the field's mode (`render/image-occlusion-format#F5`) |
| zoom | Fit (M6.1) | Fit |
| undo history (M8) | empty | empty |
| translucency (M6.4) | as last set | as last set |

- Anki keeps the mode chosen in its Add window for the next note added there until the window closes; Meguri does the same for one Add screen (decided 2026-11-10, Anki's behavior). A new Add screen starts in Hide All, Guess One.
- Opening the editor writes nothing. Closing it without a completed change writes nothing (R1012; `render/image-occlusion-format#F11` rule 1).

## M3. Tools (R995, R277)

One tool is active at a time. Choosing a tool (its control or its key in the table below) keeps it active until another is chosen, except Text (M3.4). Choosing any tool first discards an unfinished polygon (M3.3).

| Tool | Key | Press on an empty area | Press on a shape |
|---|---|---|---|
| Select | S | starts a selection rectangle (M3.6) | selects it (a member of a group selects the group) and drags it |
| Rectangle | R | inside the image: draws a rectangle (M3.1) | selects and drags the shape, as Select |
| Ellipse | E | inside the image: draws an ellipse (M3.1) | selects and drags the shape, as Select |
| Polygon | P | inside the image: adds a point (M3.3) | inside the image: adds a point there, or closes the polygon near its first point (M3.3); shapes cannot be selected or moved with this tool |
| Text label | T | inside the image: places a label (M3.4) | selects and drags the shape, as Select |
| Fill color | C | nothing | fills it (M3.5) |

A press outside the image draws nothing, adds no point and places no label, with every tool; with Select it starts a selection rectangle. On iPhone and iPad one finger does what the pointer does here; two fingers never draw (M6.3, R1002).

### M3.1 Rectangle and ellipse

- The drag spans a box from the press point to the current point, in any direction. The current point may leave the image; the press point may not.
- At release the box is width w and height h in reference points. When w < 5 or h < 5 nothing is added, and there is no undo step.
- Otherwise the new mask is added on top: for a rectangle, `left` and `top` the box's top-left corner and `width`, `height` the box's size; for an ellipse, the box is its bounding box: `left`, `top` its top-left corner, `rx` = w / 2 and `ry` = h / 2. Angle 0, fill none (drawn `#ffeba2`), number by `render/image-occlusion-format#F12` (new mask). It becomes the selection; the tool stays.

### M3.2 Default look of new masks

A new mask has no `fill` (the writer then writes none, `render/image-occlusion-format#F10`) and is drawn in `#ffeba2`, Anki's mask color, which is part of the note's look on every client. Its outline and the selection handles are the screen contract's.

### M3.3 Polygon

- Each press inside the image adds a point at the press, whether it falls on an empty area or on a shape. While points exist, a preview shows the open polygon and a segment from the last point to the pointer. The first point is marked so that it can be found again.
- **Closing** (R995): a press near the first point closes the polygon and adds no point. Near means within 5 reference points of it, or, for a touch, within 22 screen points (half the 44-point hit target), whichever is larger. While the unfinished polygon has at least 3 points, Close Polygon (Return, and a control the screen contract places) closes it too.
- **Added or not.** A closed polygon is added when it has at least 3 points and its points span more than 5 reference points in both x and y (largest x − smallest x and largest y − smallest y, each > 5); otherwise nothing is added and there is no undo step. Its `points` are the pressed points in order, its `left` and `top` the smallest x and the smallest y of its points, its number by F12 (new mask). It becomes the selection; the tool stays.
- **Discarded.** An unfinished polygon (points but not closed) is discarded, adding nothing, when another tool is chosen, when Undo is used (that Undo does only this, M8), when Redo is used (Redo then goes on), when the editor closes or switches to the fields view (`editor/image-occlusion-notes` O3), and on Esc (decided 2026-11-10). A second finger does not discard it (M6.3).

### M3.4 Text label

- A press inside the image on an empty area places a new label on top: text `text`, top-left corner (`left`, `top`) at the press, angle 0, `scale` 1, `fs` = 40 / H (a font size of 40 reference points), no `fill` (drawn `#000000` on a white box, as `render/image-occlusion-review#M8.4` draws labels), number 0 (`render/image-occlusion-format#F12`). These are Anki's defaults.
- The label is then edited at once with its whole text selected, so typing replaces `text`, and the active tool becomes Select.
- Editing a label: the label's text editor takes the keys (M7). Return inserts a line feed, which breaks the line (`render/image-occlusion-format#F6`). Esc, a press outside the label, choosing a tool, or the Mask Editor toggle ends the edit; that end is one completed change when the text changed.
- `}}`: the format cannot store a label holding `}}` (`render/image-occlusion-format#F10` Refusal). An edit (typing, paste, dictation, deletion) after which the text would contain `}}` is not applied, and the editor shows `editor/image-occlusion-strings` `io.label.braces`. A label read from the field never holds `}}` (`render/image-occlusion-format#F3`).
- A label whose text is empty stays in the document until the editor closes; the writer leaves it out of the field (`render/image-occlusion-format#F11` rule 2.2).

### M3.5 Fill color

- Choosing Fill color (its control or C) opens the color picker each time. The picker starts at the color the tool last used in this editor, or `#ffeba2` at first; its swatches are `#ffeba2` followed by the custom swatches of `editor/config-keys` K9, which it reads and writes by K9's rules (adding a swatch writes the key; picking a color does not).
- While the tool is active, shapes cannot be selected or moved. A press on a mask or label sets that shape's fill to the tool's color as lower-case `#rrggbb`; inside a group, only the member under the press changes. A shape whose fill already equals that color (an absent fill counts as the kind's default, `#ffeba2` or `#000000`) does not change and there is no undo step. A fill of `#ffeba2` on a mask, or `#000000` on a label, is written as no `fill` key (`render/image-occlusion-format#F10`).
- A picked color is converted as `editor/formatting` F7 says (sRGB, each component clamped to 0 to 1, rounded half up to 0 to 255, lower-case `#rrggbb`, alpha ignored) before it becomes the tool's color or is added to the palette.

### M3.6 Selecting

- A press on an item selects it alone; ⇧-press and ⌘-press add the item to the selection or remove it from it. A press on an empty area with Select clears the selection and starts a selection rectangle.
- The selection rectangle selects every item whose bounding box it intersects or contains (⇧ or ⌘ held at the press: adds them to the selection). It may start anywhere in the editor's area.
- Select All (M4.4) selects every item. A selection never holds a member of a group without its group.

## M4. Commands (R996, R277)

Anki's keys use Control on Windows and Linux and Command on macOS; the keys below are the macOS forms. Keys come from U158's shortcut registry in the mask editor's context (M7.1); the screen contract places the controls and the menu items, where each key is shown.

| Command | Anki 26.09.3 | Meguri | Enabled when |
|---|---|---|---|
| Undo | ⌘Z | ⌘Z | the history can undo, or a polygon is unfinished (M8; otherwise ⌘Z goes to the host, M7.1) |
| Redo | ⌘Y | ⇧⌘Z and ⌘Y | the history can redo (otherwise the keys go to the host, M7.1) |
| Zoom Out | `[` | `[` | always |
| Zoom In | `]` | `]` | always |
| Zoom to Fit | F | F | always |
| Translucent Masks | L | L | always |
| Delete | the forward-delete key | the forward-delete key and ⌫ | the selection is not empty |
| Duplicate | D | D | the selection is not empty |
| Group | G | G | the selection's masks (a selected group counts as its masks) have at least two different numbers |
| Ungroup | U | U | the selection holds at least one group |
| Select All | A | A and ⌘A | the document is not empty |
| Align Left, Center, Right | ⇧L, ⇧H, ⇧R | ⇧L, ⇧H, ⇧R | the selection is not empty |
| Align Top, Middle, Bottom | ⇧T, ⇧V, ⇧B | ⇧T, ⇧V, ⇧B | the selection is not empty |
| Close Polygon | none | Return | an unfinished polygon has at least 3 points (M3.3) |
| Mode menu (R1000) | (menu only) | (menu only) | always |
| Mask Editor toggle (R993) | ⌥⇧⌘M | ⌥⇧⌘M | always (`editor/image-occlusion-notes` O3) |

- A disabled command does nothing and records no undo step; its control is disabled.
- **M.** R277 lists "M magnify". Anki 26.09.3 binds no action to M in its mask editor (the key is reserved and does nothing), and in review M is the card's own key (R426). Meguri binds nothing to M in the mask editor; magnifying is Zoom In, ⌘-scroll and pinch (M6.1). Whether M should zoom is open for Antoine's decision; until then M does nothing and types nothing.
- Each command that changes the document is one completed change (M1, M8, M9).

### M4.1 Delete

Removes every selected item; a selected group is removed with its members. Other shapes keep their numbers and the gaps stay (`render/image-occlusion-format#F12`). Deleting a numbered mask shows `editor/image-occlusion-notes` O9's notice and `editor/mask-audio` A7's warning when they apply.

### M4.2 Duplicate

Copies the selection, with every copied shape moved 10 reference points right and 10 down from its original, adds the copies on top, and makes them the selection. Duplicating again copies the copies, so repeated Duplicate steps by 10 points each time. Numbers: F12's Duplicate rule (a copied mask a new number, a copied group one new number, copied labels 0).

### M4.3 Group and Ungroup

- Group: F12's Group rule, over every mask of the selection (the masks of selected groups included); the result is one group. The group's members move, scale and delete together (M5); a group cannot be rotated (R997). Selected labels are not grouped and stay selected.
- Ungroup: F12's Ungroup rule for each selected group, in the editor's order of their first masks. The former members become separate items and stay selected; selected masks and labels outside groups are unchanged.

### M4.4 Select All

Selects every item of the document: masks, labels and groups.

### M4.5 Align

With a selection of two or more items, each item is moved (never resized) so that one edge or center line of its bounding box matches the selection's bounding box: Left its left edge, Center its horizontal center, Right its right edge, Top its top edge, Middle its vertical center, Bottom its bottom edge. With one item, the item is aligned the same way to the image (fractions 0, 0.5 or 1). Decided 2026-11-10: these are Meguri's rules; Anki's results differ in the cases M10 lists. An alignment that moves nothing is not a step (M8).

## M5. Transforms (R997)

- **Move.** A drag with Select (or with Rectangle, Ellipse or Text label started on a shape) moves the selection. While it moves, an item whose bounding box fits inside the image is kept inside it, except that its right and bottom edges may pass the image's by up to 5 reference points; an item larger than the image moves freely. These are Anki's limits.
- **Resize, one shape.** Rectangles and ellipses resize freely in width and height from edge and corner handles; the change is stored in `width` and `height`, or `rx` and `ry`. Text labels have only corner handles and scale uniformly: the change is stored in `scale`, never in `fs` or in the text (R997). A polygon has corner and edge handles and its resize moves its points. A resize never makes a mask's bounding box less than 5 reference points wide or high: the handle stops there, so a resize never makes a mask that the writer would leave out (`render/image-occlusion-format#F11` rule 2.2).
- **Resize, a group or several items.** Decided 2026-11-10: a group, and a selection of several items, have corner handles only and scale uniformly about the opposite corner of their bounding box. Every member scales by the same factor: its position relative to that corner, rectangle sizes, ellipse radii, polygon points and label `scale` (never `fs`); angles do not change. The handle stops when any mask of the selection would become less than 5 reference points wide or high.
- **Rotate.** Only one selected shape that is not a group has a rotation handle; a multi-item selection and a group have none. When the rotation ends, its angle is taken in (−180°, 180°]; when it is less than 3° from 0° (strictly between −3° and 3°), it snaps to 0°. Rectangles, ellipses and labels store the angle (`render/image-occlusion-format#F9`); a polygon's rotation moves its points and stores no angle (`render/image-occlusion-format#F6`), as Anki's editor does.
- **Changed polygons.** When a polygon is moved, resized or rotated, its stored points become its points as drawn (F6's offset of `left` and `top` applied) and its `left` and `top` the smallest x and the smallest y of those points, so a polygon read with `left` and `top` off its minimums is not shifted by its first change.
- **Back into the image.** Decided 2026-11-10: at the end of every move, resize or rotation, each moved item whose bounding box starts left of the image or above it is moved right or down so that the box starts at the image's left or top edge, and an item whose box starts right of the image or below it is moved back so its box ends at that edge. Anki does this for single rectangles, ellipses and labels only (M10).
- Moves, resizes and rotations never change a number (R1010; `render/image-occlusion-format#F12`).

## M6. Zoom, pan and translucency (R998, R999)

The mask editor is not a platform control, so the platform defines no zoom steps or translucency for it; Meguri uses Anki's values, which also keep a user's sense of scale the same in both apps.

### M6.1 Zoom

- The zoom z is the shown size of the image divided by the reference size. When an image loads, z = 1 and the image fits the editor's area (M1). **Fit** is the z at which the whole image fits the current area, in the middle of it: 1 at load, and another value after the area has been resized.
- Zoom In multiplies z by 1.1 and Zoom Out divides it by 1.1, each about the center of the visible area, then clamps z to [0.5, 5] (R998). Zoom to Fit (F) sets z to Fit and centers the image.
- ⌘-scroll (macOS) zooms about the pointer: z becomes z × 0.999^Δy, Δy being the scroll's vertical delta in points (positive scrolls down), then clamped to [0.5, 5]; the image point under the pointer stays under it (U205 test scenario). These are Anki's factors.
- Pinch (iPhone, iPad, trackpad) zooms about the gesture's center within the same range.
- Resizing the window or the split view keeps W × H (M1) and sets z to the new Fit, as Anki does; that z is not clamped until the next zoom step.
- Zoom changes neither the document nor the reference size: every rule in reference points stays the same at any zoom.
- Showing the full-resolution image when zoomed (R999) is U205's rendering; it changes no rule here.

### M6.2 Pan

On macOS, scrolling pans (vertical and horizontal deltas), ⇧-scroll pans sideways by the vertical delta, and Space-drag pans with any tool. On iPhone and iPad two fingers pan (R1002). Panning changes nothing in the document.

### M6.3 Touch

One finger does what the pointer does (M3). A second finger cancels a rectangle or ellipse being drawn and a move, resize or rotation in progress (the item returns to where it was; nothing is added and there is no step), and the gesture becomes zoom and pan (R1002). Decided 2026-11-10: an unfinished polygon keeps its points through the gesture, so a polygon can be placed while zooming; Anki discards it (M10).

### M6.4 Translucent masks (L)

While on, every shape (new ones included) is drawn at opacity 0.4, Anki's value; off, at full opacity. It changes nothing in the note. Anki remembers it for the life of its editor page; Meguri remembers it on the device (R996 "remembered"), across notes and launches (U205). It is not collection config and never syncs.

## M7. Keys

### M7.1 The mask editor's context

- The mask editor registers its keys in the context `imageOcclusion.maskEditor` of U158's registry (`editor/formatting` F14): active while the mask editor view shows in the key window and keyboard focus is in it (the canvas, its tools, the mode menu, the audio inspector), not while a sheet, alert or popover is in front of it.
- Its keys take precedence over the hosting screen's uses of the same keys (`editor/add-flow` A9, Browse's editor pane, the review edit panel, `editor/edit-flows` W2 and W3).
- **Esc** goes to the mask editor only while a polygon is unfinished (it discards it, M3.3) or a label is being edited (it ends the edit, M3.4); otherwise Esc goes to the host (Close in the Add screen, focus to the table in Browse, closing the review edit panel).
- **⌘Z, ⇧⌘Z and ⌘Y** act on the mask editor's history while it can undo or redo (M8); otherwise they go to the host (`editor/add-flow` A5's undo of "Add Note", or the collection's undo, `editor/field-html` F5). Edit › Undo and Edit › Redo follow the same rule and show the mask editor's step while it acts on it (`editor/image-occlusion-strings` S2).
- ⌘↩ (Add) and the host's other keys keep working; the Add screen flushes before it adds (`editor/add-flow` A5 step 2), so the add takes the field as the mask editor last wrote it (M9).
- The registry test of `editor/formatting` F14 lists the context's bindings over each host and fails on a duplicate (R200).

### M7.2 While a label is edited (R1001)

While a label's text is being edited, none of the single-letter, `[`, `]`, ⇧-letter, Delete, ⌫ and Return keys of M3 and M4 acts on the editor: they go to the label (pressing R types an R). ⌘Z, ⇧⌘Z, ⌘Y and ⌘A act on the label's text, with the text editor's own undo. Esc ends the edit (M3.4). ⌥⇧⌘M stays active, as in Anki (it types nothing): it ends the edit and then switches the view. When the edit ends, the keys act on the editor again.

## M8. Undo history

- The editor keeps its own history of document states, separate from the collection's undo (U205 `DocumentHistory`). It is empty when the editor opens a note or a new image (M2, `editor/image-occlusion-notes` O8) and when the document is rebuilt from the field (`editor/image-occlusion-notes` O6.2).
- Each completed change (M1) adds one step, except a change after which the document equals the current step's (for example a fill with the color the shape has, or an alignment that moves nothing). A mode change is a step in Meguri's history; Anki's history does not record mode changes (M10).
- Undo restores the previous state, Redo the next; numbers are restored with the shapes (R1010). A new step after an Undo discards the steps that Redo could have restored. Undo and Redo add no step and show neither `editor/image-occlusion-notes` O9's notice nor `editor/mask-audio` A7's warning.
- With an unfinished polygon, Undo only discards it (M3.3).
- Decided 2026-11-10: switching to the fields view and back keeps the document and its history, in Add and in Edit mode. Anki keeps them only in its Add window; when editing a note it rebuilds the editor from the stored note each time it shows (M10).
- The note's own undo (Update Note, Add Note) is the editor session's (U190): the history here never undoes a saved field, and the collection's undo never edits the document while it is open; a collection undo that changes the Occlusion field reaches the editor as any other change of the note (`editor/image-occlusion-notes` O6.2).

## M9. When the Occlusion field is written

- After each completed change, and after each Undo or Redo, the editor calls the writer (`render/image-occlusion-format#F11`, with the reference size of M1) and hands its output to the editor session as the Occlusion field's new value (`editor/image-occlusion-notes` O6). Anki likewise writes the field after every change; neither has a separate Save for masks. Returning to the state read writes the original text (F11 rule 1).
- Zoom, pan, selection, tool choice, translucency and an unfinished polygon never write.
- **Refused write.** When the writer refuses the input (`unwritable_shape` or `invalid_size`, F10, F11), the completed change is taken back: the document returns to its state before the change, no step is added, nothing is written, and the editor shows `io.editor.unwritable`. This can happen without a label holding `}}`: a shape read with an unknown property whose value ends in `}` is refused when it would be written last, for example on a switch to Hide One, which drops the `oi` that came after it. An Undo or Redo whose state the writer refuses is not taken: the history stays where it was, with the same message.
- The writer's output, not the document, is what the field holds: shapes it leaves out (rule 2.2: an empty label; a changed shape read from the field that is below 5 reference points) stay drawn until the editor closes, and `editor/image-occlusion-notes` O7 judges the output.

## M10. Differences from Anki

The oracle cannot run Anki's mask editor, so none of these is in `conformance/deviations.yaml`; Meguri's tests assert them (M11).

| Item | Anki 26.09.3 | Meguri |
|---|---|---|
| Redo, Select All, Delete keys | ⌘Y; A; the forward-delete key | also ⇧⌘Z (R996), ⌘A (R996) and ⌫ |
| Undo and Redo while editing a label | inactive | act on the label's text (M7.2) |
| ⌘Z with nothing to undo | does nothing in the mask editor | goes to the host's undo (M7.1) |
| Polygon: press on a shape before the first point | adds nothing | adds a point (M3.3) |
| Polygon: press on any point marker or on a group read from the field | closes the polygon | adds a point, unless near the first point (M3.3) |
| Polygon with 2 points | added when its points span more than 5 points both ways | needs 3 points (M3.3) |
| Closing a polygon | a press on the first point's 5-point marker | also within 22 screen points on touch, and Close Polygon (Return) (M3.3) |
| Esc during a polygon | nothing | discards it (M3.3) |
| A pinch during a polygon | discards it | keeps it (M6.3) |
| Labels holding `}}` | accepted; the save then writes a field that no reader parses back | refused while typing (M3.4) |
| Resizing below 5 points | allowed; the shape is dropped at the save | stops at 5 points (M5) |
| Scaling a group or several items | free in width and height, so members change shape | uniform, corner handles only (M5) |
| Back into the image | single rectangles, ellipses and labels, at the end of a move, resize or rotation | every item, at the same moments (M5) |
| Zoom Out | divides by 1.1 twice per step, and can go below 0.5 | divides once, never below 0.5 (M6.1) |
| Scroll zoom on macOS | Control-scroll (the web view also reports a trackpad pinch that way) | ⌘-scroll (R998) and pinch (M6.1) |
| Pinch zoom | about the canvas center | about the gesture center (M6.1) |
| Align with one item | Left and Top to the image's edge; Right, Center, Bottom and Middle to the editor area's size, ignoring zoom and pan | all six to the image (M4.5) |
| Align with several items | Left and Top use the items' unrotated left or top; Right uses the right edge of the item that starts furthest right, not of the one that ends furthest right; Center and Middle are computed from those same items; boxes ignore rotation | the selection's bounding box (M4.5) |
| Align by key | records no undo step and writes nothing until the next change (by its control it does both) | one completed change either way (M4.5, M8, M9) |
| Grouping labels | a label in the selection is grouped and takes the group's number | labels stay out (`render/image-occlusion-format#F12`) |
| Numbers on duplicate, ungroup and polygon edits | assigned at save, lowest gaps first; polygon edits clear the number | F12, at once (R1009, R1010) |
| Mode change in the history | not an undo step | an undo step (M8) |
| Fields view and back, editing a note | rebuilds from the stored note; history lost | keeps the document and history (M8) |
| Translucency memory | the editor page's life | the device (M6.4) |

## M11. Tests

Meguri's unit tests (U205, `OrdinalAllocatorTests`, `OcclusionDocumentTests`, `AlignmentTests`) and UI tests (`MaskEditorUITests`) take expected values from this spec: M1's reference size example (666.5 × 500 at scale 2) and an image over the 4096 × 4096 budget, and W × H unchanged after a window resize; M2's starting tools and mode carry-over; M3.1's 4.9 and 5 point boundaries; M3.3's closing radius (5 reference points; 22 screen points on touch), Close Polygon with 2 and 3 points, the three-point minimum, the discard cases, and a press on a shape before the first point adding a point; M3.4's defaults (`text`, scale 1, fs = 40 / H, number 0) and the `}}` refusal; M3.5's no-change fill, and a Display P3 pick outside sRGB and a grayscale pick converted by F7's rule; M3.6's rectangle that touches one item and contains another (both selected) and ⇧-press removing an item; M4's key table and enabled states (Group disabled for one selected group, Ungroup of two groups), M doing nothing; M4.2's offsets after two duplicates (20 points from the original); M4.5's six alignments with one and with three items, one rotated; M5's clamp at the right edge (5 points of overhang allowed), the 5-point resize stop, a group scaled by 2 (positions and sizes doubled, angles kept), the 2.9° and 357.1° snaps and the 3.1° non-snap, no rotation handle on a group, a polygon read with `left` 0.01 below its minimum moved once (drawn position kept, `left` written as the minimum); M6.1's clamps at 0.5 and 5, Fit after a resize, and the fixed point under ⌘-scroll; M6.3's pinch during a polygon (points kept); M7.1's Esc, ⌘Z with an empty history in the Add screen (undoes "Add Note") and ⌘Z with a step; M7.2 with R typed into a label and ⌥⇧⌘M ending the edit; M8's history steps, polygon Undo and the history kept across the fields view; M9's single write per completed change and per Undo, and the refused write (an unknown property `zz=a}` read in Hide All, then a switch to Hide One: taken back, message shown, field unchanged).
