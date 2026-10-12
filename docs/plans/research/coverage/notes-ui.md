# Coverage note: notes-ui


R-ID coverage map (unit → R ranges)
- U190: R825–R833, R820 (save mechanics), R823, R248, R254; R5 (editor part).
- U191 (core): R798–R801 rules, R807, R809–R812 check states, R883 matching.
- U192: R834–R848, R857–R861, R864, R865, R249, R250, R251, R253, R258.
- U193: R882–R890, R256.
- U194: R849–R856, R878–R881, R252.
- U195: R796–R817 (UI side of R798–R801 and R807–R812), R245/R259 (Add), R246, R247, R255, R862; R804 semantics.
- U196: R818–R824, R863, R245/R259 (Edit), R257.
- U197: R866–R877.
- U200: R915–R919, R929–R937, R942–R943 (UI), R276; R914 confirmation UI.
- U201: R920–R928 (UI).
- U202: R938–R941.
- U203: R944–R948.
- U205: R995–R1002, R277.
- U206: R990–R994, R1013–R1016.
- U207: R1017–R1019.

R-IDs in my range owned elsewhere
- R275 (previewer keys): owned by the Browse previewer (U210 area, R1216–R1220).
- R1020 (review of per-mask audio types): owned by U64 (IO render, entity-encoded hidden cloze) and U175/U67 (answer-side audio).
- R804 Settings control placement: U160 (R140). Semantics are in U195.
- Core enforcement of R921 (save-time name rules), R942 and R943: U29. R923, R924, R926 and R927 (structure changes): U34. R945 default mapping and the R948 op: U35. My units supply the UI and the integration tests.
- R821 merge window: U25/U27. U196 only issues the op with a merge key.
- R801 key writes run inside U27's add op, using U191's function.

Implemented here but outside my assigned ranges (orchestrator should reconcile ownership)
- R1009, R1010, R1011 (ordinal allocation, no renumbering, dropping tiny or transparent shapes) live in the U205 editor model. The IO format group R1003–R1012 is otherwise U64's.
- R5: the editor-draft part is in U190.
- R1793: the banner UI is in U196. The requirement is owned by Integrations (U141).
- R1550: the editor publishes `hasUnsavedEdits`; U151 enforces.
- R139: the paste-inversion UI is in U197.

Cross-area contracts assumed
- U27:
  - field-patch note update op (touched fields plus optional tags) with merge key, undo name "Update Note", and a no-op on identical content;
  - add op with `updateAddDefaults`, undo name "Add Note", returns noteId and card count;
  - note-for-copy query (fields, tags, type, card deck → home deck if filtered);
  - note-existence query.
- U29:
  - notetype snapshot with field configs, IO roles (tags, else positions) and the cloze-field ordinal set;
  - non-schema sticky toggle;
  - draft validation returning located errors (live, R930);
  - update and delete ops taking `schemaConfirmed`;
  - `schemaChangeNeedsConfirmation`;
  - counts: notes per type, notes per template, cards per template;
  - stock kinds and add/clone;
  - add stock IO type if missing.
- U34: structure-change ops (fields, templates, sort field, delete, Restore to Default) and the count of notes left cardless on template removal.
- U35: change-notetype info (default maps plus schema stamp) and change op.
- U28: deck snapshot for the choosers (filtered flag, empty Default); create deck by name (deck override).
- U30: tag canonicalization on note save (R891–R893). U191's tag completion may overlap with any tag-completion U30 or U80 builds for search autocomplete; reuse one implementation.
- U31: add-media op returning the stored name; media folder path.
- U20/U31: display encode and decode of media names in field HTML (R988); HTML-to-text-line helper.
- U60–U64:
  - preview render taking draft templates and CSS, note values, ordinal or cloze number, side, fill-empty and type-in simulation;
  - IO Occlusion codec (parse to shapes with original per-shape text; serialize keeping unchanged shapes verbatim);
  - cloze segment extraction per number;
  - IO text-metrics spec for the editor.
- U66/U67:
  - the scheme handler serves an editor-runtime host plus the media host;
  - CardWebView preview mode with night-mode and platform-class overrides;
  - audio player for one-shot playback.
- U80: canonical search builder for `dupe:<ntid>,<text>` and `nid:<id>`.
- U24: change events carry `source`, as in spec-flow C2. Lifecycle state ExclusiveBusy drives KTD17 closing.
- U150:
  - shortcut context registry (R200), with editor, add, edit, template-editor and mask-editor contexts;
  - menu-command routing so ⌘Z/⇧⌘Z go to field undo while a field is focused, and ⌘, reaches Settings only without rich-text focus;
  - a Format menu (an addition to the R279–R287 menu list) carrying the editor commands for R864/R865;
  - A-key entry point;
  - toast host.
- U151: scene-background and resign-active flush hook; sync coordinator honors `hasUnsavedEdits` (R1550); generation reset.
- U175: hosts the Edit panel over the card, suspends review shortcuts while it is open, re-renders the shown side on close; Create Copy entry.
- U210: hosts BrowseEditorPane and awaits `flush()` before selection changes and actions; Preview ⇧⌘P; ⇧⌘M entry for U203; opens searches from Show Duplicates and History (R1114).
- U160: Settings controls for `addToCur`, `pasteStripsFormatting`, `pasteImagesAsPng`, and the device-local "image max size" (see open questions).
- U3/U4:
  - an editor scenario kind (initial field HTML plus selection plus command or paste sequence → field HTML observation), run by a Swift WKWebView harness instead of meguri-cli;
  - goldens with `source: observed` provenance (see the R1960 question below).
- Output structure addition: `app/Packages/MeguriKit/Sources/Features/Editor/Resources/EditorRuntime/` (Meguri-authored editor page), bundled as target resources like the card runtime in `app/Packages/MeguriKit/Sources/CardRendering/Resources/CardRuntime/`. Test targets are `EditorTests`, `NoteTypesTests` and `ImageOcclusionTests` under `app/Packages/MeguriKit/Tests/`.

Conflicts and open questions
1. R249 says ⌘= is subscript and ⇧⌘= superscript. R837 and R861 (matching Anki) say the opposite. U192 follows R837/R861; R249 should be corrected.
2. R258 (⌘] / ⌘[ indent/outdent) differs from R845/R861 (⇧⌘. / ⇧⌘,). U192 binds both pairs in the editor context.
3. R139 says Shift-modified paste is replaced by Paste and Match Style (⌥⇧⌘V). R866 says ⇧⌘V and ⌥⇧⌘V both invert. U197 makes both invert; confirm.
4. R830 Return markup cannot come from the pylib oracle, and R1960 bans Anki web assets in tests. So an automated editor oracle (Anki's editor bundle in headless Chromium) is ruled out. The proposal: the spec writer observes Anki desktop 26.09.3 once and commits the observed HTML as goldens marked `source: observed`. The same applies to the formatting (U192) and paste-filter (U197) goldens. The orchestrator should confirm this reading of KTD1/R1960.
5. Editor conflict policy. R822 (save, then reload) is applied for sync and user ops, made safe by field-patch saves. R1793's banner applies only to LocalApi. Spec-flow default 10 proposed the banner for sync too; that would contradict R822, so it is not adopted.
6. R939 ("about 200 ms after typing stops") versus R1835 ("within 100 ms after typing pauses"). U202 debounces on a short idle and renders within 100 ms of it. One number is needed.
7. R852's device-local "image max size" setting (default 1024 px, can be off) has no Settings requirement; it is placed in Settings › Editing (U160).
8. R277 lists "M magnify", which R995/R996 do not define. R996 adds "A" for select all and ⌘Y redo, which R277 lacks. U205 binds the union; M needs a definition (proposal: a temporary magnifier/zoom tool) or removal.
9. R1018 does not say how a mask's "matching A{NN} field" is found. U207 uses a letter prefix plus the mask number with optional zero padding; confirm.
10. Spec-flow default 9 (run a normal sync before a schema change, "syncing first…") is not in the planning contract. SchemaChangeGate (U200) implements only R951 unless the orchestrator adopts it.
11. Spikes owned here:
    - E1: key routing between SwiftUI commands and a focused WKWebView.
    - E2: no smart-quote substitution in the in-page HTML editor.
    - E3: pre-warmed editor open in ≤100 ms.
    - E4: iOS drop interception into WKWebView.
    
    E1 overlaps the review-area spike S3; one combined spike is suggested.
12. Unit count is 15 (U190–U197, U200–U203, U205–U207), one over the preferred 14. U207 could be merged into U206 if needed.
