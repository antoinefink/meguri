# Field HTML, field list and save pipeline

Spec: editor/field-html
Requirements: R248, R254, R820, R821, R823, R825, R826, R827, R828, R829, R830, R831, R832, R833, R5 (editor part), R988 (editor side); supports R396, R563, R807, R812, R822, R877, R1833, R1834, R1960, R2017; AE40
Anki: 26.09.3
Scenarios: conformance/scenarios/editor/field-html/

The note editor component (U190) that Add (U195), the edit flows (U196, U184), the Browse editor (U221), the Edit Field sheet (U220) and the image-occlusion flows reuse: what field HTML it shows and what it writes back, when a change is saved and as which op, the field list, and the messages between the editor page and its native host. Formatting and cloze tools (U192), the tag editor (U193), media insertion and image display (U194), paste and drop (U197) and what the editor does when another writer changes the note (U196, R822) build on it and are specified with those units.

Meguri's editor page is Meguri's own: no Anki editor script, style or asset is used in the app or in its tests (R1960).

## E1. Terms

- **Stored HTML**: a field value as the note row holds it (`notes/note-records` N1).
- **Display HTML**: the stored HTML with local media names percent-encoded for the web view (`text/media-references` M6, R988). The page only ever receives display HTML.
- **Rich mode** and **HTML mode**: the two ways a field is edited, a rich-text editable area or a plain source text area (R827). A field shows one of them at a time.
- **Content**: what the field holds in the page now: the editable area's DOM in rich mode, the text area's text in HTML mode.
- **Serialization**: the HTML fragment serialization algorithm of the HTML Standard, as the page's web engine applies it to the editable area's children.
- **Block element**: an element whose tag is one of `address`, `article`, `aside`, `blockquote`, `details`, `dialog`, `dd`, `div`, `dl`, `dt`, `fieldset`, `figcaption`, `figure`, `footer`, `form`, `h1` to `h6`, `header`, `hgroup`, `hr`, `li`, `main`, `nav`, `ol`, `p`, `pre`, `section`, `table`, `ul`, or any element with a `block` attribute whose value is not `false`.
- **Inline-only**: content in which no element, at any depth, is a block element. Empty content is inline-only.
- **Input**: a change of a field's content made by the user: typing, deleting, Return, an undo or redo inside the field (F5), a formatting or cloze command (U192), an insertion (U194), a paste or drop (U197). Focus, caret and selection moves, scrolling, collapsing or expanding, and switching between rich and HTML mode are not inputs.

## E2. Sources and goldens

Decided 2026-10-27, for every editor spec: editor behaviour that the Python-library oracle cannot observe is specified from Anki 26.09.3 in this file's own words. Its goldens are observed by an oracle maintainer who runs Anki 26.09.3's own editor page inside a quarantined oracle image, as `stats/graphs-common` G12 decision 2 does for the graphs page. The page and its assets never leave that image; goldens hold observed values only; Meguri's tests and app never load any of it (R1960). This replaces the plan's U190 note that a spec writer records the editor goldens once by hand.

- The page runs in the web engine of the pinned Anki desktop build (its QtWebEngine), because Return and space handling are that engine's defaults. Key presses reach it as native key events, never as script edits of the DOM. Each golden records the engine and its version.
- Goldens are authoritative for the markup that keystrokes produce (H4, H5). Every other rule in this file is normative as written, and a golden that contradicts one is a finding against this file.
- Automated Meguri tests compare Meguri's runtime, in WKWebView on macOS and the iOS simulator, only with these goldens and with this file.

## H1. Loading a field

1. The host gives the page each shown field's display HTML (B4).
2. Rich mode: the page parses the display HTML as an HTML fragment with a body-like context (as setting `innerHTML` on a `div` does). When the parsed content is inline-only, the page appends one `<br>` element after it. This makes a final line break of the stored HTML show as an empty last line on which the caret can sit, and gives an empty field a line to type on. H3 step 1 takes it out again.
3. HTML mode: the text area holds the display HTML verbatim, unparsed.
4. MathJax (`\(…\)`, `\[…\]`), sound tags (`[sound:…]`), cloze markup and template-like text are ordinary text in both modes: nothing is typeset, replaced or decorated (R563, R2017).
5. Nothing in field content runs (F6). Relative `src` names load from the collection's media folder.
6. Loading writes nothing. A field that receives no input is never serialized, so opening a note and closing it without input writes nothing (R823, AE40).

## H2. Changed and unchanged fields

- A field's **baseline** is a stored HTML value: the one the field was loaded with, until the host's `setField` (B3) replaces it with the value that message carries. The page holds it as display HTML: B4's `baselineHtml` when the load gives one, otherwise the loaded `html`.
- A field loaded without `baselineHtml` and with no input since is **unchanged**. A field loaded with `baselineHtml` (F7) starts **changed**, and so does any field after an input; it stays changed until a commit (S1) finds it equal to its baseline again:
  - in rich mode, when H3 steps 1 and 2 of its content equal the same steps applied to the baseline's display HTML parsed as H1 step 2 parses it (the page may compute the baseline's value just before applying the first input);
  - in HTML mode, when the text area's text equals the baseline's display HTML.
- The page alone decides whether a field is unchanged, always against its baseline, and reports it with each commit (`unchanged`, B2, B3).
- A field's **target** is the value the editor would store: for an unchanged field its baseline's stored HTML, byte for byte (R823); for a changed field the result of H3.
- So undoing every edit inside a field (F5) makes it unchanged again, and its stored bytes come back even when the engine would have serialized them differently.

## H3. Serializing a changed field

The steps, in this order, on the field's content:

1. Rich mode: serialization of the content. When the content is inline-only and its last child node is a `<br>` element, that `<br>` is left out. HTML mode: the text as it is.
2. Rich mode: every `data-meguri-editor` attribute is removed. It is the only attribute the runtime may add to elements inside field content (U194 keeps an image's display size state in it), and the runtime adds no element other than H1's `<br>`.
3. Every U+0000 is removed.
4. Local media names are decoded (`text/media-references` M7), undoing H1's display encoding.
5. A result that is exactly `<br>` or exactly `<div><br></div>` becomes the empty string (R830). No other form is emptied here: `<div></div>`, `<br><br>`, ` ` and `&nbsp;` stay as they are. Whether a field counts as empty for the Add checks is `text/field-emptiness` and `notes/field-check` K3.

The core's save normalization (`notes/note-records` N2) then runs on every field of the note.

Step 1's serialization rewrites the whole field, the parts the user did not edit included. Anki's editor parses and serializes the same way, so these rewrites are the same in both (examples, rich mode):

| Content as loaded | After an input at its end and step 1 |
|---|---|
| `<span  class="x">a</span><br/>` | `<span class="x">a</span><br>` and the input |
| `<img src='a.png'>` | `<img src="a.png">` and the input |
| `<B>a` | `<b>a</b>` with the input inside or after it, as the caret was |
| `&eacute;&#31169;` | `é私` |
| U+00A0 or `&#160;` | `&nbsp;` |
| `a` CR LF `b` | `a` LF `b` |
| `<!-- c -->`, `onerror="…"` | kept as they are |

Meguri also keeps `<script>…</script>` and `<link>` elements as they are through step 1. Anki does not: it removes them from every field when the note loads (X1, R831).

Where WebKit's serializer and the pinned engine's write a scenario's input differently, Meguri's runtime corrects its output to the golden; a difference it cannot correct becomes a deviation entry.

## H4. Return and Shift-Return (R830)

- In rich mode, Return starts a new line as a new block (the pinned engine's line element is `div`) and Shift-Return inserts a `<br>` line break, both at the caret, after replacing a non-empty selection. In a list item, Return starts a new item, and Return in an empty last item leaves the list.
- The exact markup is what the goldens of T1's Return group record. Meguri's runtime handles the paragraph and line-break inputs (`beforeinput` with `insertParagraph` and `insertLineBreak`) itself instead of WebKit's default, and for each scenario its H3 result must equal the golden byte for byte. Its rule for cases no scenario covers is the general rule those goldens show; a later golden that contradicts the rule changes the rule.
- Return never inserts a line feed character into rich content.
- In HTML mode, Return and Shift-Return insert one line feed (U+000A) into the text.
- Add shortcuts (⌘Return, U195) never insert anything.

## H5. Spaces

- In rich mode, a typed space that HTML would collapse or hide (at the start or end of a line, or next to another space) is held as U+00A0, so H3 writes it as `&nbsp;`. The pinned engine rebalances a run of spaces while the user types: a trailing U+00A0 becomes an ordinary space once a character is typed after it, and a run alternates the two. The goldens of T1's space group give the exact patterns; Meguri's runtime reproduces them.
- In HTML mode, a space is an ordinary space.
- U+00A0 characters and `&nbsp;` that were already in a changed field are written `&nbsp;`; in an unchanged field they stay as stored.

## H6. HTML mode (R254, R827)

- ⇧⌘X, or the field's mode button, switches the focused field between rich and HTML mode.
- Entering HTML mode, the text area gets its baseline's display HTML verbatim when the field is unchanged, and otherwise H3 steps 1 and 2 of the rich content, so the names stay display-encoded as in H1 and the same steps 3 to 5 apply to both modes.
- Leaving HTML mode, the rich content is parsed from the text as H1 step 2 parses display HTML.
- A switch is not an input: switching an unchanged field back and forth leaves it unchanged. A pending commit of the field is made first (S1).
- The text area has autocorrect, spell checking, auto-capitalization, smart quotes and smart dashes off: typing `"` stores U+0022 and `--` stores two hyphens, on Mac and on iOS.
- The "Close HTML tags" option (`closeHTMLTags`, U192) closes a tag as it is typed; U192 specifies it.
- Tab in the text area moves focus (F4); it inserts no tab character.

## S1. Commits (R820)

A commit is the page handing one field's H3 steps 1 and 2 result to the host, with whether the field is now unchanged (H2). A field commits:

- **idle**: 600 ms after its last input. Each input restarts that field's timer. Every field has its own timer, so a commit of one field never cancels another field's pending commit.
- **blur**: at once when focus leaves the field.
- **flush**: at once, with every other field that has a pending commit and with the tag editor's pending edit (U193). The host flushes, and waits for the reply, before any toolbar, menu or keyboard command that acts on the note or leaves it (U192's commands, Cards…, Fields…, preview, Add), before it loads another note into the page, before it closes the editor, and when the scene moves to the background or resigns active (U151). A mode switch (H6) and collapsing a focused field (F3) first commit that field.

A field with no input since its last commit commits nothing on blur, flush, a mode switch or a collapse.

The page sends each commit except a flush's as B2's `commit` message, whose `reason` names its trigger: `idle`, `blur`, `modeSwitch` (H6) or `collapse` (F3). A flush's commits come back only in the `flush` reply (B3), never as `commit` messages.

For each commit the host computes the field's target (H2) and compares it with the value it last sent for that field, which starts as its baseline's stored HTML and is reset to it by `setField` (B3). When they are equal nothing is sent; otherwise the field goes into a patch (S2) in edit and Browse modes, or into the session's draft in Add mode.

## S2. The field patch (R821, R823, R1834)

In edit and Browse modes each commit, and each flush reply as a whole, is sent as one field patch:

| Member | Content |
|---|---|
| note id | the note being edited |
| note type id | as the note was loaded |
| field layout | as the note was loaded: for each field of the note type, in ordinal order, its name and its `id` when the field has one (`notetypes/records` N2, R905) |
| fields | the (field ordinal, target) pairs whose targets differ from the last sent values |
| tags | the whole tag list, only when the tag editor changed it (U193) |

A patch with neither fields nor tags is not sent.

- Decided 2026-10-27: the patch is a second input form of U27's note update (`notes/note-records` U1), not a new op: same catalog entry and undo class (`engine/op-model` O9), same step name. U190 adds it to U27's note update in the engine crate and exposes it through the FFI crate with the whole-note form; emulating it in Swift (read the note, then send the whole note) is not allowed, since that is two transactions and loses another writer's change. D1 records the follow-ups.
- The op applies a patch in its own write transaction, reading the live rows there (`engine/op-model` O7, re-derived). It refuses and writes nothing:
  - with `NotFound` when the note no longer exists (the kind U1 gives a deleted note);
  - with `Stale` when the note's `mid` is not the patch's note type id, or when the note type's field layout (its fields' names and `id`s in ordinal order, so their count too) differs from the patch's. A reorder, an added or removed field and a rename all refuse, so a patch never writes a field by an ordinal that now means another field;
  - with `InvalidArgument` when a listed ordinal is outside the layout, or when the stored row's field count differs from the note type's (a row only `notes/note-records` F1 repairs), as U1 refuses a whole-note input of the wrong count.
- Otherwise it replaces the listed fields, keeps every other field and, without a tags member, the stored tags. It then saves as `notes/note-records` U1 to U3 say: N2 for every field, nothing written when the result equals the stored row (U2), stamps and card generation (U3). A field the patch does not list therefore keeps what another writer stored after the editor loaded it.
- The op's source is `UserUndoable`, its step name "Update Note", and it names no review session, from every host (the review panel U184, Browse U221, the edit flows U196). Consecutive patches of one note thus merge into one undo step as `engine/undo` M1 and A3 say: only while each changes that note row and nothing else, within 60 seconds of the step's first op, and while the step is still local. A patch that registers a tag or generates a card records a step of its own, and no later patch merges into it (R821, R396).
- Patches go to the interactive queue (`engine/op-model` O2) in commit order and the host does not wait for them, so typing never waits on a save (R1834). The host waits only for the patches of a background flush, which must commit before the app suspends (R820, U151).
- Decided 2026-10-27: a field's last sent value becomes its target when the host submits the patch, so a later commit, made while that patch is still queued, is compared with the queued value. A field that later becomes unchanged again sends its baseline's stored HTML, so the stored bytes come back.
- When a patch is refused (`NotFound` or `Stale`), each of its fields gets back, as its last sent value, the target of that field's last patch that committed, or its baseline's stored HTML when none did; its target stays in the session. The refusal is reported to the host's owner: U196 closes the editor for a deleted note and reloads for a changed note type, saving pending edits first (R822). A refused patch is never sent again for another note.
- The `Changes` events of the editor's own patches (`engine/change-stream` S3, matched by op seq) do not reload the editor; U196 specifies the matching.
- Add mode sends no op: the draft keeps the targets and U195's add op takes them. Content mode (U220) binds no note, has one field and no tags, and never sends an op; its flush returns that field's target.

## S3. Field check updates (R807, R812)

- After loading, and after each commit, the host runs `notes/field-check` on the draft on the query lane: each field's target as of its last commit (its baseline's stored HTML for unchanged fields), and the note id in edit and Browse modes (none in Add mode). Content mode runs no check.
- Every request carries a counter; an answer is shown only when no later request was sent.
- `duplicate` marks the first field and offers Show Duplicates (R807). The other states except `normal` show their hint (R812). `normal` clears the mark and the hint. What each state refuses and its wording are U195's and U196's (`notes/field-check` K5).

## F1. The field list (R825, R833)

- One entry per field, in note type field order, then the tag editor (U193), which content mode leaves out.
- In a note type whose `original_stock_kind` is Image Occlusion, the fields whose `tag` is 0 (Occlusion) and 1 (Image) are not shown (`notetypes/stock-kinds` K4, R833); a note type without such tags hides nothing. Hidden fields are never loaded into the page, committed or patched; the mask editor writes them (U206).
- Each entry has the field name as its label, the input, and a mode button (H6). Accessibility identifiers: `editor.field.<ord>`, `editor.field.<ord>.label`, `editor.field.<ord>.htmlToggle`.
- The field's `description` is its placeholder: shown as plain text (never parsed as HTML), in rich mode only, while the field's target would be empty, in the field's font size, in the secondary label color, on one line truncated at its end.

## F2. Font, size and direction (R828)

- Font: `font_name` as a CSS font family, quoted unless it consists only of lower-case ASCII letters and hyphens (a generic family such as `serif`), followed by the system font as fallback. An empty name gives the system font. A name that ends in ` L` (space, capital L) is used with ` Light` in place of that ending, as Anki does: `Example Sans L` gives `Example Sans Light`. The stored name is not changed.
- Size: `font_size` CSS pixels, scaled by the Dynamic Type factor (1 at the default text size). Decided 2026-10-27: a stored size of 0 uses 20, the default of a new field (`notetypes/records` N5.2), since 0 would hide the text. The placeholder uses the same size.
- Direction: `rtl` makes the rich content's base direction right to left. HTML mode and the label keep the interface direction.
- These options act in the editor only. Nothing is written into field HTML, and card rendering does not read them (R828).

## F3. Collapsing and mode memory (R826, R827)

- A field starts collapsed when `collapsed` is set, and in HTML mode when `plain_text` ("Use HTML editor by default") is set.
- Clicking or tapping the label collapses or expands the field. Expanding by the label focuses the field with its caret restored (F4). Collapsing the focused field commits it first.
- A collapsed field shows only its label, is skipped by Tab, and ignores drops (R877, U197).
- Decided 2026-10-27: the collapsed state and the mode of each field are remembered per note type id for the app session, in all editor instances and scenes, by field position. The next note of that type loads with them while the type's field count equals the remembered count, and with the field options otherwise. They are never written to the collection, so they do not sync and are gone after relaunch.

## F4. Focus, Tab and caret (R248, R829)

- Tab moves to the next shown, expanded field and from the last one into the tag editor; Shift-Tab moves back. Shift-Tab from the first field and Tab out of the tag editor leave the page (`focusLeave`, B2).
- Each field keeps its selection when it loses focus. When Tab, Shift-Tab, its label or the host's `setFocus` moves focus back to it, that selection comes back; a click in its text places the caret at the click. A field focused for the first time gets the caret at its end.

## F5. Undo inside a field (R832)

Each field keeps its own edit history, apart from WebKit's: snapshots at edit boundaries, with consecutive typing grouped. ⌘Z and ⇧⌘Z with a field focused undo and redo that field only, and each is an input (S1). With no field focused they go to the collection undo (U159). Loading a note or `setField` clears the field's history.

## F6. Scripts in fields (R831)

While a field is edited, in either mode, no script element in it runs, no event handler attribute fires and no `javascript:` URL is followed. The stored HTML keeps all of them unchanged: an unchanged field keeps its bytes (H2), and a changed field keeps them through serialization (H3). Field content can send no bridge message, since no script of it runs.

## F7. Layout changes and the web view (R5)

- The session (targets, last sent values, baselines, selections, collapsed states and modes) lives above the layout. A layout switch moves the existing web view and never reloads it, so unsaved text and the caret survive (R5).
- When the page's web content process ends, input not yet committed is lost: at most one field's last 600 ms. The host reloads the page from the session. Each field whose last commit was changed gets that commit's target as `html` and its baseline as `baselineHtml` (B4), both as display HTML, so it starts changed and is still compared with its baseline (H2). Example: stored `A`, the user types to `AB`, the patch commits, the process ends; after the reload the user types `C` and deletes it, the commit is changed with target `AB`, equal to the last sent value, so nothing is sent; reverting to `A` makes the field unchanged and sends `A`.

## B1. Bridge messages

The page and its host exchange JSON objects through one message handler with replies, each with a `type` member naming one of the messages below. This is a data contract, version 1.

- The host rejects a message of another type, or one whose members do not match the tables, and logs it; the page does the same and answers the host's message with an error.
- `openNote` carries a `loadId`, increasing for the life of the web view. Every page message after it carries that `loadId`, and the host ignores messages of an older one. Since the host flushes before each load (S1), no edit is lost by this.
- Page messages arrive in the order the page sent them.
- `ord` is a field's ordinal in its note type.

## B2. Page to host

| Type | Members | Sent |
|---|---|---|
| `ready` | `protocol` (1) | once, when the runtime has loaded |
| `loaded` | `loadId` | when `openNote`'s fields are editable; the end of the "editor open→editable" signpost (R1833) |
| `focus` | `loadId`, `ord`, `mode` (`rich` or `html`) | a field got focus |
| `blur` | `loadId`, `ord` | a field lost focus |
| `dirty` | `loadId`, `ord` | the first input of a field after its last commit |
| `commit` | `loadId`, `ord`, `html`, `unchanged` (boolean), `reason` (`idle`, `blur`, `modeSwitch` or `collapse`) | S1, for every commit except a flush's; `html` is H3 steps 1 and 2 |
| `focusLeave` | `loadId`, `direction` (`next` or `previous`) | F4 |
| `collapsedChanged` | `loadId`, `ord`, `collapsed` | F3, from the label |
| `modeChanged` | `loadId`, `ord`, `mode` | H6 |
| `historyState` | `loadId`, `ord`, `canUndo`, `canRedo` | F5, when either changes, for the Edit menu |
| `contentHeight` | `loadId`, `height` (CSS pixels) | when the page's content height changes, for hosts that size the web view |

U192 adds the format state message, U193 the tag messages and U194 the image messages.

## B3. Host to page

| Type | Members | Reply |
|---|---|---|
| `openNote` | `protocol`, `loadId`, `mode` (`add`, `edit`, `browse` or `content`), `fields` (B4, shown fields only), `focus` (`ord` or null), `theme` (CSS custom properties), `options` (`closeHTMLTags`, and `shrinkImages` from `shrinkEditorImages`: the config keys of `editor/config-keys` K8) | `loadId`, once `loaded` is sent |
| `setField` | `loadId`, `ord`, `html` (display HTML of the field's newly stored value, U196) | replaces the content, makes that value the field's baseline so the field is unchanged (H2), clears the field's history. The host makes the same value the field's last sent value (S1) |
| `setFieldState` | `loadId`, `ord`, and `collapsed`, `mode` or both | done |
| `setFocus` | `loadId`, `ord` | done; the selection is restored as F4 says |
| `setMarks` | `loadId`, `duplicate` (boolean), `hint` (string or null) | done (S3) |
| `flush` | `loadId` | `commits`: one `{ord, html, unchanged}` per field committed (S1), in field order, and the tag editor's pending tags (U193) |
| `undo`, `redo` | `loadId`, `ord` | `applied` (boolean) |
| `setTheme` | `theme` | done |

U192 adds the command messages and U194 the insertion messages. A content change they make is an input of the focused field.

## B4. Field description

| Member | Content |
|---|---|
| `ord` | the field's ordinal |
| `name` | the field name, the label and accessibility label |
| `html` | the display HTML (H1) |
| `baselineHtml` | the baseline's display HTML (H2), present only when it differs from `html`: F7's reload of a changed field. The field then starts changed |
| `description` | F1 |
| `fontFamily`, `fontSize`, `rtl` | F2, already resolved |
| `collapsed`, `mode` | F3, already resolved |
| `clozeField` | whether the field is a cloze field (`notes/field-check` K2), for U192 |

## X1. Differences from Anki

- **Writes on open.** Anki's editor parses and serializes every field as it loads the note, passes every field through its HTML-mode input (always present, hidden while unused), which removes `<script>` and `<link>` elements, and its per-field timers then send the note back about 600 ms later. A field whose serialization differs from its stored bytes, or whose whole value is `<br>` or `<div><br></div>`, can so be rewritten without any typing. Meguri serializes only fields with an input (H1, H2; R823, AE40). Scenario `open-close-untouched` shows Anki's result; when its golden records a rewrite, it needs the deviation entry `editor-open-rewrites-fields`.
- **Whole-note saves.** Anki sends every field's current value with each save; Meguri sends only the fields whose targets changed (S2). With nothing else writing the note the stored results are the same; when another writer changed a field meanwhile, Meguri keeps that change.
- **Scripts.** Anki's editor removes `<script>` and `<link>` elements from every field when the note loads, and its timers then save the result; Meguri keeps them (H3, F6; R831).
- **MathJax.** Anki shows MathJax as typeset previews and, when such a field is saved, turns each `<br>` inside a MathJax span into a line feed and removes line feeds at the span's ends. Meguri leaves MathJax as text (H1; R563, R2017).
- **HTML mode.** Anki can show both inputs of a field at once and shows parsed, re-serialized source in its HTML input. Meguri shows one input at a time, and the stored bytes in HTML mode while the field is unchanged (H6).
- **Remembered states.** Anki remembers collapsed states and modes per note type in each editor window, and reuses collapsed states even when the field count changed. Meguri's memory spans the app session and needs an equal field count (F3).
- **Hints.** Anki keeps a cloze hint shown, even after the problem is fixed and across notes loaded into the same editor, until the editor is reopened; Meguri clears it at the next `normal` check (S3).
- **Font size 0.** Anki shows a field whose stored `font_size` is 0 at 0 px, and uses 16 px only when the field's options are missing; Meguri shows a stored 0 at 20 px (F2).
- **Descriptions.** Anki drops backslashes from a field description when showing it; Meguri shows the description as stored (F1).

## T1. Scenarios for the oracle maintainer

Requested in `conformance/scenarios/editor/field-html/`, with goldens in `conformance/goldens/editor/field-html/` marked `source: observed`. None is written or recorded yet: no runner step and no oracle driver op drives an editor page today. The oracle maintainer builds the quarantined image (E2) and the scenario vocabulary from this list.

**Setup.** One note type `Pair` defined by the scenario (R1959), standard, fields `Front` and `Back`, empty CSS, field options as each row says (default: N5.2's). One note, with the stored fields given; `Back` is empty unless given. Every probe reads the note's `flds` bytes and whether `mod` or `usn` changed since setup.

**Actions**, all as native key events on Anki desktop's Edit Current editor unless a row says Add: `open`; `caret F end|start|after "x"`; `type "…"`; `Return`; `Shift+Return`; `Backspace`; `select-all`; `toggle-html F`; `blur` (focus moves to the tag editor); `wait N` (ms); `probe`; `close`.

Return group (H4):

| Scenario | Front stored | Actions |
|---|---|---|
| `return-end` | `abc` | caret Front end, Return, type `d`, blur, probe |
| `return-end-no-text` | `abc` | caret Front end, Return, blur, probe |
| `return-middle` | `abcd` | caret Front after `ab`, Return, blur, probe |
| `return-start` | `abc` | caret Front start, Return, type `x`, blur, probe |
| `return-twice` | `a` | caret Front end, Return, Return, type `b`, blur, probe |
| `return-empty-field` | (empty) | caret Front end, type `a`, Return, type `b`, blur, probe |
| `return-bold-end` | `<b>abc</b>` | caret Front end, Return, type `d`, blur, probe |
| `return-bold-middle` | `<b>abcd</b>` | caret Front after `ab`, Return, blur, probe |
| `return-div` | `<div>a</div><div>b</div>` | caret Front after `a`, Return, type `x`, blur, probe |
| `return-list` | `<ul><li>a</li></ul>` | caret Front end, Return, type `b`, Return, Return, type `c`, blur, probe |
| `return-after-break` | `a<br>` | caret Front end, type `b`, blur, probe |
| `shift-return-end` | `abc` | caret Front end, Shift+Return, type `d`, blur, probe |
| `shift-return-end-no-text` | `abc` | caret Front end, Shift+Return, blur, probe |
| `shift-return-middle` | `abcd` | caret Front after `ab`, Shift+Return, blur, probe |
| `shift-return-div` | `<div>a</div>` | caret Front end, Shift+Return, type `b`, blur, probe |
| `return-html-mode` | `abc` | toggle-html Front, caret Front end, Return, type `d`, blur, probe |

Empty and trailing break group (H1, H3):

| Scenario | Front stored | Actions |
|---|---|---|
| `delete-all-plain` | `abc` | caret Front end, select-all, Backspace, blur, probe |
| `delete-all-lines` | `a` | caret Front end, Return, type `b`, select-all, Backspace, blur, probe |
| `delete-all-bold` | `<b>abc</b>` | caret Front end, select-all, Backspace, blur, probe |
| `delete-all-divs` | `<div>a</div><div>b</div>` | caret Front end, select-all, Backspace, blur, probe |
| `stored-br-type` | `<br>` | caret Front end, type `x`, blur, probe |
| `stored-div-br-type` | `<div><br></div>` | caret Front end, type `x`, blur, probe |
| `trailing-br-keep` | `a<br>` | caret Front start, type `x`, blur, probe |
| `trailing-double-br` | `a<br><br>` | caret Front start, type `x`, blur, probe |
| `block-then-br` | `<div>a</div><br>` | caret Front start, type `x`, blur, probe |

Space group (H5):

| Scenario | Front stored | Actions |
|---|---|---|
| `space-between` | (empty) | caret Front end, type `a b`, blur, probe |
| `space-trailing` | (empty) | caret Front end, type `a `, blur, probe |
| `space-double` | (empty) | caret Front end, type `a  b`, blur, probe |
| `space-triple` | (empty) | caret Front end, type `a   b`, blur, probe |
| `space-leading` | (empty) | caret Front end, type ` a`, blur, probe |
| `nbsp-stored` | `a&nbsp;b` U+00A0 `c` | caret Front end, type `x`, blur, probe |

Serialization and preservation group (H2, H3, F6, AE40):

| Scenario | Stored | Actions |
|---|---|---|
| `open-close-untouched` | Front `<span  class="x">a</span><br/>`, Back `t` | open, wait 1500, probe, close, probe |
| `edit-other-field` | the same | caret Back end, type `u`, blur, probe |
| `reserialize-touched` | Front `<span  class="x">a</span><br/>` | caret Front end, type `x`, blur, probe |
| `entities-touched` | Front `&eacute;&#31169;&amp;&lt;` | caret Front end, type `x`, blur, probe |
| `unbalanced-touched` | Front `<b>a` | caret Front end, type `x`, blur, probe |
| `script-untouched` | Front `<b>a</b><script>var n=1</script>`, Back `t` | open, wait 1500, probe, toggle-html Front, wait 1500, probe, close, probe |
| `handler-touched` | Front `<img src="a.png" onerror="f()">b` | caret Front end, type `x`, blur, probe |
| `type-then-revert` | Front `<span  class="x">a</span>` | caret Front end, type `x`, Backspace, blur, probe |
| `mathjax-touched` | Front `\(a<br>b\) \[<br>c<br>\]` | caret Front end, type `x`, blur, probe |
| `mathjax-untouched` | Front `\(a<br>b\)`, Back `t` | caret Back end, type `u`, blur, probe |
| `sound-touched` | Front `[sound:a b.mp3]x` | caret Front end, type `y`, blur, probe |
| `cloze-touched` | Front `{{c1::a::h}}` | caret Front end, type `x`, blur, probe |

Media names group (H1, H3, `text/media-references` M6 to M8):

| Scenario | Front stored | Actions |
|---|---|---|
| `media-space` | `<img src="a b.jpg">` | caret Front end, type `x`, blur, probe |
| `media-percent` | `<img src="100%25.jpg">` | caret Front end, type `x`, blur, probe |
| `media-non-ascii` | `<img src="日本.png">` | caret Front end, type `x`, blur, probe |

HTML mode group (H6), with Front's "Use HTML editor by default" set:

| Scenario | Front stored | Actions |
|---|---|---|
| `html-quotes-dashes` | (empty) | caret Front end, type `"a" -- b`, blur, probe |
| `html-toggle-no-edit` | `<span  class="x">a</span>` | toggle-html Front, toggle-html Front, blur, close, probe |
| `html-edit` | `<b>a</b>` | caret Front end, type `<i>c</i>`, blur, probe |

Timing and undo group (S1, S2):

| Scenario | Actions | Observed |
|---|---|---|
| `idle-commit` | caret Front end, type `a`, wait 300, probe, wait 400, probe | first probe unchanged, second saved |
| `idle-restart` | caret Front end, type `a`, wait 400, type `b`, wait 400, probe, wait 300, probe | saved only at the second probe |
| `blur-commit` | caret Front end, type `a`, wait 100, blur, probe | saved |
| `two-fields` | caret Front end, type `a`, wait 300, caret Back end, type `b`, wait 700, probe | both saved |
| `save-undo-step` | caret Front end, type `a`, wait 700, caret Front end, type `b`, wait 700, probe, and the undo status | one "Update Note" step (`specs/conformance/undo-ops.md` V4); the 60-second merge itself is already recorded through `edit_note` in `undo/edit-note-merge` |

## T2. Meguri tests without goldens

The plan's U190 test scenarios cover what no editor golden shows: AE40's row checks (no op on open and close; an untouched field byte-identical after another field's save), F6 with a handler that would post to the bridge, F4's Tab order and caret restore, F3's session memory and relaunch, H6's quotes and dashes on Mac and iOS, F2's `rtl` with the rendered card unchanged, F1's hidden Image Occlusion fields, F5's routing, S1's timings and background flush, S2's merge at 20 s and split at 61 s, F7's layout switch without reload and its reload after the web content process ends (`baselineHtml`, the example in F7), S2's refusals and the restored last sent values, `setField`'s new baseline, R1833's open time, and content mode. Each runs on macOS and on the iOS simulator.

## D1. Follow-ups in files this spec does not own

Decided 2026-10-27: these are owed by the owners of the named files and change nothing above.

- `notes/note-records` U1: add S2's field-patch input form next to the whole-note form, with S2's refusals (`NotFound`, `Stale`, `InvalidArgument`) and the same catalog entry and step name.
- The plan's U190 Files list: add the patch form to U27's note update module in the engine crate and to the FFI crate's note ops, with engine tests for each S2 refusal (a reorder that keeps the field count included) and for an unlisted field that another writer changed.
- `editor/paste` P1 still names the oracle by its package directory; it should say "the Python-library oracle", as E2 does.
