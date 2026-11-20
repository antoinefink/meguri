# Filter sidebar

Spec: browser/sidebar
Requirements: R1157, R1158, R1159, R1160, R1161, R1162, R1163, R1164, R1165, R1166, R1167, R1168, R1169, R1170, R1171, R1172, R1174, R1175, R1176, R1177, R1178, R1179, R1180, R1268 (sidebar part), AE43 (sidebar part); supports R274, R899, R1173, R66, R67
Anki: 26.09.3 for S2 to S12 (the desktop browser's sidebar); none for the Meguri rules S15 lists
Scenarios: conformance/scenarios/browser/sidebar/ (recorded, not active; S16)
Status: Under review (round 8, `docs/design/decisions.md` 2026-11-20, row `browser`) for the lines that begin "Changed 2026-11-20 (round 8)": do not implement them until approved. Every other section is in force and unchanged.

What Browse's filter sidebar (U216, U217) holds and does: its sections and items, the search each item stands for, their order, which items keep their expansion and where, the filter box, how activating items changes the search, and which management action each item offers with the operation it calls. The operations themselves are other files' (named in each section); this file says which one runs, with which input. Layout, icons, colours, labels and gestures are Meguri's (`docs/design/screen-inventory.yaml` `browse.browser`, `browser/browse-strings` W6). Anki's sidebar is a desktop widget the oracle does not run (`conformance/AGENTS.md`), so apart from the searches the items build (S16) Meguri's tests check this file from its tables.

## S1. Terms

- **Item**: one row of the sidebar. It has a **kind** (S2), a **label** (the text shown), sometimes a **search** (a built item of `search/join-and-replace` J3), sometimes children, and an **identity** (S4).
- **Section**: one of the seven top-level items (S2). Its own row is the **section root**.
- **Full name**: for a deck or tag item, the item's name with all its levels and `::` between them; the label shows only the last level (`decks/decks` D12, `tags/tags` T13).
- **Display order**: the order in which the items are listed, top to bottom, with every item before its children.
- **Selection**: the items the user has selected (several only in the Select tool, S7). The **acted-on item** is the item a context menu was opened on, or the focused item for a key.
- **Field text**: the search field's text at that moment, typed or committed, as `search/join-and-replace` J2 reads it.

## S2. Sections and items (R1157 to R1164)

Changed 2026-11-20 (round 8): Browse's sidebar sections, order, counts and folding are `browser/workspace` V6; the Today section is replaced by built-in saved searches (`search/filter-tokens` T13). The item searches of S3 stay the built items the values use (T3).

The sections, always in this order, each with its collapse key (`browser/config-keys` K6.2):

| Section | Root's search | Children, in order |
|---|---|---|
| Saved Searches (`collapseSavedSearches`) | none | one item per saved search (S3.1) |
| Today (`collapseToday`) | none | the eight items of S3.2 |
| Flags (`collapseFlags`) | any flag (`-flag:0`) | No flag (`flag:0`); flags 1 to 7 (S3.3) |
| Card State (`collapseCardState`) | none | New `is:new`, Learning `is:learn`, Review `is:review`, Suspended `is:suspended`, Buried `is:buried` |
| Decks (`collapseDecks`) | any deck (`deck:_*`) | Current deck (`deck:current`); the deck tree (S3.4) |
| Note Types (`collapseNotetypes`) | any note type (`note:_*`) | one item per note type (S3.5) |
| Tags (`collapseTags`) | any tag (`tag:_*`) | Untagged (`-tag:_*`); the tag tree (S3.6) |

- Every search in this file is a J3 built item, written by `build` (J4) where a text is needed; the texts above are what J4 writes. A section root with no search, and an item whose J3 item is refused (J3.3, J3.5 without an exact fallback, a negative id), runs nothing: activating it does nothing, it gives no term to a multi-selection (S8.2) and cannot be dropped on Saved Searches (S10.3).
- The Card State items are J3 states. Learning is `is:learn`, Buried `is:buried` (both kinds of burying).
- The labels are Meguri's wording (`browser/browse-strings` W6); R1157, R1159 and R1161 quote them.

## S3. Children

### S3.1 Saved searches (R1158)

- One item per saved search, as `browser/config-keys` K7.1 reads `savedFilters`. The label is the name, the search the parsed-text item of J3 with the stored text and `field_text` unset.
- Order (R1158, Meguri): names compared with the `unicase` comparison (`text/unicode-primitives` U2: full case folding of `unicase` 2.6.0), names equal under it in code point order: `b`, `A`, `c` are listed `A`, `b`, `c`, and `STRASSE` before `Straße` (equal under U2; `T` comes before `t` in code points). Anki lists them in code point order, so `B` comes before `a` there (S15).
- A saved search whose text no longer parses is still listed; activating it reports the parse error (S8.3).

### S3.2 Today (R1159)

| Item | Built item | Text |
|---|---|---|
| Due today | due on day 0 | `prop:due=0` |
| Added today | added in 1 day | `added:1` |
| Edited today | edited in 1 day | `edited:1` |
| Studied today | answered in 1 day, any button | `rated:1` |
| First review | first review in 1 day | `introduced:1` |
| Rescheduled | rescheduled in 1 day | `resched:1` |
| Again today | answered in 1 day, button 1 | `rated:1:1` |
| Overdue | group AND (state due, negation of due on day 0) | `is:due -prop:due=0` |

The meaning of each term is `search/history` and `search/properties` P4; the day boundary is the scheduler day (`search/history` H2).

### S3.3 Flags (R1160)

No flag first, then flags 1 to 7 in number order. A flag item's label is its shown name (`tags/flags-and-marked` F4), its colour the flag's (F1), its search flag N. The root and No flag are not renamable; flags 1 to 7 are (S9.2).

### S3.4 Decks (R1162, R1268)

- Current deck is the J3 current-deck item (`decks/decks` D11 decides which deck it finds when the search runs). It is one item whatever the current deck is (S4).
- Then the deck tree of `decks/decks` D12, with each node's `browser_collapsed` as its expansion (S5): the same nodes, order and default-deck hiding as Home's tree. A filtered deck is marked (its own icon, R1268).
- A deck item's search is the deck-by-name item with its full name. When J3.5 refuses that name, and for a deck one of whose stored levels holds `::` (which the full name cannot show; J3.5 builds no name item for it), the search is the deck-ids item with the deck's id and its descendants' ids.

### S3.5 Note types (R1163)

- One item per note type, in the order of the `unicase` comparison of their names (`text/unicode-primitives` U2). Two note type names are never equal under it (`storage/schema-18`: the name index is unique under `unicase`), so the order is total. Search: note type by name, or by id when J3.5 refuses the name.
- Children of a note type: first its templates in ordinal order, then its fields in ordinal order (`notetypes/records`). A template child's label is the template name and its search the group AND (that note type's item, template by ordinal), written `note:Basic card:1` (J9). A field child's label is the field name and its search the group AND (that note type's item, field present for that field), written `note:Basic Front:_*`; the field term is quoted only when J4 quotes it (`note:Basic "Extra Info:_*"`, J9). A field J3.3 refuses has no search.
- A cloze note type has one template child, like any note type with one template.
- Note type items keep no stored expansion (S5).

### S3.6 Tags (R1164)

Untagged first, then the tag tree of `tags/tags` T13 with each node's collapsed state as its expansion. A tag item's search is the tag-by-name item with its full name (J3, J3.2 for other white space), which also finds its descendants (`search/names-and-ids` N2).

## S4. Item identity (R1167)

Two builds of the sidebar (before and after a change) show the same item when:

| Kind | Same item when |
|---|---|
| tag | same full name |
| saved search, Today item, Card State item | same label |
| template or field child | same ordinal and same note type id |
| section root, No flag, Untagged, Current deck | same kind (one item of each) |
| flag 1–7, deck, note type | same number: the flag number, the deck id, the note type id |

U216's view diffs by this identity (S12). An item renamed in place keeps its identity only where the table says so: a renamed saved search or tag is a new item; a renamed deck, flag or note type is the same item.

## S5. Expansion (R1157, R1162, R1164, R1166)

Each item with children is expanded or collapsed. Where the state lives:

| Item | Stored in | Write |
|---|---|---|
| section root | its collapse key (S2) | `browser/config-keys` K6.2, only on the user's toggle |
| deck | the deck's `browser_collapsed` | `decks/decks` D10, browser scope |
| tag | the tag row's `collapsed` | `tags/tags` T12, with the full name |
| note type | nowhere: collapsed at every build | none |

- The study scope (`study_collapsed`) is Home's and never changes here (R1162).
- Only a user's toggle writes, and only while the filter is off (S6): expansions the filter makes, and toggles made while it is on, are display only. A rebuild (S12) never writes.
- **Context menu actions** (R1166), offered only while the filter is off and when at least one selected item has children:

  | Action | Offered when | Does |
  |---|---|---|
  | Expand | a selected item with children is collapsed | expands every selected item |
  | Collapse | a selected item with children is expanded | collapses every selected item |
  | Expand Children | a direct child (that has children) of a selected item is collapsed | expands every selected item and each of its direct children that has children |
  | Collapse Children | a direct child (that has children) of a selected item is expanded | expands every selected item and collapses each of its direct children that has children |

  Only direct children change, never deeper levels. Each affected item gets one **final state**: a selected item takes the state its action gives selected items (expanded for Expand, Expand Children and Collapse Children, collapsed for Collapse), and a direct child that is not itself selected takes the state the action gives children. A selected item that is also a direct child of another selected item takes its state as a selected item, so Collapse Children on the selection `A`, `A::B` leaves `A` and `A::B` expanded and collapses those of `A`'s other direct children and `A::B`'s direct children that have children. This is Meguri's fixed rule; Anki's result depends on the order it reads the selection (S15).

  One action then writes in this order: first the collapse key of every section root whose final state differs from the stored one, then one D10 call (browser scope) whose list holds one entry per deck whose final state differs from its stored `browser_collapsed`, each with that final state, then one T12 call whose list holds one entry per tag whose final state differs from its stored state (an implicit parent's is collapsed), each with that final state. Items whose final state equals the stored one are left out, and a kind with no entry left gets no call. The entries of one list can hold both states: Collapse Children on a collapsed deck `A` whose child `A::B` is expanded and has children makes one D10 call with `A` expanded and `A::B` collapsed. So the action records at most one "Expand/Collapse" step for decks and one for tags, and the key writes, which clear the undo history (S13), come before those steps and do not clear them. Anki toggles each row on its own (S15).

## S6. The filter box (R1165)

- The filter is **on** while the box's text holds a character that is not White_Space; empty or White_Space-only text turns it **off**, and the sidebar shows the stored expansion again (S5).
- An item **matches** when its label, folded with simple case folding (`search/case-folding` C3), contains the whole box text folded the same way, leading and trailing spaces included (as Anki): ` mam` does not match `mammal`. Deck and tag items match by their label (the last level), not their full name: `mam` matches the tag item `mammal` under `animal`, `animal::m` matches nothing. Section roots match by their labels too.
- While the filter is on (Meguri, R1165): a section is shown when its root or one of its items matches. Within it, an item is shown when it matches or one of its descendants matches; an item with a matching descendant is shown expanded; a matching section root is shown expanded one level (all its direct children shown) as well as with its other matches; nothing else is shown expanded. Matching items are highlighted. No match anywhere shows an empty state (Meguri's wording).
- The filter is applied at once on every change of the text (R1165); after a rebuild (S12) it is applied again to the new items.
- Keyboard focus stays in the box while the user types. The first matching item in display order becomes the list's focused item and is scrolled into view. ↓ or ↑ in the box moves keyboard focus into the list, on that item (R1165 names ↓; Anki takes both). Return in the box does nothing more.
- ⇧⌘F focuses the box and shows the sidebar if it was hidden (R260, R1229).

## S7. Tools (R1168, R1170)

Changed 2026-11-20 (round 8): S7 and S8 are replaced for Browse by `search/filter-tokens` T9: one row click sets its kind's token, ⌘-click adds or removes a value, ⌥-click excludes; no Search and Select tools and no multi-selection.

Two tools, as in Anki: **Search** (the tool at every open) and **Select**.

| | Search | Select |
|---|---|---|
| Selection | one item | several (⌘-click, ⇧-click on Mac and with an iPad keyboard; Select mode on touch, R1170) |
| Click or tap on an item | runs S8.1 for it | changes the selection only |
| Drag and drop | none | S10 |
| Return with the list focused | S8.1 on the focused item | S8.2's Search all selected (U216; Anki runs the focused item alone) |

Anki binds the two tools to Alt+1 and Alt+2. Meguri's keys for them are not fixed yet (plan Open Questions; S15's open points).

## S8. Running items (R1168, R1169, R1170, AE43)

### S8.1 One item

The item's search I is combined with the field text E (S1) by the input that activated it:

| Mac, iPad keyboard (R1168) | Touch (R1169) | Result |
|---|---|---|
| click, Return | tap; long press › Search | `build`(I) (J4): the search is replaced |
| ⌘-click | leading swipe; long press › Add (AND) | `join`(E, I, AND, false) (J6) |
| ⇧-click | long press › Add as alternative (OR) | `join`(E, I, OR, false) |
| ⌥-click | | `build`(negation of I) (J5) |
| ⌥⌘-click | trailing swipe; long press › Exclude (NOT) | `join`(E, I, AND, true) |
| ⌥⇧-click | | `join`(E, I, OR, true) |
| ⌘⇧-click | long press › Replace same kind | `replace`(E, I) (J7) |
| ⌥⌘⇧-click | | `replace`(E, negation of I): J7 replaces nothing for a negation, so the result is E's canonical text |

- When both ⌘ and ⇧ are held, replace wins over joining, as in Anki. ⌥ applies to I before the other modifiers, so ⌥ alone replaces the search with the negated item.
- The result is committed as a search (U210, R1109) and shown in the field.
- Examples (J9, AE43): field `deck:French is:due`, ⌘⇧-click deck `Spanish` gives `deck:Spanish is:due`; ⌥⌘-click tag `verbs` gives `(deck:French is:due) -tag:verbs`; with an empty field, ⌘-click deck `Spanish` gives `deck:* deck:Spanish`.

### S8.2 Several items (R1170)

- **Search all selected** runs S8.1 with I the group AND of the searches of the selected items, **Search any selected** with the group OR; selected items without a search are left out. The items are taken in display order (Meguri; Anki takes them in the order they were selected, S15). A group of one item is that item (J3).
- With a mouse, the menu item is chosen with modifiers held or not, and S8.1's row for those modifiers applies to the group (Anki reads them when the menu item fires). A group is never replaced by kind (J7): ⌘⇧ gives E's canonical text.
- The context menu shows Search for one selected item with a search, and a Search submenu with All Selected and Any Selected for two or more.
- Example: tags `a` and `b` selected, field `deck:x`, Search any selected with ⌘ held gives `deck:x (tag:a OR tag:b)`; without modifiers it gives `tag:a OR tag:b`.

### S8.3 Errors

When E does not parse, `join` and `replace` return its parse error (J1): the search stays as it was and U214 shows the error (`search/errors` E7), as Anki shows a warning and keeps the search. A saved search whose text does not parse reports its error the same way when it is run with `build`; the field text is not changed.

## S9. Item actions (R1171 to R1180)

Changed 2026-11-20 (round 8): in this version Browse offers only Edit Note Type… on a note type row and Rename… and Delete on a user saved search (`browser/workspace` V6); the other actions of S9 and the drags of S10 wait.

### S9.1 Which item offers what

Context menu entries, by kind. "One selected" means the selection is exactly the acted-on item; "same kind" means every selected item has the acted-on item's kind.

| Kind | Actions |
|---|---|
| Saved Searches root | Save Current Search (S9.4) |
| saved search | Update to Current Search (S9.4); Rename, one selected (S9.2); Delete, same kind (S9.3) |
| flag 1–7 | Rename, one selected (S9.2); Restore Default Name (R899, `tags/flags-and-marked` F4) |
| deck | Add Note (S9.5); Rename, one selected (S9.2); Rename with Parents, one selected and the deck is not top-level (S9.2); Delete, same kind (S9.3) |
| tag | Add to Selected Notes and Remove from Selected Notes, same kind (S9.6); Rename and Rename with Parents as for decks (S9.2); Delete, same kind (S9.3) |
| note type, Note Types root | Manage Note Type (S9.7) |
| template child | Edit Template (S9.7) |
| field child | Edit Fields (S9.7); Find and Replace, one selected (S9.7) |
| every item with a search | Search (S8.2) |
| every item with children | the S5 actions |

- Current deck, No flag, Untagged, the Today and Card State items and the other section roots have no management action.
- Restore Default Name is Meguri's (R899): Anki restores the default only through an empty rename. It removes only that flag's member.
- Anki offers Rename with Parents only for a deck or tag that has a parent; Meguri does the same (on a top-level item it would equal Rename).

### S9.2 Rename (R1171, R1173, R1174, R1176)

Inline rename edits the label; Rename with Parents (decks and tags) asks for a whole name, prefilled with the full name.

1. Every `"` is removed from the typed text (Anki does this for every sidebar rename).
2. Flag: an empty result restores the default name; any other result is `tags/flags-and-marked` F4's rename (which does nothing when the result equals the shown name).
3. Other kinds: nothing happens when the result is empty or equals the label (inline) or the full name (Rename with Parents).
4. **Deck**: the new full name is, inline, the parent part of the full name, `::` and the result (the result alone for a top-level deck); with Rename with Parents, the result. Then `decks/decks` D6 with the deck's id and that name; its refusals (`filtered_parent`, `under_descendant`, `too_deep` and the rest) are reported and the label goes back (Meguri; Anki keeps the typed label until the next rebuild, S15).
5. **Tag**: the new name is built the same way from the full name. Then `tags/tags` T7 with the full name as the old name. A count of 0 (no note uses the tag, T7 step 2) puts the label back and shows the warning of R1176; a refusal (`invalid_name`) puts it back too and is reported; otherwise the result message gives the count ("N notes updated", `browser/browse-strings` W6). Anki does the same for an inline tag rename.
6. **Saved search**: `browser/config-keys` K7.2's Rename with the old name and the typed text. When another saved search already has the new name, the user confirms first (R1171); cancelling writes nothing.

The label shows the new name at once and goes back on any refusal, count 0 or cancel (R1176 for tags; Meguri for the other kinds).

### S9.3 Delete (R1174, R1178, R1180, R66, R67)

Offered, and done by ⌫ (Anki: the Delete key) with the sidebar focused, when the acted-on item is a deck, a tag or a saved search and every selected item has its kind. It acts on all selected items:

| Kind | Operation | Confirmation and result |
|---|---|---|
| deck | `decks/decks` D8 with the selected decks' ids | D8.5's summary over the selection: a confirmation stating both counts when it removes at least one card (R67; Anki asks nothing), then the toast "Deleted <deck> (N cards)" with Undo (R66, `decks/decks`), <deck> the acted-on item's label |
| tag | `tags/tags` T9 with the selected tags' full names | none; undoable (R1178); result message with the count |
| saved search | `browser/config-keys` K7.2's Delete Saved Searches with the selected names | none; undoable "Delete Saved Search" |

A mixed selection offers no Delete, and ⌫ does nothing.

### S9.4 Saved searches (R1171, R274)

- **The current search** is the field text E (S1) put in canonical form (`search/canonical-form`), as Anki reads it: typed text counts even before it is committed. When E does not parse, the error is reported (`search/errors` E7) and nothing is saved.
- **Save Current Search**: a name prompt (empty name: nothing happens); then K7.2's Save with that name and the current search. When the name exists with another text, the user confirms first; the same text writes nothing (K7.2). ⌘S does the same (R274).
- **Update to Current Search**: K7.2's Update with the item's name and the current search, with no confirmation.
- Both, and ⌘S, are disabled while the field text is empty or only White_Space (Meguri, R274's "with a search active"): on a plain open the field is empty although `deck:current` runs (screen contract), and nothing is offered. Anki offers them always and saves `deck:*` for an empty field (S15).

### S9.5 Add Note (R1174)

Opens Add with that deck preset (`notes/add-defaults` D1 with `preset_deck`, row "a deck's Add Note in Browse's sidebar").

### S9.6 Tags on the selected notes (R1178)

Add to Selected Notes and Remove from Selected Notes pass the full names of all selected tag items in display order, separated by spaces, with Browse's selected notes (the table's selection, `browser/result-sets` H12) to `tags/tags` T10. Both are disabled when Browse has no selected row (Meguri; Anki offers them and shows a "no selection" hint, S15).

### S9.7 Note types (R1179)

- Manage Note Type opens the note type manager (U200), with that note type selected (from the root: none).
- Edit Template opens the template editor for the template child's note type, on that template's ordinal, previewing a note with empty fields filled (`render/preview` V6).
- Edit Fields opens the fields editor for the field child's note type, on that field's ordinal.
- Find and Replace opens `browser/find-replace` with In preset to that field's name (F2's sidebar case) and Browse's selected notes.

## S10. Drag and drop (R1172, R1175, R1177)

Only in the Select tool. What the dragged selection may be dropped on:

| Dragged selection | Drop targets | Operation |
|---|---|---|
| only deck items (Current deck is not one) | a deck item; the Decks root | `decks/decks` D7 with the dragged decks' ids in display order and the target deck, or the top level for the root |
| only tag items | a tag item; the Tags root | `tags/tags` T8 with the dragged full names in display order and the target's full name, or no target for the root |
| exactly one item that has a search and is not a saved search | the Saved Searches root; a saved search item | S10.3 |

### S10.1 Decks (R1175)

The UI refuses, before calling D7, a drop onto a filtered deck (D7's `filtered_parent`, R1268) and a drop whose target is, for every dragged deck, that deck itself or one of its descendants (D7 step 2.2 would skip them all: deck `A` onto `A::B`), and shows why; no operation is issued. Otherwise D7 runs and skips what it skips. The result message gives D7's count (R64).

### S10.2 Tags (R1177)

T8 decides what moves; merging with an existing tag is T7 step 6. A drop that moves nothing (count 0) changes nothing.

### S10.3 Onto Saved Searches (R1172)

The item is saved by K7.2's Save, as Save Current Search would save it: the name is the item's **label**, the text `build` of its search. The same confirmation applies when the name exists with another text. So dragging the deck item `Audio` under `Japanese` saves `Audio` → `deck:Japanese::Audio`, and dragging Due today saves `Due today` → `prop:due=0` (label text in the user's language). Anki names the saved search after the label, not the full name; see S15 for the plan text that differs.

A deck whose search is the deck-ids item (S3.4) or a note type whose search is the note-type-id item saves that text (`did:…`, `mid:…`). It names the ids at the moment of the drop: it keeps finding the deck after a rename, but a subdeck added later is not in it (J3.5). Meguri saves it all the same, as Anki would save its own text (S15's open points).

## S11. Keyboard (R1180, R1229)

With the list focused: Return runs S7's row; ⌫ deletes (S9.3); the Rename command edits the focused item inline when S9.1 offers Rename; ↑ and ↓ move; → and ← expand and collapse. Return while editing a name commits the rename instead. Meguri's key for the Rename command is not fixed yet (S15's open points). Anki starts an inline rename with its toolkit's edit key (F2 outside macOS; on macOS its Return runs the item instead, so there is no rename key there).

## S12. Refresh (R1167)

- The sidebar is rebuilt off the main thread after every change set that touches decks, tags, note types or a collection config key (Anki redraws its sidebar for exactly those changes); `savedFilters`, `flagLabels` and the `collapse*` keys are the keys whose values it shows. A sync that brings such changes counts (R1268: a filtered deck from another client appears with its mark).
- The rebuild keeps the focused item, the selection and the scroll position by S4's identity (R1167, Meguri); a selected or focused item that no longer exists is dropped from them. The expansion is the stored one (S5) unless the filter is on (S6 is applied again). Anki keeps only the focused item: it becomes the whole selection and is scrolled to the middle (S15).
- A rebuild changes nothing in the collection.

## S13. Undo

- Each of these writes is the operation's own undo step, with the names `decks/decks` D14, `tags/tags` T15, `tags/flags-and-marked` F4 and `browser/config-keys` K10.2 give: "Rename Deck" for a deck rename and move, "Delete Deck", "Rename Tag" for a tag rename and reparent, "Remove Tag" for a tag delete and for Remove from Selected Notes, "Update Tag" for Add to Selected Notes, "Rename Flag", the saved-search steps of K10.2, and "Expand/Collapse" for decks and tags. Each is one entry on the app's undo stack (R1182, U25).
- Section collapse keys are not undo steps: K10.2's first row makes each a non-undoable write, which records no entry and, when it changes the key, clears the undo and redo history. S5 orders a mixed action's writes so that this never clears the steps of the same action.

## S14. Strings

Every label and message here is Meguri's wording or a requirement's (`browser/browse-strings` W6); flag names are `tags/flags-and-marked` F1 and F4's. Anki's sidebar strings are not used.

## S15. Differences from Anki

| Difference | Anki | Meguri | Why |
|---|---|---|---|
| Saved search order | code point order of names | `unicase` (U2), then code point (S3.1) | R1158 |
| Note type order | the names lowered with Python's lowercase mapping, ties in the order the note types are listed | the `unicase` comparison (S3.5); this differs where lowercasing and full folding differ: `Strasses` comes before `Straße` in Anki, after it in Meguri | one comparison for every name list (`decks/decks` D12, `tags/tags` T13) |
| Current deck identity | the current deck's id, so after the current deck changes it is a new item and loses the focus | one item by kind (S4) | R1167 |
| Filter display | the tree is collapsed, then the ancestors of each match (and a matching section root one level) are expanded; every child of an expanded item is shown, matching or not, and items under collapsed items are hidden; with no match the whole list is hidden; a 600 ms pause before filtering | only matches and their ancestors are shown; an empty state when nothing matches; no pause (S6) | R1165 |
| Filter's first focused match | the first match met after its children (deepest first) | the first in display order (S6) | simpler; not observable in the collection |
| Filter folding | Python lowercasing | simple case folding (S6) | `search/case-folding` |
| Expand Children / Collapse Children | each row toggled on its own, each deck or tag written alone; a selected item that is also the direct child of another selected item ends in the state of whichever of the two Anki reads last from its selection | one list per kind, holding each item's final state, one undo step per kind; such an item takes its state as a selected item (S5) | `decks/decks` D10, `tags/tags` T12; a result that does not depend on selection order |
| Order of a multi-selection | the order the items were selected, for searches, drops and the tag actions | display order (S8.2, S9.6, S10) | the result does not depend on click order (as `browser/bulk-actions` B1) |
| Return in the Select tool | searches the focused item | Search all selected (S7) | U216 |
| Restore Default Name | none (only an empty rename) | a menu item (S9.1) | R899 |
| Deck rename refused | the typed label stays until the next rebuild | the label goes back at once (S9.2) | R1176's rule for tags, applied to decks |
| Deck delete | no confirmation | a confirmation when a card goes (S9.3) | R67 |
| Save Current Search, Update, with an empty field | offered; saves `deck:*` | disabled (S9.4) | R274 |
| Tag actions with no selected row | offered; a "no selection" hint | disabled (S9.6) | `browser/bulk-actions` B1 |
| Deck drop onto a filtered deck, or onto the dragged deck itself or one of its descendants | the operation runs and fails or skips | refused in the UI with a reason (S10.1) | R1175 |
| Rebuild | keeps only the focused item, which becomes the selection and is scrolled to the middle | keeps the focused item, the selection and the scroll position (S12) | R1167 |
| Undo of sidebar writes | none of them records an undo step or clears the history | saved-search writes, flag renames and deck and tag expansion one step each; section collapse keys clear the history (S13) | R2020, `browser/config-keys` K10.2 |

Open points for Antoine:

1. U217's test "Drag deck item `Japanese::Audio` onto Saved Searches → saved search `Japanese::Audio` with `deck:"Japanese::Audio"`" names the saved search after the full name and quotes the text. Anki names it after the item's label (`Audio`), which S10.3 follows, and J4 writes `deck:Japanese::Audio` without quotes. R1172 ("named after the item") allows either name; until Antoine decides, S10.3 stands. The text is a correction either way.
2. The keys of the Search and Select tools (S7) and of the sidebar's Rename command (S11). Proposed: ⌥1 and ⌥2 with the sidebar focused (Anki's Alt+1 and Alt+2 read by R1231's mapping), and ⇧⌘R with the sidebar focused (R225's Rename key, free in Browse because Meguri leaves Anki's sidebar-focus command unbound, `browser/command-map` M5).
3. Dragging a deck or note type whose search is an id list onto Saved Searches (S10.3): saved as it is (the default above), or refused because the saved text would not include later subdecks.

## S16. Scenarios and tests

The oracle runs Anki's collection library, not its sidebar, so only the searches the items build are recorded. Both scenarios have goldens (`conformance/goldens/browser/sidebar/`) and are not yet in `browser/area.yaml`'s `active` list; they become active with U88's `search_build`, `search_join` and `search_replace` adapter, together with the search area's builder and join-replace scenarios:

| Scenario | Covers |
|---|---|
| `browser/sidebar/item-searches` | S2, S3: `search_build` of every fixed item, of a deck, tag, note type, template and field item, and the rows each search finds on a small collection |
| `browser/sidebar/combine` | S8: the modifier rows of S8.1 and the groups of S8.2 through `search_join` and `search_replace` |

Meguri's tests (U216, U217; files in the plan): the order and identity rules (S3, S4) in `SidebarModelTests`; S5 and S6 in `SidebarModelTests` and `SidebarFilterTests`; S8 in `SidebarSearchComposerTests` against the J9 examples; S9 and S10 in `SidebarActionsTests`, `SavedSearchesEditorTests` and `SidebarDragDropTests`, each asserting the operation called and its input.
