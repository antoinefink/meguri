# Field HTML, field list and save pipeline

Spec: editor/field-html
Requirements: R248, R254, R820, R821, R823, R825, R826, R827, R828, R829, R830, R831, R832, R833, R5 (editor part), R988 (editor side); supports R396, R563, R807, R812, R822, R877, R1833, R1834, R1960, R2017; AE40
Anki: 26.09.3
Scenarios: conformance/scenarios/editor/field-html/

The note editor component (U190) that Add (U195), the edit flows (U196, U184), the Browse editor (U221), the Edit Field sheet (U220) and the image-occlusion flows reuse: what field HTML it shows and what it writes back, when a change is saved and as which op, the field list, and the messages between the editor page and its native host. Formatting and cloze tools (U192), the tag editor (U193), media insertion and image display (U194), paste and drop (U197) and what the editor does when another writer changes the note (U196, R822) build on it: `editor/tag-editor`, `editor/paste`, `editor/formatting`, `editor/cloze-wrap`, `editor/add-flow` and `editor/edit-flows`.

Meguri's editor page is Meguri's own: no Anki editor script, style or asset is used in the app or in its tests (R1960).

## E1. Terms

- **Stored HTML**: a field value as the note row holds it (`notes/note-records` N1).
- **Display HTML**: the stored HTML with local media names percent-encoded for the web view (`text/media-references` M6, R988). The page only ever receives display HTML.
- **Rich mode** and **HTML mode**: the two ways a field is edited, a rich-text editable area or a plain source text area (R827). A field shows one of them at a time.
- **Content**: what the field holds in the page now: the editable area's DOM in rich mode, the text area's text in HTML mode.
- **Serialization**: the HTML fragment serialization algorithm of the HTML Standard, as the page's web engine applies it to the editable area's children (in HTML mode, to the nodes parsed from the text, H3 step 1).
- **Block element**: an element whose tag is one of `address`, `article`, `aside`, `blockquote`, `details`, `dialog`, `dd`, `div`, `dl`, `dt`, `fieldset`, `figcaption`, `figure`, `footer`, `form`, `h1` to `h6`, `header`, `hgroup`, `hr`, `li`, `main`, `nav`, `ol`, `p`, `pre`, `section`, `table`, `ul`, or any element with a `block` attribute whose value is not `false`.
- **Inline-only**: content in which no element, at any depth, is a block element. Empty content is inline-only.
- **Input**: a change of a field's content made by the user: typing, deleting, Return, an undo or redo inside the field (F5), a formatting or cloze command (U192), an insertion (U194), a paste or drop (U197). Focus, caret and selection moves, scrolling, collapsing or expanding, and switching between rich and HTML mode are not inputs.

## E2. Sources and goldens

Decided 2026-10-27, for every editor spec: editor behaviour that the Python-library oracle cannot observe is specified from Anki 26.09.3 in this file's own words. Its goldens are observed by an oracle maintainer who runs Anki 26.09.3's own editor page inside a quarantined oracle image, as `stats/graphs-common` G12 decision 2 does for the graphs page. The page and its assets never leave that image; goldens hold observed values only; Meguri's tests and app never load any of it (R1960). This replaces the plan's U190 note that a spec writer records the editor goldens once by hand.

- The page runs in the web engine of the pinned Anki desktop build (its QtWebEngine), because Return and space handling are that engine's defaults. Key presses reach it as native key events, never as script edits of the DOM.
- Anki 26.09.3 has two editors, which save differently. Goldens come from its default editor, as `editor/paste` P1 follows: the experimental editor option off, and Edit Current and Add opened without Shift held (Shift opens the other editor).
- How a golden carries this. As for the graphs page (`oracle/graphs-page/`), goldens are written only by `tools/mg oracle record`, which routes the editor scenarios to the quarantined image. They are ordinary goldens of `conformance/schema/golden.schema.json`, with no extra marker: the observation kind that drives the page returns, beside the probe's values, the engine's name and version and the editor variant used (default editor, no Shift), and the golden's `oracle` member pins the base lock and the driver (`lock_sha256`, `driver_sha256`, the driver covering the page runner). The desktop package's own lock is pinned as the graphs page's is, by the rule or golden member that `oracle/graphs-page/README.md` asks for. The kind and its image are the oracle maintainer's to add.
- Goldens are authoritative for the markup that keystrokes produce (H4, H5). Every other rule in this file is normative as written, and a golden that contradicts one is a finding against this file.
- Automated Meguri tests compare Meguri's runtime, in WKWebView on macOS and the iOS simulator, only with these goldens and with this file.

## H1. Loading a field

1. The host gives the page each shown field's display HTML (B4).
2. Rich mode: the page parses the display HTML as an HTML fragment with a body-like context (as setting `innerHTML` on a `div` does). When the parsed content is inline-only, the page appends one `<br>` element after it. This makes a final line break of the stored HTML show as an empty last line on which the caret can sit, and gives an empty field a line to type on. H3 step 1 takes it out again. This parse with the appended `<br>` is the **load parse**; H2 and H6 use it too.
3. HTML mode: the text area holds the display HTML verbatim, unparsed.
4. MathJax (`\(…\)`, `\[…\]`), sound tags (`[sound:…]`), cloze markup and template-like text are ordinary text in both modes: nothing is typeset, replaced or decorated (R563, R2017).
5. Nothing in field content runs (F6). Relative `src` names load from the collection's media folder.
6. Loading writes nothing. A field that receives no input is never serialized, so opening a note and closing it without input writes nothing (R823, AE40).
7. Decided 2026-10-27: a note whose stored row holds a different number of field values than its note type has fields (`notes/note-records` N1; a row that only F1 repairs, for example one synced from another client) is not loaded into the page, since S2 would refuse every patch to it. In place of its fields the editor shows that the note needs Check Database (U128), with a button that starts it, and writes nothing.

## H2. Changed and unchanged fields

- A field's **baseline** is a stored HTML value: the one the field was loaded with, until the host's `setField` (B3) replaces it with the value that message carries. The page holds it as display HTML: B4's `baselineHtml` when the load gives one, otherwise the loaded `html`.
- A field loaded without `baselineHtml` and with no input since is **unchanged**. A field loaded with `baselineHtml` (F7) starts **changed**, and so does any field after an input; it stays changed until a commit (S1) finds it equal to its baseline again:
  - in rich mode, when H3 steps 1 and 2 of its content equal the same steps applied to the load parse (H1 step 2) of the baseline's display HTML (the page may compute the baseline's value just before applying the first input);
  - in HTML mode, when the text area's text equals the baseline's display HTML.
- The page alone decides whether a field is unchanged, always against its baseline, and reports it with each commit (`unchanged`, B2, B3).
- A field's **target** is the value the editor would store: for an unchanged field its baseline's stored HTML, byte for byte (R823); for a changed field the result of H3.
- So undoing every edit inside a field (F5) makes it unchanged again, and its stored bytes come back even when the engine would have serialized them differently.

## H3. Serializing a changed field

The steps, in this order, on the field's content:

1. Rich mode: serialization of the content. When the content is inline-only and its last child node is a `<br>` element, that `<br>` is left out. HTML mode: the text is parsed as an HTML fragment as H1 step 2 parses display HTML, but in an inert document (nothing in it runs or loads, F6) and with no `<br>` appended, and the parsed nodes are serialized. Decided 2026-10-27: HTML-mode text is stored parsed, as Anki's HTML input stores it, so `<b>x` is written `<b>x</b>`, a stray end tag is dropped, and a field never stores markup that is not balanced; only `<script>` and `<link>` differ (X1).
2. Rich mode: every `data-meguri-editor` attribute is removed. It is the only attribute the runtime may add to elements inside field content (U194 keeps an image's display size state in it), and the runtime adds no element other than H1's `<br>`.
3. Every U+0000 is removed.
4. Local media names are decoded (`text/media-references` M7), undoing H1's display encoding. Decided 2026-11-10 (data safety first): each `"` that decoding puts into a name is written `&quot;` instead, since step 1 writes every attribute value in double quotes and a raw `"` would end the value. So the stored `<img src='q"r.png'>`, shown as `<img src='q%22r.png'>` (M6), is written `<img src="q&quot;r.png">` once the field is changed. Its decoded name is still `q"r.png` (M4), and later displays and saves leave it as it is. Anki writes `<img src="q"r.png">`, whose reference reads `q` (X1).
5. When step 2's result is exactly `<br>` or exactly `<div><br></div>`, the result is the empty string (R830). The test is made on step 2's result, before steps 3 and 4, in Anki's default editor's order: `<br>` followed by U+0000 is stored as `<br>`. No other form is emptied here: `<div></div>`, `<br><br>`, ` ` and `&nbsp;` stay as they are. Whether a field counts as empty for the Add checks is `text/field-emptiness` and `notes/field-check` K3.

The core's save normalization (`notes/note-records` N2) then runs on every field of the note.

Step 1's serialization rewrites the whole field, the parts the user did not edit included. Anki's editor parses and serializes the same way, so these rewrites are the same in both (examples, rich mode):

| Content as loaded | After an input at its end and step 1 |
|---|---|
| `<span  class="x">a</span><br/>` | `<span class="x">a</span><br>` and the input after it: at the end, the caret is on the empty last line that H1 step 2's appended `<br>` makes, so typing `x` gives `<span class="x">a</span><br>x` |
| `<img src='a.png'>` | `<img src="a.png">` and the input |
| `<B>a` | `<b>a</b>` with the input inside or after it, as the caret was |
| `&eacute;&#31169;` | `é私` |
| U+00A0 or `&#160;` | `&nbsp;` |
| `a` CR LF `b` | `a` LF `b` |
| `<!-- c -->`, `onerror="…"` | kept as they are |

Meguri also keeps `<script>…</script>` and `<link>` elements as they are through step 1, in both modes. Anki does not: it removes them from every field when the note loads and from HTML-mode text at each change (X1, R831).

Where WebKit's serializer and the pinned engine's write a scenario's input differently, Meguri's runtime corrects its output to the golden; a difference it cannot correct becomes a deviation entry.

## H4. Return and Shift-Return (R830)

- In rich mode, Return starts a new line as a new block (the pinned engine's line element is `div`) and Shift-Return inserts a `<br>` line break, both at the caret, after replacing a non-empty selection. In a list item, Return starts a new item, and Return in an empty last item leaves the list.
- The exact markup is what the goldens of T1's Return group record. Meguri's runtime handles the paragraph and line-break inputs (`beforeinput` with `insertParagraph` and `insertLineBreak`) itself instead of WebKit's default, and for each scenario its H3 result must equal the golden byte for byte. Its rule for cases no scenario covers is the general rule those goldens show; a later golden that contradicts the rule changes the rule.
- Provisional until T1's goldens land; a golden overrides any row. No T1 golden is recorded yet (T1), so until then Meguri's runtime and tests take these H3 results, derived from the first bullet, H1 step 2 and H3:

  | T1 row | Front stored | Expected H3 result |
  |---|---|---|
  | `return-end` | `abc` | `abc<div>d</div>`: the first line stays bare, the new line is a `div` |
  | `return-end-no-text` | `abc` | `abc<div><br></div>` (the content is not inline-only, so H3 step 1 keeps the `<br>`) |
  | `shift-return-end` | `abc` | `abc<br>d`: the line break goes before H1's appended `<br>`, which H3 step 1 then drops |
  | `shift-return-end-no-text` | `abc` | `abc<br>`: two `<br>` elements in the page, the final one dropped by H3 step 1 |

  The empty and trailing break group needs no such table: H1 step 2 and H3 steps 1 and 5 decide it.
- Return never inserts a line feed character into rich content.
- In HTML mode, Return and Shift-Return insert one line feed (U+000A) into the text.
- Add shortcuts (⌘Return, U195) never insert anything.

## H5. Spaces

- In rich mode, a typed space that HTML would collapse or hide (at the start or end of a line, or next to another space) is held as U+00A0, so H3 writes it as `&nbsp;`. The pinned engine rebalances a run of spaces while the user types: a trailing U+00A0 becomes an ordinary space once a character is typed after it, and a run alternates the two. The goldens of T1's space group give the exact patterns; Meguri's runtime reproduces them.
- Provisional until T1's space goldens land; a golden overrides any row. `space-between` gives `a b`, `space-trailing` `a&nbsp;`, and `space-leading` `&nbsp;a`. For runs of two or more spaces between characters (`space-double`, `space-triple`) no pattern is fixed before the golden: Meguri's runtime leaves typed spaces to WebKit's own editing, which shares the pinned engine's rebalancing of whitespace, and its tests check only that the run holds as many U+00A0 and space characters together as were typed, at least one U+00A0, and no two ordinary spaces in a row.
- In HTML mode, a space is an ordinary space.
- U+00A0 characters and `&nbsp;` that were already in a changed field are written `&nbsp;`; in an unchanged field they stay as stored.

## H6. HTML mode (R254, R827)

- ⇧⌘X, or the field's mode button, switches the focused field between rich and HTML mode. Changed 2026-11-19 (F1): there is no button per field. The one HTML mode control of F1 does what ⇧⌘X does, on the same field.
- Entering HTML mode, the text area gets its baseline's display HTML verbatim when the field is unchanged, and otherwise H3 steps 1 and 2 of the rich content, so the names stay display-encoded as in H1 and the same steps 3 to 5 apply to both modes.
- Leaving HTML mode, the rich content is the load parse (H1 step 2) of the text, the appended `<br>` included, so a final `<br>` of the text survives H3 step 1: `a<br>` gives `a<br><br>` in the page and is written `a<br>`.
- A switch is not an input: switching an unchanged field back and forth leaves it unchanged. A pending commit of the field is made first (S1).
- The text area has autocorrect, spell checking, auto-capitalization, smart quotes and smart dashes off: typing `"` stores U+0022 and `--` stores two hyphens, on Mac and on iOS.
- The "Close HTML tags" option (`closeHTMLTags`, U192) closes a tag as it is typed; U192 specifies it.
- Tab in the text area moves focus (F4); it inserts no tab character.

## S1. Commits (R820)

A commit is the page handing one field's H3 steps 1 and 2 result to the host, with whether the field is now unchanged (H2). A field commits:

- **idle**: 600 ms after its last input. Each input restarts that field's timer. Every field has its own timer, so a commit of one field never cancels another field's pending commit.
- **blur**: at once when focus leaves the field.
- **flush**: at once, with every other field that has a pending commit and with the tag editor's pending edit (`editor/tag-editor` G11). The host flushes, and waits for the reply, before any toolbar, menu or keyboard command that acts on the note or leaves it (U192's commands, Cards…, Fields…, preview, Add), before it loads another note into the page and before it closes the editor (these two with `hold`, B3), and when the scene moves to the background or resigns active (U151). A mode switch (H6) and collapsing a focused field (F3) first commit that field.

A field with no input since its last commit commits nothing on blur, flush, a mode switch or a collapse.

A flush's commit of a field carries the field's content at the moment the flush runs. An input that arrives after that moment counts as input since the last commit. This holds even while the host still waits for the flush's reply or its patches: a flush without `hold`, such as Sync Now's (`editor/edit-flows` W6), lets typing go on. Such an input starts the field's idle timer like any other input. The end of the earlier flush never cancels that timer, so the next idle, blur or flush trigger commits the input, and it is never lost. Example: type `a`, flush without `hold`, then type `b` and, within 600 ms of `b`, type `c`, all before the flush's reply arrives. The flush carries `a`, and the one commit after it carries `abc`.

The page sends each commit except a flush's as B2's `commit` message, whose `reason` names its trigger: `idle`, `blur`, `modeSwitch` (H6) or `collapse` (F3). A flush's commits come back only in the `flush` reply (B3), never as `commit` messages.

For each commit the host computes the field's target (H2), which becomes the field's session target. Then, by mode:

- Edit and Browse: the host compares the target with the value it last sent for that field, which starts as its baseline's stored HTML and is reset to it by `setField` (B3). When they are equal nothing is sent; otherwise the field goes into a patch (S2).
- Add: the target replaces the field's value in the session's draft, with no comparison, so a field edited back to its baseline (empty, or its sticky value) holds that value in the draft again.
- Content (U220): the target replaces the session's value of its one field. Content mode's result is that value, or its baseline's stored HTML when no commit happened.

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

- Decided 2026-10-27: the patch is a second input form of U27's note update (`notes/note-records` U1), not a new op: the same plain op (not listed in `engine/op-model` O9, which lists only long writes and maintenance tasks), with source `UserUndoable` and recorded as an undoable step (`engine/undo` M1), and the same step name. U190 adds it to U27's note update in the engine crate and exposes it through the FFI crate with the whole-note form; emulating it in Swift (read the note, then send the whole note) is not allowed, since that is two transactions and loses another writer's change. D1 records the follow-ups.
- The patch may name the collection generation the editor loaded the note in (decided 2026-11-20; the review's Edit Note and every host built on the editor draft name it). The op then resolves the collection once and refuses with `StaleGeneration` when that collection's generation differs, as Add's screen entry does, so a draft is never written into a collection that replaced the one it was loaded from.
- The op applies a patch in its own write transaction, reading the live rows there (`engine/op-model` O7, re-derived). It refuses and writes nothing:
  - with `NotFound` when the note no longer exists (the kind U1 gives a deleted note);
  - with `Stale` when the note's `mid` is not the patch's note type id, or when the note type's field layout (its fields' names and `id`s in ordinal order, so their count too) differs from the patch's. A reorder, an added or removed field and a rename all refuse, so a patch never writes a field by an ordinal that now means another field;
  - with `InvalidArgument` when a listed ordinal is outside the layout, or when the stored row's field count differs from the note type's (a row only `notes/note-records` F1 repairs), as U1 refuses a whole-note input of the wrong count.
- Otherwise it replaces the listed fields, keeps every other field and, without a tags member, the stored tags. It then saves as `notes/note-records` U1 to U3 say: N2 for every field, nothing written when the result equals the stored row (U2), stamps and card generation (U3). A field the patch does not list therefore keeps what another writer stored after the editor loaded it.
- The op's source is `UserUndoable` and its step name "Update Note". It names no review session, except from the review's Edit Note (`review/edit-current` E2, decided 2026-11-20): there the patch carries the open session's id, the review admits it as it admits a card action (the open session only; its redo list cleared), and the step records that session, so the review's Undo covers it. Browse U221 and the edit flows U196 pass none; as for any user op, the writer still records the session that is open, if any. Consecutive patches of one note thus merge into one undo step as `engine/undo` M1 and A3 say: only while each changes that note row and nothing else, within 60 seconds of the step's first op, and while the step is still local. A patch that registers a tag or generates a card records a step of its own, and no later patch merges into it (R821, R396).
- Patches go to the interactive queue (`engine/op-model` O2) in commit order and the host does not wait for them, so typing never waits on a save (R1834). The host waits only for the patches of a background flush, which must commit before the app suspends (R820, U151), and of the flush it makes before loading another note or closing the editor (below).
- The patch op asks to be refused, never parked, while the collection is unavailable (`engine/lifecycle` L4), so it never runs on another generation; it then fails with `CollectionUnavailable`.
- Decided 2026-10-27: a field's last sent value becomes its target when the host submits the patch, so a later commit, made while that patch is still queued, is compared with the queued value. A field that later becomes unchanged again sends its baseline's stored HTML, so the stored bytes come back.
- Decided 2026-10-27: when a patch fails, whatever its result (`NotFound`, `Stale`, `InvalidArgument`, `CollectionUnavailable`, `StaleGeneration`, a storage error such as a full disk, or a panic), each of its fields gets back, as its last sent value, its **last committed value**: the target of that field's last patch that committed, or its baseline's stored HTML when none did. Its session target and its content on screen stay.
- Every failure is shown to the user at once, never silently, in the wording of `editor/edit-flows` W9: `NotFound` says the note was deleted, and U196 then closes the editor (R822, `editor/edit-flows` W5.4); `InvalidArgument` says the note needs Check Database (U128); `Stale` is handled as below; any other failure says the change was not saved and keeps the text in the editor.
- Each flush also puts into its patch every field whose session target differs from its last sent value, even with no input since its last commit, so a field whose patch failed is sent again at the next flush.
- When a patch of the flush made before loading another note or closing the editor fails, the host neither loads nor closes, except for a deleted note: it reports the failure, sends `release` (B3) and keeps the note and its text in the editor. A background flush's failure is reported when the scene is active again.
- After `Stale`, the host reads the note and its note type again and reloads the editor without losing text (R822). Each field whose session target differs from its last committed value is matched to a field of the new layout: the one with the same `id`, or the one with the same name when the old field had no `id`, and none when the note's `mid` is no longer the patch's note type id. Each matched field loads with its session target as `html` and the newly stored value as `baselineHtml` (B4), so it starts changed (H2), and its last sent value is the newly stored value. The host then sends at once one patch on the new layout with those targets; when it fails, it is handled as any failure above, with no further automatic retry. A field with no match is shown to the user with its text, which can be copied, and is never dropped silently. A rename alone is therefore saved with no text lost.
- A refused patch is never sent again for another note.
- The `Changes` events of the editor's own patches (`engine/change-stream` S3, matched by op seq) do not reload the editor; `editor/edit-flows` W1 specifies the matching: while an own patch is in flight the host holds the note's `Changes` events, then skips those whose seq is an own patch's.
- Add mode sends no op: the draft holds the targets (S1) and U195's add op takes them. Content mode (U220) binds no note, has one field and no tags, and never sends an op; after its flush, its result is the value S1 keeps.

## S3. Field check updates (R807, R812)

- After loading, and after each commit, the host runs `notes/field-check` on the draft on the query lane: each field's target as of its last commit (its baseline's stored HTML for unchanged fields), and the note id in edit and Browse modes (none in Add mode). Content mode runs no check.
- Every request carries a counter; an answer is shown only when no later request was sent.
- `duplicate` marks the first field and offers Show Duplicates (R807). The other states except `normal` show their hint (R812). `normal` clears the mark and the hint. What each state refuses and its wording are U195's and U196's (`notes/field-check` K5).

## F1. The field list (R825, R833)

- One entry per field, in note type field order, then the tag editor (U193), which content mode leaves out.
- In a note type whose `original_stock_kind` is Image Occlusion, two fields are not shown (`notetypes/stock-kinds` K4, R833), found as Anki finds them: when fields with `tag` 0, 1, 2 and 3 (Occlusion, Image, Header, Back Extra) all exist, the fields whose `tag` is 0 and 1; otherwise, when the note type has at least four fields, ordinals 0 and 1; otherwise ordinal 0 only. Decided 2026-10-27: Meguri follows Anki here, so a note type that lost its tags (Check Database restores them) still never shows the occlusion data as an editable field. Hidden fields are never loaded into the page; the session writes them as host fields, which S1 and S2 commit and patch like page fields (`editor/image-occlusion-notes` O6.1). Which view an image-occlusion session shows (mask editor or fields) is `editor/image-occlusion-notes` O3; a stock-kind-6 note type with fewer than four fields gets no mask editor and shows `editor/image-occlusion-notes` O2's notice.
- Each entry has the field name as its label, the input, and a mode button (H6). Accessibility identifiers: `editor.field.<ord>`, `editor.field.<ord>.label`, `editor.field.<ord>.htmlToggle`.
- Changed 2026-11-19 (Antoine; `docs/design/native-interface-contract.md` N17.1): an entry has its label and its input, and no mode button. The editor has **one HTML mode control**, outside the field list:
  - It acts on the field that has focus; when no field has focus, on the field of this editor that last had it. Until a field has had focus it is disabled.
  - Touch: it is in the keyboard accessory bar, at its leading end, so it shows whenever a field is being edited. Mac: it is in the editor's bottom bar, at its leading end.
  - It is shown selected while the field it acts on is in HTML mode, and its accessibility value says so. Its label is "HTML Editor" (`editor/formatting` F19's name for the command).
  - It carries the identifier `editor.field.<ord>.htmlToggle` of the field it acts on, so one element holds that identifier at a time. While no field has had focus the Add screen's control is `editor.add.htmlToggle`.
  - Activating it is H6's switch: a pending commit first, the caret into the text area or back, no input recorded. It never moves focus to another field.
  - A collapsed field cannot have focus (F3), so the control never acts on one. Each field's mode is still its own, remembered as F3 says; the control only shows and changes the mode of one field at a time.
  - H4 is unchanged: Return and Shift-Return belong to the field in both modes, and ⌘Return adds.
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

Each field keeps its own edit history, apart from WebKit's: snapshots at edit boundaries, with consecutive typing grouped. ⌘Z and ⇧⌘Z with a field focused undo and redo that field only, and each is an input (S1). When the focused field has nothing to undo (or redo), ⌘Z (or ⇧⌘Z) and Edit › Undo (Redo) go to the collection undo (U159), so after an add, whose next draft's fields load with empty histories, ⌘Z in the first field undoes the add (`editor/add-flow` A5); the Edit menu's title follows the target (the field's history while it can undo, else the collection's step name). With no field focused they go to the collection undo (U159). Loading a note or `setField` clears the field's history. Decided 2026-10-27: the history holds snapshots of the content in the field's current mode, and a mode switch (H6) clears it, since the two modes edit different things; the page then sends `historyState`.

## F6. Scripts in fields (R831)

While a field is edited, in either mode, no script element in it runs, no event handler attribute fires and no `javascript:` URL is followed. The stored HTML keeps all of them unchanged: an unchanged field keeps its bytes (H2), and a changed field keeps them through serialization (H3). Field content can send no bridge message, since no script of it runs.

## F7. Layout changes and the web view (R5)

- The session (targets, last sent values, baselines, selections, collapsed states and modes) lives above the layout. A layout switch moves the existing web view and never reloads it, so unsaved text and the caret survive (R5).
- When the page's web content process ends, input not yet committed is lost: at most one field's last 600 ms. The host reloads the page from the session. Each field whose last commit was changed gets that commit's target as `html` and its baseline as `baselineHtml` (B4), both as display HTML, so it starts changed and is still compared with its baseline (H2). Example: stored `A`, the user types to `AB`, the patch commits, the process ends; after the reload the user types `C` and deletes it, the commit is changed with target `AB`, equal to the last sent value, so nothing is sent; reverting to `A` makes the field unchanged and sends `A`.

## F8. Styles in fields (R831)

Decided 2026-10-27: in rich mode each field's editable area is the content of its own shadow root, as in Anki's editor. A `<style>` element in field content, or a `<link rel="stylesheet">` that Meguri keeps and Anki removes (X1), styles that field's content only; it never affects the page chrome (labels, placeholders, mode buttons, the tag editor) or other fields. F2's font, size and direction are set on the shadow root's host. In HTML mode the text area shows the markup as text, so it styles nothing. Nothing of this changes what is stored (H2, H3).

## B1. Bridge messages

The page and its host exchange JSON objects through one message handler with replies, each with a `type` member naming one of the messages below. This is a data contract, version 1.

- The host rejects a message of another type, or one whose members do not match the tables (besides `seq`, below), and logs it; the page does the same and answers the host's message with an error.
- `openNote` carries a `loadId`, increasing for the life of the web view. Every page message after it carries that `loadId`, and the host ignores messages of an older one. Since the host flushes with `hold` before each load (S1, B3), no input falls between that flush and the load, and no edit is lost by this.
- Every page message after `openNote` and every `flush` reply also carries `seq`, an integer that starts at 1 for each `loadId` and increases by one with each. The host handles them in `seq` order and holds one that arrives before its predecessor, so a `flush` reply is never handled before a `commit` the page sent earlier.
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
| `flush` | `loadId`, `hold` (boolean) | `commits`: one `{ord, html, unchanged}` per field committed (S1), in field order, and the tag editor's pending tags (U193). With `hold` true the page, once it has answered, accepts no input (keys, paste, drop, commands) until the next `openNote` or `release` |
| `release` | `loadId` | done; ends a flush's hold (S2); also sent by Browse after a change that keeps the same note in the pane (`editor/edit-flows` W2) and by the Add screen when an add or close stops (`editor/add-flow` A5, A7) |
| `undo`, `redo` | `loadId`, `ord` | `applied` (boolean) |
| `setTheme` | `theme` | done |

U192 adds the command messages and U194 the insertion messages. A content change they make is an input of the focused field.

## B4. Field description

| Member | Content |
|---|---|
| `ord` | the field's ordinal |
| `name` | the field name, the label and accessibility label |
| `html` | the display HTML (H1) |
| `baselineHtml` | the baseline's display HTML (H2), present only when it differs from `html`: F7's reload of a changed field, and S2's reload after `Stale`. The field then starts changed |
| `description` | F1 |
| `fontFamily`, `fontSize`, `rtl` | F2, already resolved |
| `collapsed`, `mode` | F3, already resolved |
| `clozeField` | whether the field is a cloze field (`notes/field-check` K2), for U192 |

## X1. Differences from Anki

- **Writes on open.** Anki's editor parses and serializes every field as it loads the note, passes every field through its HTML-mode input (always present, hidden while unused), which removes `<script>` and `<link>` elements, and its per-field timers then send the note back about 600 ms later. A field whose serialization differs from its stored bytes, or whose whole value is `<br>` or `<div><br></div>`, can so be rewritten without any typing. Meguri serializes only fields with an input (H1, H2; R823, AE40). Scenario `open-close-untouched` shows Anki's result; when its golden records a rewrite, it needs the deviation entry `editor-open-rewrites-fields`.
- **Whole-note saves.** Anki sends every field's current value with each save; Meguri sends only the fields whose targets changed (S2). With nothing else writing the note the stored results are the same; when another writer changed a field meanwhile, Meguri keeps that change.
- **Scripts.** Anki's editor removes `<script>` and `<link>` elements from every field when the note loads, and from HTML-mode text at each change, and its timers then save the result; Meguri keeps them in both modes (H3, F6; R831). Scenario `script-untouched` shows Anki's result; when its golden records the removal, it needs the deviation entry `editor-keeps-scripts`.
- **MathJax.** While Anki's MathJax preview setting is on (its default), its editor shows MathJax as typeset previews. On load each `\(…\)` and `\[…\]` becomes an element, each `<br>` inside it becomes a line feed and line feeds at its ends are removed, so `<br>`s at the ends of math are gone; the save writes the math back as text in that form. A display `\[…\]` element counts as a block element (E1), so a field holding one is not inline-only in Anki: no `<br>` is appended on load and no final `<br>` is dropped on save (H1 step 2, H3 step 1). With the setting off, Anki leaves math as text, as Meguri always does (H1; R563, R2017). Scenario `mathjax-touched` runs with the setting at its default; where its golden differs from Meguri's result, it needs the deviation entry `editor-mathjax-as-text`.
- **HTML mode.** Anki can show both inputs of a field at once and shows parsed, re-serialized source in its HTML input. Meguri shows one input at a time, and the stored bytes in HTML mode while the field is unchanged (H6). Both store HTML-mode text parsed and re-serialized (H3 step 1); Anki parses it as a whole document's body, Meguri as a fragment.
- **Remembered states.** Anki remembers collapsed states and modes per note type in each editor window, and reuses collapsed states even when the field count changed. Meguri's memory spans the app session and needs an equal field count (F3).
- **Font size 0.** Anki's default editor shows a field whose stored `font_size` is 0 at 0 px; Meguri shows it at 20 px (F2).
- **Descriptions.** Anki drops backslashes from a field description when showing it; Meguri shows the description as stored (F1).
- **Quotes in media names.** Anki decodes a display-encoded `"` in a media name to a raw `"` after serializing the field, so a changed field holding `<img src='q"r.png'>` is stored as `<img src="q"r.png">`, which names `q` (`text/media-references` M2). Meguri writes `&quot;` there (H3 step 4). Scenario `media-quote` shows Anki's result; it needs the deviation entry `editor-media-quote-entity`.

## T1. Scenarios for the oracle maintainer

Requested in `conformance/scenarios/editor/field-html/`, with goldens in `conformance/goldens/editor/field-html/` recorded in the quarantined editor-page image as E2 says. None of the rows below is written or recorded yet: no runner step and no oracle driver op drives an editor page today. The scenarios that observe this file's core half with existing ops are listed in T3. The oracle maintainer builds the quarantined image (E2) and the scenario vocabulary from this list.

**Setup.** One note type `Pair` defined by the scenario (R1959), standard, fields `Front` and `Back`, empty CSS, field options as each row says (default: N5.2's). `closeHTMLTags` is `false` in every scenario, so typed tags are not completed (U192). One note, with the stored fields given; `Back` is empty unless given. Every probe reads the note's `flds` bytes and whether `mod` or `usn` changed since setup.

**Actions**, all as native key events on Anki desktop's Edit Current editor unless a row says Add: `open`; `caret F end|start|after "x"`; `type "…"`; `Return`; `Shift+Return`; `Backspace`; `select-all`; `toggle-html F` (in Anki both of F's inputs then show and focus is in its HTML input; a `caret F …` after it, and the keys that follow, act in that HTML input); `blur` (focus moves to the tag editor); `wait N` (ms); `probe`; `close`.

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
| `media-quote` | `<img src='q"r.png'>` | caret Front end, type `x`, blur, probe (X1) |

HTML mode group (H6), with Front's "Use HTML editor by default" set:

| Scenario | Front stored | Actions |
|---|---|---|
| `html-quotes-dashes` | (empty) | caret Front end, type `"a" -- b`, blur, probe |
| `html-toggle-no-edit` | `<span  class="x">a</span>` | toggle-html Front, toggle-html Front, blur, close, probe |
| `html-edit` | `<b>a</b>` | caret Front end, type `<i>c</i>`, blur, probe |
| `html-unclosed` | (empty) | caret Front end, type `<b>x`, blur, probe |
| `html-stray-end` | `a` | caret Front end, type `</i>b`, blur, probe |

Timing and undo group (S1, S2):

| Scenario | Actions | Observed |
|---|---|---|
| `idle-commit` | caret Front end, type `a`, wait 300, probe, wait 400, probe | first probe unchanged, second saved |
| `idle-restart` | caret Front end, type `a`, wait 400, type `b`, wait 400, probe, wait 300, probe | saved only at the second probe |
| `blur-commit` | caret Front end, type `a`, wait 100, blur, probe | saved |
| `two-fields` | caret Front end, type `a`, wait 300, caret Back end, type `b`, wait 700, probe | both saved |
| `save-undo-step` | caret Front end, type `a`, wait 700, caret Front end, type `b`, wait 700, probe, and the undo status | one "Update Note" step (`specs/conformance/undo-ops.md` V4); the 60-second merge itself is already recorded through `edit_note` in `undo/edit-note-merge` |

## T2. Meguri tests without goldens

The plan's U190 test scenarios cover what no editor golden shows. Where a T3 scenario records the core half of a case (S2's merge at 20 s and split at 61 s in `patch/merge-window`; an untouched field byte-identical after another field's save in `patch/other-writer`), the Meguri test adds the editor's half: the commits and the patches the host sends. The cases: AE40's row checks (no op on open and close; an untouched field byte-identical after another field's save), F6 with a handler that would post to the bridge, F4's Tab order and caret restore, F3's session memory and relaunch, H6's quotes and dashes on Mac and iOS, F2's `rtl` with the rendered card unchanged, F1's hidden Image Occlusion fields, F5's routing, S1's timings and background flush, S1's input during a flush without `hold` (the example in S1, with the flush's reply held back for longer than the idle period), S2's merge at 20 s and split at 61 s, F7's layout switch without reload and its reload after the web content process ends (`baselineHtml`, the example in F7), S2's refusals and failures (`NotFound`, `Stale`, `InvalidArgument`, `CollectionUnavailable`, a storage error) with the restored last sent values, the report and the resend at the next flush, S2's `Stale` reload after a field rename and after a reorder (text kept, matched by `id`) and with a removed field (its text shown), a close or load refused after a failed flush patch, H1 step 7's notice for a row whose field count differs, S1's Add-mode draft when a field is edited back to its baseline (empty `a` then deleted; sticky `x` to `xy` and back), B1's `seq` order and B3's `hold` (input after a held flush is refused until `release`), F5's history cleared by a mode switch, F8 with a field holding `<style>div{display:none}</style>x` (the other fields, labels and tag editor unchanged), `setField`'s new baseline, R1833's open time, content mode, and H3 step 4's `&quot;` for a `"` in a media name (stored `<img src='q"r.png'>` and `<img src="q%22r.png">`, each typed `x`, then saved again with no change of the name). Each runs on macOS and on the iOS simulator.

## T3. Core scenarios

Written in `conformance/scenarios/editor/field-html/`, inactive until Meguri's adapters pass them. They use only existing ops (`edit_note`, `db_exec`, `undo`, `undo_status` and the `text` functions `iri_encode` and `iri_decode`) and observe what the Python-library oracle can: the note update a field patch makes, and the display encoding and decoding of media names. Each `edit_note` stands for one patch, whose value is the target H2 and H3 give for the input the scenario's comment names.

| Scenario | Covers |
|---|---|
| `patch/other-writer` | S2 and X1's whole-note saves: a patch of Front keeps the other fields and, with no tags member, the stored tags; `notes/note-records` N2 runs on every field, so a field another writer stored decomposed is stored composed; Front's bytes kept through another editor's save of Back and the tags (AE40's core half, `notes/note-records` U3); H3's table row for `<span  class="x">a</span><br/>`; one undo of the patch (`engine/undo` A4) |
| `patch/revert-to-baseline` | H2, S2 and F7's example: a typed character saved, then the baseline's stored bytes sent and stored again; the two patches 20 s apart are one step; a patch equal to the stored row writes nothing and records no step (`notes/note-records` U2) |
| `patch/merge-window` | S2: patches of two fields 20 s apart merge into one "Update Note" step; patches 61 s apart are two steps (`engine/undo` A3) |
| `patch/script-kept` | F6, H3 step 1 and X1: a patch stores a `<script>` element and an `onerror` attribute as given |
| `display/media-names` | H1 step 1 and H3 step 4 (`text/media-references` M6 to M8): T1's media inputs and others (an `onerror` attribute, a sound tag, a name in two attributes, a `"` in a single-quoted name, `#`, a remote name, an upper-case tag), encoded for display and, as the page writes them back with a typed character, decoded |

- The patch scenarios cannot show the patch's own members: on both executors an `edit_note` reads the live note when its step runs (`conformance/record-ops` W2), so a stale whole note and a patch store the same row. What they fix is the W2 result S2's patch form must equal. Decided 2026-11-10: from U190 on, Meguri's executor runs every `edit_note` step through S2's patch form of the note update. The step's listed or named fields are the patch's (field ordinal, value) pairs; its `tags` are the tags member, only when given; the note type id and the field layout are read from the note when the step runs. With that live layout the patch never refuses with `Stale`, a field count that differs refuses as U1's whole-note form does, and a missing note stays a run error (W2). The results equal the whole-note form's, so no golden changes, and the scenarios run the patch form's apply and save in the engine. S2's refusals, the last sent values, the resend at a flush and the `Stale` reload have no Anki counterpart, since Anki has no patch op: Meguri's tests cover them (T2, D1).
- `display/media-names` records M7's decode, so its golden decodes `<img src="q%22r.png">x` to `<img src="q"r.png">x`. Meguri's `iri_decode` gives the same. H3 step 4's `&quot;` is the editor's step around M7, and only T1's `media-quote` and Meguri's tests (T2) show it.

## D1. Follow-ups in files this spec does not own

Decided 2026-10-27: these are owed by the owners of the named files and change nothing above.

- `notes/note-records` U1: add S2's field-patch input form next to the whole-note form, with S2's refusals (`NotFound`, `Stale`, `InvalidArgument`) and the same op and step name.
- The plan's U190 Files list: add the patch form to U27's note update module in the engine crate and to the FFI crate's note ops, with engine tests for each S2 refusal (a reorder that keeps the field count included) and for an unlisted field that another writer changed.
- `editor/paste` P1 still names the oracle by its package directory; it should say "the Python-library oracle", as E2 does.
