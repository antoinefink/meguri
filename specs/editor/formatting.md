# Editor formatting, toolbar and command routing

Spec: editor/formatting
Requirements: R834, R835, R836, R837, R838, R839, R840 (editor side), R841, R842, R843, R844, R845, R846, R847, R848 (editor side), R861, R864, R865, R249, R250, R253, R254 (routing), R258; supports R196, R197, R199, R200, R201, R218, R827, R1960; AE10 (editor clause)
Anki: 26.09.3
Scenarios: conformance/scenarios/editor/formatting/

What the editor's formatting commands do to a rich-text field's HTML (U192): inline formats, text and highlight colour, removing formatting, lists, indent and alignment, the toolbar with its note type buttons and options menu, and how every editor shortcut reaches its command through the native menus without colliding with another context. Cloze commands are `editor/cloze-wrap`. The field model, the save pipeline and the bridge are `editor/field-html` (U190); media insertion and paste are `editor/paste` (U194, U197); the stored editor keys are `editor/config-keys` K8 to K11.

## F1. Terms and sources

- **Field**, **rich mode**, **HTML mode**, **content**, **input**, **block element**, **serialization**, **target**: as `editor/field-html` E1 and H2 define them.
- **Focused rich field**: the field whose rich-mode editable area has keyboard focus. Its **selection** is the page's selection inside that area; it is **collapsed** when it is only a caret.
- **Inline format**: one of Bold, Italic, Underline, Superscript and Subscript (F3).
- **Wrapper**: an element a command adds around selected content.
- **Sources** (`editor/field-html` E2, decided 2026-10-27 for every editor spec): this file states Anki 26.09.3's editor behaviour in its own words. Its goldens are observed by the oracle maintainer, who runs Anki's own editor page inside the quarantined oracle image. The page and its assets never leave that image, and Meguri's tests and app never load any of it (R1960). The goldens decide the exact markup of every scenario in F17. The rules and examples here are normative; a golden that contradicts one is a finding against this file, which is then corrected, unless the case is a difference listed in F16.
- Meguri's runtime applies these commands with its own DOM transforms. It never uses the web engine's built-in editing commands (`document.execCommand`, WebKit's bold, italic and underline key handling, the iOS edit menu's format actions) on field content.
- **Notation** in examples and scenarios: field HTML, with `[` and `]` around the selection and `‸` for a collapsed caret. Fields in examples never contain those characters.

## F2. Where commands act

- Inline formats, colours, Remove Formatting (F3 to F8), lists, indent and alignment (F9) act only on the focused rich field. With focus in a field's HTML mode, in the tag editor, elsewhere in the window or nowhere, they are disabled: their toolbar controls are dimmed, their menu items are disabled and their keys do nothing (R846). Insertion (F10) follows the same rule.
- Activating a toolbar control or a menu item keeps the field's keyboard focus and its selection, so a command applies to the selection the user made.
- Before each command the host flushes the editor (`editor/field-html` S1).
- A command that changes the content is one input and one entry in the field's history (`editor/field-html` F5), so ⌘Z undoes it as a whole. A command that leaves the serialized content as it was is no input: the field stays unchanged (`editor/field-html` H2) and nothing is saved.
- After a command with a non-collapsed selection, the selection covers the content the command acted on.

## F3. Inline formats (R834 to R837, R249)

| Format | Wrapper | Counts as this format | Excludes |
|---|---|---|---|
| Bold | `<b>` | elements `b` and `strong`; any element whose inline style has `font-weight` `bold` or a number of 700 or more | |
| Italic | `<i>` | elements `i` and `em`; any element whose inline style has `font-style` exactly `italic` or `oblique` | |
| Underline | `<u>` | elements `u` only (a `text-decoration` style does not count) | |
| Superscript | `<sup>` | elements `sup`; any element whose inline style has `vertical-align` `super` | Subscript |
| Subscript | `<sub>` | elements `sub`; any element whose inline style has `vertical-align` `sub` | Superscript |

`font-weight: bolder`, `600` and `font-style: oblique 10deg` do not count. Only the element's own `style` attribute is read, never a class or a computed style.

**Removing a format from an element** that counts as it:

- an element of the format's own tags (`b`, `strong`; `i`, `em`; `u`; `sup`; `sub`) is unwrapped: its children take its place. Decided 2026-10-27: when it has attributes, they are not dropped; the element is replaced by a `span` with the same attributes holding its children, and that `span` then follows the next rule (the format's own property, if its `style` has one, is removed, and a `span` with no attribute left is unwrapped). So `<b style="color: red;">x</b>` loses Bold as `<span style="color: red;">x</span>` and `<strong lang="ja">x</strong>` as `<span lang="ja">x</span>` (F16, `editor-format-unwrap-own-tag-attributes`). Content of the element outside the selection gets its format back inside that `span` (F4);
- otherwise the matching property is removed from the element's `style`. When no property is left the `style` attribute is removed, and then, when the element is a `span` with no attribute left, it is unwrapped. Any other element, and a `span` that keeps another attribute such as `lang`, stays (F16, `editor-format-unwrap-non-span`).

## F4. Applying and removing with a selection

**Toggle decision.** For an excluding format (Superscript, Subscript), the excluded format is first removed from the selection (as below). The selection then **has** the format when its start point or its end point lies inside an element that counts as the format, within the field. A command on a selection that has the format removes it; otherwise it applies it.

**Applying.**

1. Text nodes cut by the selection's edges are split there.
2. Inside the selection, every element that counts as the format loses it (F3).
3. The selected content gets one wrapper per run of adjacent inline content. A wrapper never contains or crosses a block element, so a selection over two `div`s gets one wrapper in each. Void elements such as `img` and `br` inside a run are wrapped with it.
4. When the selection covers the whole content of an inline element (not a block element, not the field itself), the wrapper goes around that element rather than inside it.
5. A wrapper merges with an element of the same format that is the node right before or right after it, so adjacent bold becomes one `<b>`.

**Removing.** Every element that counts as the format and contains or overlaps the selection loses it (F3), starting from the outermost such element around the selection. Content of those elements outside the selection keeps the format, in the format's own wrapper.

| Before | Command | After |
|---|---|---|
| `a[b]c` | Bold | `a<b>b</b>c` |
| `[a<strong>b</strong>c]` | Bold | `<b>abc</b>` |
| `<b>[abc]</b>` | Bold | `abc` |
| `<strong>a[b]c</strong>` | Bold | `<b>a</b>b<b>c</b>` |
| `<span style="font-weight: bold; color: red;">[x]</span>` | Bold | `<span style="color: red;">x</span>` |
| `<span style="font-weight: 700;">[x]</span>` | Bold | `x` |
| `<b style="color: red;">[x]</b>` | Bold | `<span style="color: red;">x</span>` |
| `<b>a</b>[b]` | Bold | `<b>ab</b>` |
| `[<i>x</i>]` | Bold | `<b><i>x</i></b>` |
| `<div>[a</div><div>b]</div>` | Bold | `<div><b>a</b></div><div><b>b</b></div>` |
| `<span style="font-style: oblique;">[x]</span>` | Italic, then Italic | `x`, then `<i>x</i>` |
| `<sub>[x]</sub>` | Superscript | `<sup>x</sup>` |
| `[x]` | Subscript, then Superscript | `<sub>x</sub>`, then `<sup>x</sup>` |

## F5. No selection: the pending format (R842)

- With a collapsed selection a format command changes no content. It toggles a **pending** state for that format: pressing it again before typing cancels it.
- The next text the user inserts at the caret (one typed character, or the committed text of an input-method composition) gets the format: the toggle decision of F4 is taken at the caret, so the new text is wrapped when the caret is outside the format and taken out of it when the caret is inside. The caret then stays at the end of the new text, inside its wrapper, so further typing continues in the same format.
- A space typed while a format is pending is inserted as U+00A0 (stored `&nbsp;`).
- Pending states are cancelled when the field loses focus, on a pointer press in the field, and when the selection moves.
- Decided 2026-10-27: any other insertion while a state is pending (Return, Shift-Return, a spelling or autocorrect replacement, dictation, a paste, a drop or an F10 insertion) first cancels every pending state and then acts as it does with nothing pending; its text gets no pending format. So `a‸`, Bold, Return, type `x` gives a new line holding a plain `x` (F16, `editor-pending-return`).
- Pending Superscript removes Subscript from the new text, and the reverse. Pending colours are F7's, pending removal F8's.
- Example: `a‸`, Bold, type `x`: `a<b>x</b>`. `<b>ab‸cd</b>`, Bold, type `x`: `<b>ab</b>x<b>cd</b>`.

## F6. Format state

The page reports the state of the focused rich field to the host (message `formatState`, F13), which shows it on the toolbar:

- each inline format is **on** when the selection has it (F4's test), inverted while it is pending (F5);
- the list kind is that of the innermost list item around the selection's start (`unordered`, `ordered` or none), and `inListItem` says whether the selection's start lies in an `li` (F9);
- the alignment is that of the block holding the selection's start (`left`, `center`, `right`, `justify`, or none when no `text-align` is set).

Inline format, list and alignment controls show their state as selected toggles (and as the selected trait for VoiceOver). Menu items show no check marks for it; only the option toggles (F12) and the removal choices (F8) do.

## F7. Text colour and highlight (R838, R839, R840)

- **Apply Text Color** (F7, ⌃⌘C, the colour button) applies the last text colour to the selection. It is not a toggle: it replaces every text colour inside the selection.
- An element **has a text colour** when its inline style has a `color` property, or when it is a `font` element with a non-empty `color` attribute. A `font` element can have both; both count, and both are removed (F16, `editor-color-font-style`: Anki reads only a `font` element's attribute).
- Applying: every text colour inside the selection is removed (a `color` property as F3's style rule; a `font` element's `color` attribute is removed too, and a `font` element left with no attribute is unwrapped, F16 `editor-color-font-attributes`). Then the selection is wrapped as F4 says, in `<span style="color: rgb(R, G, B);">`, R, G, B in decimal. When step 4 of F4 places the wrapper exactly around an existing `span`, that span gets the `color` property instead of a new wrapper. Adjacent wrappers merge only when their colours are equal. Content of an old coloured element outside the selection keeps its old colour.
- **Choose Text Color…** (F8, ⌃⇧⌘C, the colour button's chevron) opens the system colour picker: the shared `NSColorPanel` on macOS, `UIColorPickerViewController` on iOS and iPadOS, with the palette of `editor/config-keys` K9 (F12 says where the swatches show). It runs a **picker session**:
  - Opening. With a non-collapsed selection the host, before it presents the picker, sends `colorSession` with `state` `start` (F13). The page records the session's field and its selection then; the field keeps that selection (`editor/field-html` F4) when the picker takes focus and the field blurs. The blur commits as usual (`editor/field-html` S1). Without a focused rich field the command is disabled (F2).
  - A **pick** is every colour change the picker reports (a click on a swatch, each change while the user drags across the colour area). Each pick becomes the last text colour at once. In a session the host sends it as `format` `textColor` with the session's `session`; the page applies it to the recorded selection, first restoring the content as it was when the session started, so only the latest pick's result remains. The session's changes are one input and one history entry, which closes when the session ends; the field commits by S1's idle rule, and at once when the session ends.
  - Ending. The session ends when the picker closes, when another field or the tag editor gets focus, when the field switches mode or collapses, and when the page reloads or the note closes (the host sends `colorSession` `end` for the picker closing and for its own loads and closes; the page ends the session itself on a focus move, mode switch or collapse and sends `colorSessionEnded`). After the end, picks from a picker that stays open (the macOS panel) only set the last colour; a new Choose… starts a new session. When the picker closes, the host gives the field its focus back (`setFocus`), with the selection covering the recoloured content (F2).
  - A session with no pick changes nothing and makes no history entry.
  - With a collapsed selection, Choose… opens the picker without a session: picks only set the last colour, and nothing becomes pending.
- **Highlight** works the same with the `background-color` property: wrapper `<span style="background-color: rgb(R, G, B);">`. `font` elements play no part. Apply Highlight Color and Choose Highlight Color… have no shortcut (R839).
- With a collapsed selection, Apply Text Color and Apply Highlight Color set a pending colour for the next inserted text (F5); it is applied, never toggled.
- Colours are taken off only by Remove Formatting (F8, R838).

| Before | Last colour | After Apply Text Color |
|---|---|---|
| `a[b]c` | `#ff0000` | `a<span style="color: rgb(255, 0, 0);">b</span>c` |
| `<span style="color: rgb(255, 0, 0);">[x]</span>` | `#0000ff` | `<span style="color: rgb(0, 0, 255);">x</span>` |
| `<span class="k" style="color: red;">[x]</span>` | `#0000ff` | `<span class="k" style="color: rgb(0, 0, 255);">x</span>` |
| `<font color="red">[x]</font>` | `#0000ff` | `<span style="color: rgb(0, 0, 255);">x</span>` |
| `<font style="color: red;">[x]</font>` | `#0000ff` | `<span style="color: rgb(0, 0, 255);">x</span>` (the emptied `style` is removed, then the `font` without attributes is unwrapped) |
| `<span style="color: rgb(255, 0, 0);">a[b]c</span>` | `#0000ff` | `<span style="color: rgb(255, 0, 0);">a</span><span style="color: rgb(0, 0, 255);">b</span><span style="color: rgb(255, 0, 0);">c</span>` |

**Last colours** (R840). The last text colour and the last highlight colour are device-local preferences (KTD12, `storage/config-table` C8), never collection config. Each is a lower-case `#rrggbb` string; both default to `#0000ff`, as in Anki, which keeps them per profile. Only a pick in the picker changes them; applying does not. The toolbar's colour buttons show them.

## F8. Remove Formatting (R841)

- The **removal set** holds Bold, Italic, Underline, Superscript, Subscript, Text Color and Highlight Color, each with a check box in the Remove Formatting menu, plus **bare spans**, which are always in it and not listed. A bare span is a `span` with no attributes, an empty `class` or `style` attribute counting as none (F16, `editor-bare-span-attributes`).
- All seven start checked. Decided 2026-10-27: the choices last for the app session and are shared by every editor; they are never stored.
- **With a selection** (⌘R, the toolbar button, the menu item): every checked format is removed from the whole selection as F4's removal does, whatever F4's toggle decision would say; colours as F7 describes, bare spans unwrapped. Unchecked formats stay; elements inside the selection that carry them are rewritten into the format's own wrapper (`<strong>` becomes `<b>`, a colour style moves to its own `span`).
- **With a collapsed selection**: pending formats are cancelled, and each checked format the caret lies inside becomes a pending removal, so the next inserted text is outside it.
- Lists, alignment, headings, links, images and any other element or style are never touched.

| Before | Unchecked | After |
|---|---|---|
| `[<b>a</b><i>b</i><u>c</u>]` | none | `abc` |
| `[<b>a</b><span style="color: rgb(255, 0, 0); background-color: rgb(255, 255, 0);">b</span>]` | Highlight Color | `a<span style="background-color: rgb(255, 255, 0);">b</span>` |
| `<ul><li>[<b>a</b>]</li></ul>` | none | `<ul><li>a</li></ul>` |
| `[<span>a</span><span lang="ja">b</span>]` | none | `a<span lang="ja">b</span>` |
| `<b style="color: red;">[x]</b>` | Text Color | `<span style="color: red;">x</span>` (F3: the `b` becomes a `span` keeping its `style`) |

## F9. Lists, indent and alignment (R843 to R846, R258)

- **Paragraph.** These commands act on paragraphs. A paragraph is a maximal run of inline content (text, inline elements, void elements other than `br`) bounded by the edges of block elements (`editor/field-html` E1) and by `<br>` elements. The `<br>` that ends a paragraph belongs to it: when the paragraph moves into a new `li` or a new `div`, or when the paragraph after it does, that `<br>` is removed, since a block edge now ends the line. A paragraph **fills** a block when it is that block's whole content, apart from its own final `<br>`. A selection **touches** every paragraph it contains or overlaps; a collapsed selection touches the paragraph holding the caret.
- **Bulleted List** (⌘,) and **Numbered List** (⌘.):
  - when every touched paragraph is in an item of a list of this kind (the innermost list around it), they leave it: each touched item's content takes the list's place as paragraphs in order, separated by `<br>` where two paragraphs of inline content meet. Untouched items before or after stay in a list of this kind, so the list is split around them. An item that has attributes (such as an alignment `style`) becomes a `div` with those attributes holding its content, so they are kept;
  - otherwise each touched paragraph that is in no list becomes its own `li`, holding the paragraph's inline content, and touched items of a list of this kind stay as they are. A paragraph that fills a `div` or `p` replaces that element, whose attributes go onto the `li`. A paragraph already in a list of the other kind changes kind: when the list's every item is touched, the list element is replaced by one of this kind with the same attributes and children; otherwise it is split, and the touched items form a list of this kind between the untouched parts. Consecutive new items go into one list, which merges with a list of this kind that is its node right before or right after;
  - examples: `a[b]c` gives `<ul><li>abc</li></ul>`; `[a<br>b]` gives `<ul><li>a</li><li>b</li></ul>`; `a<br>[b]` gives `a<ul><li>b</li></ul>`; `a<br>[b<br>c]` with Numbered List gives `a<ol><li>b</li><li>c</li></ol>`; `<ul><li>[a]</li></ul>` with Bulleted List gives `a`; `<ul><li>[a</li><li>b]</li></ul>` gives `a<br>b`; `<ul><li>a</li><li>[b]</li><li>c</li></ul>` gives `<ul><li>a</li></ul>b<ul><li>c</li></ul>`; `<ul><li>[a]</li></ul>` with Numbered List gives `<ol><li>a</li></ol>`; `<ul><li>[a</li></ul>b]` with Bulleted List gives `<ul><li>a</li><li>b</li></ul>`.
- **Increase Indent** (⌘] or ⇧⌘.) and **Decrease Indent** (⌘[ or ⇧⌘,) act only when the selection's start lies inside an `li`. They act on every `li` the selection touches, in its innermost list. Elsewhere they change nothing and show an alert with the message "Indent/unindent currently only works with lists." and an OK button (R845). They stay enabled in rich mode, so the message is reachable.
  - Indent moves each touched item one level deeper: consecutive touched items are wrapped together in a new list of their list's kind, placed where they were, as a child of the list itself (not inside the previous `li`, which is the pinned engine's structure). That nested list merges with a list of the same kind that is its node right before or right after. `<ul><li>a</li><li>[b]</li></ul>` gives `<ul><li>a</li><ul><li>b</li></ul></ul>`.
  - Outdent moves each touched item one level up. An item of a nested list goes into the enclosing list at the nested list's place, which is split around it, and an emptied nested list is removed: the last example back gives `<ul><li>a</li><li>b</li></ul>`. An item of a top-level list leaves the list as Bulleted List's first rule says, so a second outdent gives `<ul><li>a</li></ul>b`.
- **Alignment** (Align Left, Center, Align Right, Justify; no shortcut, R844) sets `text-align` (`left`, `center`, `right`, `justify`) for each touched paragraph. A paragraph that fills a block (a `div`, `p`, `li`, `h1` to `h6`, `blockquote`, `td` or any other block element) gets the property in that block's `style`, replacing any `text-align` there. Any other paragraph is wrapped in a new `<div style="text-align: …;">` of its own. A block that already has the alignment keeps it. Examples: `a[b]c` with Center gives `<div style="text-align: center;">abc</div>`; `a<br>[b]` with Center gives `a<div style="text-align: center;">b</div>`; `[a<br>b]` gives `<div style="text-align: center;">a</div><div style="text-align: center;">b</div>`; `<ul><li>[a]</li></ul>` gives `<ul><li style="text-align: center;">a</li></ul>`.
- **Goldens.** The goldens of F17's list group decide the exact markup of their scenarios, and for each one Meguri's result must equal the golden byte for byte. The rules above are normative for every case no scenario covers, and are implemented now; a golden that contradicts a rule changes the rule, which is then corrected here, as `editor/field-html` H4 does for Return. These commands never add inline formats.
- The indent controls' symbols follow the interface layout direction, as system symbols do, so they are mirrored in a right-to-left interface. A field's `rtl` option (`editor/field-html` F2) does not change them, so the shared toolbar does not flip as focus moves between fields.

## F10. Inserting HTML

The editor's one insertion step, used by `editor/paste` for attached media, photos and recordings (P5, P6, P20) and for paste and drop (P16), and by `editor/cloze-wrap` W4:

- An HTML fragment, parsed as `editor/field-html` H1 step 2 parses field HTML, but in an inert document and with no `<br>` appended (as H3 step 1 parses HTML-mode text), replaces the focused rich field's selection: the selected content is deleted, a text node at that point is split, and the fragment's nodes are placed there. The caret ends after the last inserted node.
- No wrapper, style or `span` is added around the inserted nodes. Where the goldens show that Anki's page adds or merges markup, Meguri's runtime matches the golden for that scenario.
- It is one input and one history entry. An empty fragment inserts nothing and is no input.
- Only the focused rich field takes insertions; commands that insert are disabled in HTML mode (F2).

## F11. HTML editor toggle (R254, R827)

⇧⌘X, the field's mode button and the Format menu's HTML Editor item switch the field that has focus between rich and HTML mode, as `editor/field-html` H6 says. Opening HTML mode moves the keyboard focus into the text area with the caret at its end; closing it returns focus to the rich content with its earlier selection restored (`editor/field-html` F4). The menu item shows a check mark while the focused field is in HTML mode. With no field focused it is disabled.

## F12. Toolbar, note type buttons and options (R847, R848, R864)

The toolbar is a native control row: a row above the fields on macOS, the keyboard's input accessory on iPadOS and iOS (screen contract `editor.add`). Its groups, in order, with accessibility identifiers `editor.toolbar.<id>`:

| Group | Controls (id) |
|---|---|
| Note type | Fields… (`fields`), Cards… (`cards`) |
| Options | a menu button (`options`): Shrink Images, Close HTML Tags |
| Inline | Bold (`bold`), Italic (`italic`), Underline (`underline`); Superscript (`superscript`), Subscript (`subscript`); Text Color (`textColor`) with its chevron (`textColorPicker`), Highlight Color (`highlight`) with its chevron (`highlightPicker`); Remove Formatting (`removeFormatting`) with its menu of check boxes (`removeFormattingMenu`) |
| Block | Bulleted List (`bulletedList`), Numbered List (`numberedList`), an Alignment menu (`alignment`) with the four alignments, Decrease Indent (`outdent`), Increase Indent (`indent`) |
| Media | Attach (`attach`; on iPhone and iPad a menu of its sources, `editor/paste` P5, P6), Record Audio (`record`, `editor/paste` P20) |
| Cloze | `editor/cloze-wrap` W2 |

- **Fields…** and **Cards…** (⌘L) flush the editor, then open the field editor (U201) or the card templates editor (U200) for the note's type, with the current note, or the Add screen's draft, in the preview (R847, R253). They are enabled whenever the editor shows a note, wherever keyboard focus is in its window (context `editor.note`, F14).
- **Shrink Images** and **Close HTML Tags** are toggles with check marks. Choosing one writes its key at once (`editor/config-keys` K8), and every open editor applies the new value without reloading (message `setOptions`, F13). Shrink Images' effect is `editor/paste` P18 (R878).
- **Close HTML Tags**, while on, acts in a field's HTML mode as the user types:
  - **the scan**: the text before the caret is tokenized as the HTML Standard's tokenizer does, so comments, doctypes and quoted attribute values are skipped and the text of `script`, `style`, `textarea` and `title` is raw text. A stack holds the names of start tags, lower-cased, leaving out void elements (below) and tags written with `/>`. An end tag pops the stack down to and including the innermost entry of its name and is ignored when there is none. Decided 2026-10-27: nothing is closed implicitly, so `<ul><li>a<li>b` leaves `ul`, `li`, `li` open (F16). The **innermost element still open** is the top of the stack. A typed `>` **ends an opening tag** only when the tokenizer is then in a start tag's name or attribute state, outside a quoted value: in `<a title="x>y">` only the last `>` does;
  - typing `>` that ends an opening tag `<name …>` inserts `</name>` right after the caret, unless the tag ends in `/>`, `name` is a void element (`area`, `base`, `br`, `col`, `command`, `embed`, `hr`, `img`, `input`, `keygen`, `link`, `meta`, `param`, `source`, `track`, `wbr`, compared ignoring case) or a closing tag for it already follows; the caret stays between the two tags;
  - typing `/` right after `<` inserts the name of the innermost element still open before the caret, and `>`, unless no element is open or a closing tag for that element already follows;
  - a closing tag for `name` **already follows** when, with n the number of consecutive innermost open elements named `name` around the caret (plus one for the tag just typed in the `>` case), the next n closing tags after the caret that close no element opened after the caret are all `</name>`. Example: with `‸</b>`, typing `<b>` inserts nothing more. Decided 2026-10-27: Meguri searches the rest of the field; Anki stops after 500 lines, which no field in a scenario reaches;
  - nothing else is inserted. Decided 2026-10-27: no line breaks or indentation between the tags, also after a block tag such as `div` (F16, `editor-close-tags-no-indent`).
  - Each insertion is part of the same input as the keystroke.
- The palette swatches (`editor/config-keys` K9) show in the colour panel on macOS, as a colour list the panel offers, and as a swatch row beside the system picker on iOS and iPadOS, whose own favourites an app cannot set. Choosing Add to Palette in that place adds the current colour; a swatch's context menu removes it.
- On iPhone the row may group controls into menus to fit; every command stays one tap or one menu away.
- Every function-key action is also a toolbar control and a menu command (R864): F3, F5 (U194), F7, F8, and F9 (U195).

## F13. Bridge messages

Additions to `editor/field-html` B2 and B3, same rules (B1).

| Direction | Type | Members | Reply or when |
|---|---|---|---|
| page to host | `formatState` | `loadId`, `ord`, `bold`, `italic`, `underline`, `superscript`, `subscript` (booleans, F6), `list` (`unordered`, `ordered` or null), `inListItem` (boolean), `align` (`left`, `center`, `right`, `justify` or null), `collapsed` (boolean) | when the focused rich field's state changes |
| host to page | `format` | `loadId`, `ord`, `command` (`bold`, `italic`, `underline`, `superscript`, `subscript`, `textColor`, `highlight`, `removeFormatting`, `bulletedList`, `numberedList`, `indent`, `outdent`, `alignLeft`, `alignCenter`, `alignRight`, `justify`), `color` (`#rrggbb`, colour commands only), `remove` (an array of the tokens of F8's checked formats, from `bold`, `italic`, `underline`, `superscript`, `subscript`, `textColor` and `highlight`; `removeFormatting` only) | `changed` (boolean); `notInList: true` when indent or outdent found no list item, and the host then shows F9's alert |
| host to page | `wrap` | `loadId`, `ord`, `prefix`, `suffix` (`editor/cloze-wrap` W4) | `changed` (boolean) |
| host to page | `setOptions` | `closeHTMLTags`, `shrinkImages` | done |
| host to page | `colorSession` | `loadId`, `ord`, `session` (an integer the host picks, new for each session), `state` (`start` or `end`) | `started` (boolean) for `start`: false when `ord` is not the focused rich field or its selection is collapsed; done for `end` |
| page to host | `colorSessionEnded` | `loadId`, `session` | when the page ends a session itself (F7: focus moved, mode switch, collapse) |

`format` takes an optional `session` member, only with `textColor` or `highlight` (F7's picker session). The page refuses `format` and `wrap` for an `ord` that is not the focused rich field and replies `changed: false`, except a `format` whose `session` is the open session of that `ord`, which acts on the session's recorded selection. `editor/paste` P25 adds the insertion and image messages.

## F14. Shortcuts and contexts (R861, R249, R250, R258, R197, R200)

Contexts, registered with U158's shortcut registry:

| Context | Active when |
|---|---|
| `editor.richText` | a field in rich mode has keyboard focus |
| `editor.htmlSource` | a field in HTML mode has keyboard focus |
| `editor.any` | any part of an editor page has keyboard focus: a field in either mode, or the tag editor |
| `editor.note` | the key window shows an editor with a note (the Add screen, Edit Current, Browse's editor), wherever keyboard focus is in that window |

| Command | Key shown in the menu | Other keys | Context | Anki |
|---|---|---|---|---|
| Bold | ⌘B | | `editor.richText` | Ctrl+B |
| Italic | ⌘I | | `editor.richText` | Ctrl+I |
| Underline | ⌘U | | `editor.richText` | Ctrl+U |
| Superscript | ⌘= | | `editor.richText` | Ctrl+= |
| Subscript | ⇧⌘= | | `editor.richText` | Ctrl+Shift+= |
| Apply Text Color | ⌃⌘C | F7 | `editor.richText` | F7 |
| Choose Text Color… | ⌃⇧⌘C | F8 | `editor.richText` | F8 |
| Apply Highlight Color, Choose Highlight Color… | none | | `editor.richText` | none |
| Remove Formatting | ⌘R | | `editor.richText` | Ctrl+R |
| Bulleted List | ⌘, | | `editor.richText` | Ctrl+, |
| Numbered List | ⌘. | | `editor.richText` | Ctrl+. |
| Increase Indent | ⌘] | ⇧⌘. | `editor.richText` | Ctrl+Shift+. |
| Decrease Indent | ⌘[ | ⇧⌘, | `editor.richText` | Ctrl+Shift+, |
| Align Left, Center, Align Right, Justify | none | | `editor.richText` | none |
| Cloze Deletion, Cloze Deletion (Same Number) | ⇧⌘C, ⌥⇧⌘C | | `editor.richText` (`editor/cloze-wrap` W2) | Ctrl+Shift+C, Ctrl+Alt+Shift+C |
| HTML Editor | ⇧⌘X | | `editor.richText`, `editor.htmlSource` | Ctrl+Shift+X |
| Cards… | ⌘L | | `editor.note` | Ctrl+L |
| Fields… | none | | `editor.note` | none |
| Shrink Images, Close HTML Tags | none | | `editor.note` | none |
| Attach… | ⌥⌘A | F3 | `editor.richText` (`editor/paste` P5) | F3 |
| Record Audio… | ⌥⌘R | F5 | `editor.richText` (`editor/paste` P20) | F5 |
| Tags, Add-screen keys | U193, U195 | | | |

- **Superscript and Subscript.** Decided 2026-10-27: ⌘= is Superscript and ⇧⌘= Subscript, as in Anki 26.09.3. R249, R837 and R861 name the opposite keys while R861 and R249 also say the keys are Anki's with Ctrl mapped to ⌘; Anki compatibility settles the conflict. The `=` key is the key that types `=` without Shift on the active layout.
- **Primary and other keys.** Decided 2026-10-27: a menu item shows the key that every Apple keyboard can type (⌃⌘C rather than F7, which needs Fn on many keyboards) and Apple's usual indent keys (⌘] and ⌘[). The other keys run the same command and are listed in Help › Keyboard Shortcuts (R201); they are not shown again in the iPad ⌘ overlay.
- **One key path** (N4). Each key above is bound once, as a native menu command or key command, and runs the same action and guard as its toolbar control. The page does not act on these keys itself. WebKit's own handling of ⌘B, ⌘I and ⌘U, the iOS edit menu's Bold, Italic and Underline, and the system Font panel never change field HTML: Meguri removes them or routes them to F3's commands. A key held down acts once; repeats are ignored (R199).
- **Text keys** (R197, AE10). While `editor.any` is active, single-key shortcuts of every other context (review, deck list, Browse, previewer) are disabled and their keys type into the focused field or tag editor. A review behind an editor panel never receives them.
- **Collisions resolved for R200.**
  - ⌘, opens Settings (R214) except while `editor.richText` is active, when it is Bulleted List; the Settings item keeps its title and loses its key equivalent in that context (R843). With focus in the tag editor or HTML mode, ⌘, opens Settings.
  - ⌘. is Numbered List while `editor.richText` is active and then never cancels or dismisses a sheet; Esc keeps its role (R259).
  - View › Zoom In (⌘+, R218) loses its keys while `editor.richText` is active, because on most layouts ⌘+ is ⇧⌘=; Zoom Out and Actual Size keep theirs. This settles the shell coverage note's collision.
  - ⌥⌘A is Attach… while `editor.richText` is active, also in Browse's editor; Browse's Select All then keeps only ⌘A in the table (R1153, R1225, `browse.browser`), as the browser-ui coverage note proposes. While `editor.any` is active in another way (a field in HTML mode, the tag editor), ⌥⌘A does nothing; with focus elsewhere in Browse it is Select All.
  - The system's Show Colors and Show Fonts commands are not in the menus: Meguri's Format menu replaces the system's text formatting commands (⇧⌘C is Cloze Deletion; ⌘T is unbound).
  - The registry test lists the enabled bindings per context (editor over review, editor in Browse with ⌥⌘A in rich mode, in HTML mode, in the tag editor and in the table, Add panel over Browse, editor focused in each mode, tag editor focused, Add screen with focus on its Deck row) and fails on any duplicate (R200).
- A disabled command's key does nothing: it neither types nor reaches another context (for example ⇧⌘C in a field that is not a cloze field).

## F15. Menus (R864, R865, R278, N4, N11.3)

- A **Format** menu, after Edit in the macOS menu bar and the iPadOS menu bar, holds the editor commands. Each item is enabled only while its F14 context is active, and then only when its own guard allows it (F2, F11, `editor/cloze-wrap` W2); outside an editor every item is disabled (N4). Groups, in order:
  1. Bold, Italic, Underline, Superscript, Subscript
  2. Apply Text Color, Choose Text Color…, Apply Highlight Color, Choose Highlight Color…
  3. Remove Formatting, and the submenu Formatting to Remove with F8's seven check boxes
  4. Bulleted List, Numbered List, Increase Indent, Decrease Indent, and the submenu Alignment (Align Left, Center, Align Right, Justify)
  5. Cloze Deletion, Cloze Deletion (Same Number) (`editor/cloze-wrap` W2)
  6. Attach…, Record Audio… (`editor/paste` P5, P20)
  7. HTML Editor (F11)
  8. Fields…, Cards…
  9. Shrink Images, Close HTML Tags
- Each menu item runs the same action as its toolbar control and is disabled exactly when the control is (N4).
- On iPadOS every item with a key appears in the ⌘ overlay of an editor context (R865). With a hardware keyboard on iPhone the same keys work (R196).
- These menu items act on the editor that has keyboard focus, in the key window. The editor's keys take precedence over a review page behind it, and the review's page keeps its claim on its own keys only while the review has focus (N11.3).

## F16. Differences from Anki

Proposed `conformance/deviations.yaml` ids, to be entered with the goldens that show them:

| Situation | Anki 26.09.3 | Meguri | Deviation |
|---|---|---|---|
| An element whose only style was the removed property: a non-`span` element such as `<a href="u" style="color: red;">` under Remove Formatting or `<div style="font-weight: bold;">` under Bold, or a `span` with another attribute such as `<span lang="ja" style="font-weight: bold;">` under Bold | unwrapped when it has no class, so the link, the block or the `lang` is lost | only the property is removed; only a `span` with no attribute left is unwrapped (F3) | `editor-format-unwrap-non-span` |
| An element of a format's own tag with other attributes, such as `<b style="color: red;">`, `<b class="k">` or `<strong lang="ja">`, under that format or Remove Formatting | unwrapped with its attributes, so the colour, class or `lang` is lost even when Text Color is unchecked | replaced by a `span` keeping those attributes (F3) | `editor-format-unwrap-own-tag-attributes` |
| `<font color="red" face="Arial">` when a text colour is applied or removed | the whole `font` element is unwrapped | only `color` is removed; the element stays while it has attributes (F7) | `editor-color-font-attributes` |
| `<font style="color: red;">` when a text colour is applied or removed | only a `font` element's `color` attribute is read, so the style colour stays (the new colour's `span` goes inside) | the style colour counts and is removed (F7) | `editor-color-font-style` |
| Return, a replacement or another non-typing insertion while a format or colour is pending | the insertion is taken over: a formatted U+00A0 (or the replacement's text, formatted) is inserted and no line starts | every pending state is cancelled and the insertion acts as usual (F5) | `editor-pending-return` |
| Close HTML Tags after an element that HTML closes implicitly, such as `<ul><li>a<li>b</li>` then `</` | open elements are closed implicitly by later tags (a second `li` closes the first, a `div` closes a `p`), so `</ul>` is completed | no implicit closing, so `</li>` is completed (F12) | none (no golden) |
| Remove Formatting on `<span lang="ja">` or a `span` with `data-` attributes | unwrapped (only `class` and `style` are looked at) | kept (F8) | `editor-bare-span-attributes` |
| Close HTML Tags after an opening block tag such as `div`, `p`, `ul`, `ol`, `table`, `blockquote` or `h1` | the closing tag goes two lines down and the caret waits on an indented blank line between the tags | `</name>` right after the caret, no line break (F12) | `editor-close-tags-no-indent` |
| Remove Formatting choices | kept per editor window until it closes | per app session, shared (F8) | none (no golden) |
| Cards… (⌘L) and Fields… with focus outside the editor page, such as on the Add screen's Deck row | the key acts only while the editor page has focus | enabled whenever the window's editor shows a note (F14 `editor.note`) | none (no golden) |
| Zoom In while a rich field has focus | Ctrl+= is both zoom and superscript; the page takes it | Zoom In has no key in that context (F14) | none (no golden) |
| The option keys' undo and reading | `editor/config-keys` K10 | | none (no golden) |

The formatting rows keep data the user did not ask to remove (data safety first); the pending-insertion and implicit-closing rows keep the rules simple and easy to predict. Everything else in this file is meant to match the goldens.

## F17. Scenarios for the oracle maintainer

Requested in `conformance/scenarios/editor/formatting/`, with goldens in `conformance/goldens/editor/formatting/` marked `source: observed`, recorded as `editor/field-html` E2 and T1 say. None of the rows below is written or recorded: no runner step and no oracle driver op drives an editor page today. One core scenario is written there with existing ops: `options/key-writes`, the stored side of F12's option and palette writes (`editor/config-keys` K8, K9 and K11), inactive until Meguri's adapters pass it.

**Setup.** `editor/field-html` T1's note type `Pair` and note. Each row gives Front's stored HTML with the selection in F1's notation; the maintainer opens Edit Current, focuses Front, sets that selection and runs the commands with Anki's own key (Ctrl on the image's platform) or, for commands without a key, its toolbar control. `type "…"` types as native key events. Last colours are set in the profile before the editor opens. Every scenario ends with blur and probe (Front's stored HTML), and Meguri's run compares the same probe.

Inline group (F3, F4):

| Scenario | Front | Commands |
|---|---|---|
| `bold-plain` | `a[b]c` | Bold |
| `bold-over-strong` | `[a<strong>b</strong>c]` | Bold, probe, Bold |
| `bold-inside-strong` | `<strong>a[b]c</strong>` | Bold |
| `bold-weight-styles` | `[<span style="font-weight: bold; color: red;">a</span><span style="font-weight: 700">b</span><span style="font-weight: 600">c</span>]` | Bold, probe, Bold |
| `bold-merge-neighbour` | `<b>a</b>[b]` | Bold |
| `bold-around-inline` | `[<i>x</i>]` | Bold |
| `bold-two-blocks` | `<div>[a</div><div>b]</div>` | Bold |
| `bold-with-image` | `[a<img src="x.png">b]` | Bold |
| `bold-classed-strong` | `<b class="k">[x]</b>` | Bold (F16) |
| `bold-styled-b` | `<b style="color: red;">[x]</b>` | Bold (F16) |
| `bold-lang-strong` | `<strong lang="ja">a[b]c</strong>` | Bold (F16) |
| `italic-oblique` | `<span style="font-style: oblique">[x]</span>` | Italic, probe, Italic |
| `italic-em` | `<em>a[b]c</em>` | Italic |
| `underline-decoration` | `<span style="text-decoration: underline">[x]</span>` | Underline |
| `sup-sub-exclusive` | `[x]` | Subscript, probe, Superscript |
| `sub-over-sup-style` | `<span style="vertical-align: super">[x]</span>` | Subscript |
| `div-weight-style` | `<div style="font-weight: bold">[x]</div>` | Bold (F16) |
| `bold-span-lang` | `<span lang="ja" style="font-weight: bold;">[x]</span>` | Bold (F16) |

Pending group (F5):

| Scenario | Front | Commands |
|---|---|---|
| `pending-bold` | `a‸` | Bold, type `xy` |
| `pending-unbold` | `<b>ab‸cd</b>` | Bold, type `x` |
| `pending-cancel` | `a‸` | Bold, Bold, type `x` |
| `pending-space` | `a‸` | Bold, type ` ` then `x` |
| `pending-moved` | `ab‸` | Bold, press Left, type `x` |
| `pending-sup-sub` | `<sub>a‸</sub>` | Superscript, type `x` |
| `pending-return` | `a‸` | Bold, press Return, type `x` (F16) |

Colour group (F7):

| Scenario | Front | Last colours | Commands |
|---|---|---|---|
| `color-plain` | `a[b]c` | text `#ff0000` | Text Color |
| `color-replace-span` | `<span style="color: rgb(255, 0, 0);">[x]</span>` | text `#0000ff` | Text Color |
| `color-classed-span` | `<span class="k" style="color: red;">[x]</span>` | text `#0000ff` | Text Color |
| `color-font` | `<font color="red">[x]</font>` | text `#0000ff` | Text Color |
| `color-font-face` | `<font color="red" face="Arial">[x]</font>` | text `#0000ff` | Text Color (F16) |
| `color-font-style` | `<font style="color: red;">[x]</font>` | text `#0000ff` | Text Color (F16) |
| `color-middle` | `<span style="color: rgb(255, 0, 0);">a[b]c</span>` | text `#0000ff` | Text Color |
| `color-merge-same` | `<span style="color: rgb(0, 0, 255);">a</span>[b]` | text `#0000ff` | Text Color |
| `color-pending` | `a‸` | text `#00ff00` | Text Color, type `x` |
| `highlight-plain` | `a[b]c` | highlight `#ffff00` | Highlight Color |
| `highlight-replace` | `<span style="background-color: yellow; color: red;">[x]</span>` | highlight `#00ffff` | Highlight Color |
| `default-colors` | `[a]b` | none set | Text Color, then select `b`, Highlight Color |

Remove group (F8):

| Scenario | Front | Unchecked | Commands |
|---|---|---|---|
| `remove-all` | `[<b>a</b><i>b</i><u>c</u><sup>d</sup><sub>e</sub><span style="color: red;">f</span><span style="background-color: yellow;">g</span><span>h</span>]` | none | Remove Formatting |
| `remove-keep-highlight` | `[<b>a</b><span style="color: rgb(255, 0, 0); background-color: rgb(255, 255, 0);">b</span>]` | Highlight Color | Remove Formatting |
| `remove-keep-bold-rewrites` | `[<strong>a</strong><i>b</i>]` | Bold | Remove Formatting |
| `remove-blocks-untouched` | `<ul><li>[<b>a</b>]</li></ul><div style="text-align: center;"><h1>b</h1></div><a href="u">c</a>` with the whole field selected | none | Remove Formatting |
| `remove-attributed-spans` | `[<span>a</span><span lang="ja">b</span><span data-x="1">c</span>]` | none | Remove Formatting (F16) |
| `remove-link-color` | `[<a href="u" style="color: red;">a</a>]` | none | Remove Formatting (F16) |
| `remove-pending` | `<b>a‸</b>` | none | Remove Formatting, type `x` |
| `remove-partial` | `<b>a[b]c</b>` | none | Remove Formatting |
| `remove-keep-color-on-b` | `<b style="color: red;">[x]</b>` | Text Color | Remove Formatting (F16) |

List, indent and alignment group (F9):

| Scenario | Front | Commands |
|---|---|---|
| `list-bullet` | `a[b]c` | Bulleted List |
| `list-number` | `a[b]c` | Numbered List |
| `list-toggle-off` | `<ul><li>[a]</li></ul>` | Bulleted List |
| `list-switch-kind` | `<ul><li>[a]</li></ul>` | Numbered List |
| `list-two-blocks` | `<div>[a</div><div>b]</div>` | Bulleted List |
| `list-after-text` | `a<br>[b]` | Bulleted List |
| `list-multiline` | `[a<br>b]` | Bulleted List, probe, Bulleted List |
| `list-multiline-partial` | `a<br>[b<br>c]` | Numbered List |
| `list-middle-off` | `<ul><li>a</li><li>[b]</li><li>c</li></ul>` | Bulleted List |
| `list-mixed` | `<ul><li>[a</li></ul>b]` | Bulleted List |
| `list-styled-item-off` | `<ul><li style="text-align: center;">[a]</li></ul>` | Bulleted List |
| `indent-item` | `<ul><li>a</li><li>[b]</li></ul>` | Indent, probe, Outdent, probe, Outdent |
| `indent-two-items` | `<ol><li>a</li><li>[b</li><li>c]</li></ol>` | Indent, probe, Outdent |
| `indent-outside-list` | `a[b]c` | Indent (Anki's alert shows; the field is unchanged) |
| `align-center` | `a[b]c` | Center |
| `align-in-div` | `<div>a</div><div>[b]</div>` | Align Right |
| `align-justify-then-left` | `[a]` | Justify, probe, Align Left |
| `align-multiline` | `a<br>[b]` | Center |
| `align-multiline-all` | `[a<br>b]` | Center |
| `align-list-item` | `<ul><li>[a]</li></ul>` | Center |

Insertion group (F10), through Anki's attach of an existing media file `x.png` and through cloze (`editor/cloze-wrap` W9) only, since those are the page's insertions a scenario can drive:

| Scenario | Front | Commands |
|---|---|---|
| `insert-empty-field` | `‸` | attach `x.png` |
| `insert-in-bold` | `<b>a‸b</b>` | attach `x.png` |
| `insert-over-selection` | `a[b]c` | attach `x.png` |

HTML mode group (F11, F12):

| Scenario | Front | Commands |
|---|---|---|
| `close-tags-on` | `‸` in HTML mode, `closeHTMLTags` true | type `<b>`, type `x`, probe; type `<br>`, probe; type `<i>y</` |
| `close-tags-off` | the same with `closeHTMLTags` false | the same |
| `close-tags-block` | `‸` in HTML mode, `closeHTMLTags` true | type `<div>`, probe; type `y</` (F16) |
| `close-tags-existing` | `‸</b>` in HTML mode, `closeHTMLTags` true | type `<b>`, probe; type `x</` |
| `close-tags-self-closing` | `‸` in HTML mode, `closeHTMLTags` true | type `<img src="a.png"/>`, type `<keygen>`, type `<span class="k">` |
| `close-tags-quoted` | `‸` in HTML mode, `closeHTMLTags` true | type `<a title="x>y">`, probe; type `<!-- <i> -->`, type `</` |

## F18. Meguri tests without goldens

The plan's U192 tests cover what no golden shows: F2's disabled states (HTML mode, tag editor, no focus), the toolbar keeping focus and selection, F6's toolbar states, F7's pickers on macOS and iOS (a session over a drag of many picks giving one history entry and the last pick's colour, a collapsed selection setting only the last colour, focus moving to another field while the macOS panel stays open, a reload during a session) and the last colours' defaults and persistence, F8's session memory, F12's options applying to every open editor and the palette swatches, F14's registry test and the collisions (⌘, in rich mode and in the tags, ⌘. in a sheet, Zoom In, ⇧⌘= on a layout where `=` needs Shift), WebKit's ⌘B and the iOS edit menu routed to F3, held keys acting once, AE10's typed `e` with the edit panel over a review, and F15's menu enabling, ⌘ overlay and Help listing. Each runs on macOS and on the iOS simulator.

## F19. Strings

Meguri's own wording, for the String Catalog. Anki's English is shown where an item corresponds to one of its labels; R845's message is Anki's, kept exactly.

| Where | Meguri | Anki |
|---|---|---|
| Menu and toolbar labels | Format · Bold · Italic · Underline · Superscript · Subscript · Apply Text Color · Choose Text Color… · Apply Highlight Color · Choose Highlight Color… · Remove Formatting · Formatting to Remove · Text Color · Highlight Color · Bulleted List · Numbered List · Increase Indent · Decrease Indent · Alignment · Align Left · Center · Align Right · Justify · Cloze Deletion · Cloze Deletion (Same Number) · Attach… · Record Audio… (their other strings: `editor/paste` P21) · HTML Editor · Fields… · Cards… · Options · Shrink Images · Close HTML Tags · Add to Palette · Remove from Palette | Bold text · Italic text · Underline text · Superscript · Subscript · Text color · Change color · Text highlight color · Remove formatting · Select formatting to remove · Unordered list · Ordered list · Increase indent · Decrease indent · Alignment · Align left · Center · Align right · Justify · Cloze deletion (new card) · Cloze deletion (same card) · Toggle HTML Editor · Fields... · Cards... · Options · Shrink Images · Auto-close HTML tags |
| Indent outside a list (F9) | alert message "Indent/unindent currently only works with lists." with button "OK" | the same message |

The screen contracts (`docs/design`) own placement and appearance.
