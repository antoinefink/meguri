# Tags

Spec: tags/tags
Requirements: R770, R891, R892, R893, R894, R895, R896, R1176, R1177, R1178, R1190, R1191, R1467; supports R1164, R1166, R1831, R2020
Anki: 26.09.3
Scenarios: conformance/scenarios/tags/

A note carries tags: names whose levels are joined by `::`. The note row stores them in `notes.tags`; the collection's tag registry is the `tags` table (`model/row-records#R8`). This spec gives how a tag name is normalized and spelled, what a note save stores, the management operations (rename, reparent, delete, clear unused, add to and remove from notes, collapsed state), the tree built from the registry, and the stamps and undo names of each operation. Flags and the `marked` tag are in `tags/flags-and-marked`. The scenario ops are `specs/conformance/tag-ops.md`.

Stamping terms (`usn` -1, `now_s`, `now_ms`, `col.mod`) are those of `storage/ids-usn-mtime`. "Equal" for tag names always means equal under `text/unicode-primitives#U2` (full case folding), except in T5, which says otherwise.

## T1. Terms

- **Separators** are U+0020 SPACE and U+3000 IDEOGRAPHIC SPACE. A stored tag never contains one.
- **Components.** A name splits into components at each `::`, scanning left to right without overlap: `a::b` gives `a`, `b`; `a:::b` gives `a`, `:b`; `a::::b` gives `a`, (empty), `b`; `::` gives two empty components.
- **Ancestors.** The ancestors of a name with n components are the names made of its first k components, for k from n − 1 down to 1: `a::b::c` has `a::b` (its parent) and `a`. Descendants are the converse.
- **Registered.** A name is registered when a `tags` row is equal to it. Ancestors are never registered implicitly: a row `Bar::x` does not register `Bar`.
- **Nodes.** A name is a node when it is registered or is an ancestor of a registered name. A node that is not registered is an **implicit parent**: `Bar` for a row `Bar::x`. The nodes are what the tag tree shows (T13).
- **Tree spelling.** A registered node is spelled as its row's `tag`. An implicit parent P is spelled as its leading components are spelled in the first row, in U2 order of `tag`, that has an ancestor equal to P: rows `geo::EU::fr` and `GEO::eu::de` spell the implicit parents `GEO` and `GEO::eu`, since `GEO::eu::de` comes first.
- **Trim set.** The characters trimmed from the ends of a component (T2 step 3), which are the Unicode White_Space characters that step 2 has not already removed: U+0085, U+00A0, U+1680, U+2000–U+200A, U+2028, U+2029, U+202F, U+205F.
- **White space** (for T7's refusal): the trim set plus U+0009–U+000D, U+0020 and U+3000.
- **Tag text.** Where an operation takes several tags as one text, the text is split at separators and empty pieces are dropped.

## T2. Normalizing a name

Every name that enters the registry or a note goes through these steps, component by component, and the components are joined again with `::`:

1. NFC (`text/unicode-primitives#U3`).
2. Remove every ASCII control character (`text/unicode-primitives#U1`) and every separator.
3. Remove characters of the trim set from both ends.
4. A component left empty becomes `blank`.

NFC runs before the removal, so a control character between a letter and a combining mark keeps them apart. The result is never empty. Quotation marks and every other character are kept.

| Input (one name) | Normalized |
|---|---|
| `x` TAB `y` | `xy` |
| `a b` (an operation that does not split, such as T12) | `ab` |
| `a::::b` | `a::blank::b` |
| `a:::b` | `a:::b` |
| `h::` | `h::blank` |
| `::` | `blank::blank` |
| `::lead` | `blank::lead` |
| U+00A0 `q` U+00A0 | `q` |
| `r` U+00A0 `s` | `r` U+00A0 `s` |
| U+00A0 | `blank` |
| U+0085 `p` U+0085 | `p` |
| U+2028 `z` | `z` |
| `t` U+200B `u` | `t` U+200B `u` (U+200B is not white space) |
| `e` U+0301 | `é` (U+00E9) |
| `e` U+0001 U+0301 | `e` U+0301 |
| `A` U+030A U+0002 | `Å` (U+00C5) |
| U+0001 | `blank` |
| `k::` U+0001 `::m` | `k::blank::m` |
| `say"hi"` | `say"hi"` |

## T3. Spelling and registration

Given a normalized name N:

1. If a row is equal to N, N takes that row's spelling, and nothing is registered.
2. Otherwise, find N's longest leading part that is a node (T1), trying N itself first, then its ancestors from its parent towards its first component. That part of N takes the node's tree spelling, and N's later components keep theirs. When no leading part of N is a node, N keeps its spelling. N is then registered: a new row with `tag` N, `usn` -1, `collapsed` 1, `config` NULL (T7 and T8 give a renamed tag's row other columns).

The registry is read as it is at that moment, so a tag registered earlier in the same operation counts.

| Registry before | Name | Result | Registers |
|---|---|---|---|
| `German::Verb` | `german::verb::modal` | `German::Verb::modal` (AE37) | yes |
| `German::Verb` | `GERMAN::VERB` | `German::Verb` | no |
| `German::Verb` | `german::noun` | `German::noun` (implicit parent `German`) | yes |
| `Bar::x` | `bar::y` | `Bar::y` (implicit parent `Bar`) | yes |
| `Bar::x` | `BAR` | `Bar` (N is itself an implicit parent) | yes |
| `LANG` | `lang::DE` | `LANG::DE` | yes |
| `X`, `X::Y::q` | `x::y::z` | `X::Y::z` (the longest node is the implicit `X::Y`) | yes |
| `geo::EU::fr`, `GEO::eu::de` | `Geo::Eu::it` | `GEO::eu::it` (tree spelling, T1) | yes |
| `STRASSE` | `Straße` | `STRASSE` (equal under full folding) | no |

Deviation (R892). Anki gives N the spelling of a row equal to N or to one of its ancestors, and an implicit parent lends nothing: beside a row `Bar::x` it stores and registers `bar::y` and `BAR` as given, and with rows `X` and `X::Y::q` it spells `x::y::z` as `X::y::z`. Meguri spells from every node the tag tree shows, so a new tag joins the tree under the spelling the user sees there (R892: a new tag under an existing parent adopts the parent's spelling). Rows that Anki or another client wrote keep their spellings; T13 shows them in one tree either way. Deviations `tags-implicit-parent-spelling` and `tags-implicit-parent-spelling-registry` (scenario `tags/canon/spelling`), `tags-implicit-parent-collapse` (`tags/collapse/set`, T12), and `tags-implicit-parent-rename` and `tags-implicit-parent-rename-registry` (`tags/rename/implicit-parent`, T7).

## T4. A note's tags when the note is saved

Adding a note and saving a note with a tag list (the editor, the Add screen, AnkiConnect, U27's note ops) store the list this way (R770, R891–R893). Importers follow their own specs (U120, U121).

1. Split every item of the list at separators and drop empty pieces, so an item `kana kanji` gives two tags and an item made only of separators gives none.
2. In list order, normalize each tag (T2) and give it its spelling, registering it when new (T3). A tag registered earlier in the same list lends its spelling to later ones, below it and above it: `LANG`, `lang::DE`, `lang::de::noun` store `LANG`, `LANG::DE`, `LANG::DE::noun`; and `zz::Q`, `ZZ` store `zz` and `zz::Q`, since `zz::Q` makes `zz` an implicit parent (Anki stores `ZZ`, T3's deviation).
3. Drop tags equal to an earlier one.
4. Sort by `text/unicode-primitives#U2`.
5. Store a space, the tags joined by single spaces, and a space; with no tags, the empty string.

| Tags given (fresh registry) | Stored |
|---|---|
| `b`, `A`, `a` | ` A b ` |
| `Straße`, `STRASSE` | ` Straße ` |
| `z`, `_x`, `[`, `Y` | ` [ _x Y z ` |
| `a` U+3000 `b` TAB `c"d` | ` a bc"d ` |
| (empty string), U+3000 | (empty string) |

Every tag a note stores is registered after the save (R893). Removing a tag from a note never removes its row: rows go only through T9 and T11.

## T5. Matching tags against names

Rename, reparent, delete and removal from notes act on the tags that match one of a list of names:

- A tag matches a name when the tag equals the name or begins with the name followed by `::`, comparing characters case-insensitively by Unicode 16.0 simple case folding: `É` matches `é`, `K` and U+212A KELVIN SIGN match `k`, but `ß` does not match `ss` (the full folding of U2 is not used here). Every character is literal: `c++` matches only `c++` and its descendants, and `mix*` only `mix*`.
- `foo` matches `foo`, `FOO::kid` and `foo::a::b`, never `foobar`, `bar::foo` or `foo:`.
- With several names, the first name in list order that matches a tag decides how T8 renames it.
- A note matches when one of its stored tags (T6 step 1) matches.

## T6. Rewriting a note's tags in place

Rename, reparent, delete and removal from notes rewrite only the notes that match (T5):

1. Split the stored text at separators and drop empty pieces.
2. Replace (T7, T8) or drop (T9, T10) each matching tag; other tags stay as they are.
3. Join as T4 step 5, keeping the order and any duplicates: no sorting, no deduplication. ` bar foo ` with `foo` renamed to `bar` becomes ` bar bar `; ` a zzz ` with `a` renamed to `zzz2` becomes ` zzz2 zzz `. The next save of the note (T4) sorts and deduplicates.

A rewritten note gets `usn` -1 and `mod` `now_s`. Meguri does not write a matching note whose new text equals the stored text (`storage/ids-usn-mtime#S4`, R896); Anki writes and counts it. No scenario shows this, as it needs a stored spelling that differs from the registry's.

Each operation reads the notes' tags as they are when it writes (U24's write phase), so a note saved meanwhile is rewritten from its new text.

## T7. Rename (R894, R1176)

Input: the old name (one name, as the registry spells it) and the new name.

1. Refuse when the new name contains a separator, or is empty or made only of white space (T1). Refusal kind `invalid_name`; nothing changes. Other control characters are not refused: they are removed (U+0001 alone becomes `blank`).
2. When no note matches the old name (T5), nothing changes and the count is 0. The rows of the old name and its descendants stay, and the UI warns that only tags used by notes can be renamed (R1176).
3. Remove every row matching the old name (T5): its own row and every registered descendant, also those no note uses. A descendant that no note uses is not created again under the new name: its row and its columns are gone, as in Anki.
4. For each matching tag, in ascending note id order and stored tag order within a note, the new tag is the new name followed by the rest of the old tag after its matched part (nothing, or `::` and the descendant's own components as the note spells them). Normalize it (T2) and give it its spelling against the registry as it is now (T3 steps 1 and 2; step 5 below registers it in place of T3).
5. When that spelling is not registered, register it with the columns of its source: the row that step 3 removed and that is equal to the matching tag. The new row gets the source's `collapsed` and `config`, and `usn` -1. When step 3 removed no row equal to the matching tag (a tag no row lists, as another client can store), the new row gets T3's columns. When several matching tags give one new name, the first of them in step 4's order registers it, so its source gives the columns; the later ones find the row registered (T3 step 1).
6. A new tag equal to a row that step 3 left is a merge: that row keeps its spelling and all its columns, and the source's columns are dropped.
7. Rewrite the notes (T6) with those spellings. The count is the number of notes written.

Example (scenario `tags/rename/basic`):

| Note before | After renaming `foo` to `bar` |
|---|---|
| ` foo other ` | ` bar other ` |
| ` foo::child FOO::kid ` | ` bar::child bar::kid ` |
| ` foobar ` | unchanged |
| ` bar foo ` | ` bar bar ` |

- A rename that changes only letter case works: `verbs` → `Verbs` stores `Verbs` and `Verbs::x` (step 3 removes the old rows first), and each new row has its old row's `collapsed` and `config` (step 5): an expanded `verbs` stays expanded as `Verbs`.
- Renaming onto a registered name merges into it (step 6): with rows `foo`, `foo::a` and `bar`, renaming `foo` to `bar` keeps the row `bar` as it was, drops the columns of `foo`, and registers `bar::a` with the columns of `foo::a`.
- Renaming onto an implicit parent takes its tree spelling (T3): `foo` → `bar` with a row `Bar::x` stores and registers `Bar` and `Bar::c`.

Deviations (R1176, R892, R1467). Anki spells the new name with the old rows still registered and writes that exact text into the notes, and only the registry rows get its spelling rule; those rows are new rows, collapsed with `config` NULL. Meguri spells every new tag with T3 after step 3, writes the same spelling into notes and registry, and gives each new row its source's columns:

- New name below the old one: `verbs` → `Verbs::regular` stores `Verbs::regular` and `Verbs::regular::x`; Anki stores `verbs::regular` (from the old row). Deviation `tags-rename-new-casing`, scenario `tags/rename/new-casing`.
- New name equal to a registered tag spelled differently: `foo` → `bar` with a row `Bar` stores `Bar` and `Bar::c` in the notes; Anki stores `bar` and `bar::c` in the notes while its rows are `Bar` and `Bar::c`. Deviation `tags-rename-existing-spelling`, scenario `tags/rename/existing-spelling`.
- New name equal to an implicit parent: `foo` → `bar` with only a row `Bar::x` stores and registers `Bar` and `Bar::c`; Anki stores and registers `bar` and `bar::c`. Deviations `tags-implicit-parent-rename` and `tags-implicit-parent-rename-registry`, scenario `tags/rename/implicit-parent`.
- Columns: Anki's row for a renamed tag is collapsed with `config` NULL, whatever the old row held. Deviations `tags-rename-carries-columns` (scenario `tags/rename/rows`: `bar::a` stays expanded like `foo::a`) and `tags-rename-carries-columns-case-only` (`tags/rename/case-only`: a case-only rename of expanded rows with a `config`).

UI callers (U217): inline Rename edits the last component, so the new name is the parent part followed by the typed text; Rename with Parents takes a whole name. Browse's sidebar removes every `"` from the typed text, and does nothing when the result is empty or equals the current name. The result message reports the count ("N notes updated", R1176).

## T8. Reparent (R894, R1177)

Input: a list of source names (as the registry spells them) and a target name, or no target for the top level.

1. For each source, its new name is:
   - with a target: the target, `::`, and the source's last component; but the source is skipped when the target begins with the source's exact text and the two share their first component exactly (onto itself or onto a descendant), and when the new name is exactly the source;
   - with no target: the source's last component; skipped when that is exactly the source.

   The comparisons are by exact characters, as Anki makes them. So `a::b` onto `a::bc` is skipped too (a text prefix with the same first component), while `foo` onto `foobar` is not.
2. Drop the skipped sources. When none is left, or no note matches a remaining source (T5), nothing changes and the count is 0.
3. Remove every row matching a remaining source (T5).
4. For each matching tag, the first remaining source in list order that matches it gives the new tag: that source's new name followed by the rest of the tag after the matched part. Normalize it (T2) and give it its spelling (T3), in the order of T7 step 4, and register it or merge it as T7 steps 5 and 6 do: a new row takes the columns of the row step 3 removed for the matching tag.
5. Rewrite the notes (T6). The count is the number of notes written.

| Sources | Target | Result |
|---|---|---|
| `foo` | `bar` | `bar::foo`, with `foo::x` → `bar::foo::x` |
| `foo::bar` | `baz` | `baz::bar` |
| `foo::bar` | top level | `bar` |
| `foo` | `foo::bar` | skipped |
| `foo` | `foo` | skipped |
| `foo::bar` | `foo` | skipped (same name) |
| `a::b` | `a::bc` | skipped |
| `wa` | `wab` | `wab::wa` |
| `Foo::bar` | `foo` | `foo::bar` |
| `foo`, `x::foo` | `p` | both `p::foo`; a note with both stores ` p::foo p::foo `; the row `p::foo` takes the columns of whichever of `foo` and `x::foo` comes first in T7 step 4's order |
| `foo`, `foo::bar` | `p` | `foo::bar::x` → `p::foo::bar::x` (the first source decides) |
| `foo::bar`, `foo` | `p` | `foo::bar::x` → `p::bar::x` |
| `a::x` | top level, with `x` registered | merges into `x` |

Deviations:

- Anki keeps skipped sources in its matching and fails the whole operation with an internal error when a note has a tag matching a skipped source while another source remains. Meguri moves the remaining sources. The oracle cannot record that failure, so no scenario shows it.
- Anki writes the new text into the notes as built in step 1, and only the registry rows get T3's spelling. Meguri writes T3's spelling into both: `foo` onto `bar` with a row `bar::FOO` stores `bar::FOO`, where Anki's note has `bar::foo`. Deviation `tags-reparent-existing-spelling`, scenario `tags/reparent/existing-spelling`. A target that is not a registered spelling (one with separators or empty components, which no UI produces) goes through T2 and T3 in Meguri; Anki writes it raw into the notes. No scenario shows that.
- As in T7, Anki's rows for moved tags are new rows, collapsed with `config` NULL, and an implicit parent lends them no spelling (T3); Meguri gives them their sources' columns and T3's spelling. No reparent scenario shows either (every moved row in `tags/reparent/rules` has the new-row columns and no implicit parent differs); Meguri's tests do.

Sidebar drag and drop (U217) passes the dragged tags' and the target's names as the tree shows them (T13), or no target for the Tags root.

## T9. Delete (R894, R1178)

Input: tag text (T1), usually the selected tags' names.

1. Every note with a matching tag (T5) loses all its matching tags (T6). A note left with no tags stores the empty string.
2. Every row matching a name is removed, also when no note matched; that alone is a change (`col.mod` moves).
3. The count is the number of notes written.

Deleting `Ink` removes `ink`, `ink::pen`, `ink::pen::nib` and a registry-only `ink::zz` from notes and registry, and keeps `inky`. The UI confirms nothing: the step is undoable (R2020).

## T10. Adding tags to notes and removing them (R1178, R1190, R1192)

Browse's Add Tags and Remove Tags, the sidebar's Add to / Remove from Selected Notes, marking (`tags/flags-and-marked#M2`) and AnkiConnect (U141) use these two operations. Listed notes that no longer exist are skipped.

**Add.** Input: notes and tag text.

1. Split the text (T1), then normalize (T2) and spell each tag, registering new ones (T3), in order. Empty text changes nothing.
2. For each listed note: when some of these tags is not equal to any tag the note stores, the note's tags become its stored tags plus the missing ones, deduplicated (the first occurrence kept) and sorted by U2, joined as T4 step 5. Otherwise the note is untouched, even when its stored text is unsorted.
3. The count is the number of notes written. Registration (step 1) happens even when no note changes.

| Stored | Add `verb idiom KANA mix` TAB `up` (rows `VERB`, `kaNA`, `idiom`) |
|---|---|
| ` art kaNA VERB ` | ` art idiom kaNA mixup VERB ` |
| (empty) | ` idiom kaNA mixup VERB ` |
| ` zz a zz ` (stored by another client) | ` a idiom kaNA mixup VERB zz ` |
| ` idiom kaNA mixup VERB ` | unchanged |

**Remove.** Input: notes and tag text. Each listed note that matches (T5) loses its matching tags, descendants included (T6). Rows are untouched. The count is the number of notes written. ` c b a b ` without `a` becomes ` c b b `.

## T11. Clear Unused Tags (R895, R1191)

1. The used names are every tag in every note's stored text (split at separators) and every ancestor of each.
2. Remove every row not equal (T1, full folding) to a used name. Kept rows keep all their columns. Notes are untouched.
3. The count is the number of rows removed.

Rows `a`, `a::b`, `a::c`, `q`, `z` with notes using `a::c` and `z`: `a::b` and `q` go, `a` stays as the parent of a used tag. A row `Straße` stays while a note stores `STRASSE`.

## T12. Collapsed state (R1164, R1166, R1467)

Input: a list of entries, each a name and a state (collapsed or expanded). Browse's sidebar disclosure passes one entry; Expand Children and Collapse Children (`browser/sidebar` S5) pass one entry per tag they change, and the states can differ within the list. A name listed more than once gets the state of its last entry.

For each entry in order:

1. Normalize it (T2; separators inside are removed, not split) and spell it with T3, registering it when new. So expanding an implicit parent (T13), which has no row, registers it with its tree spelling (T1), whatever case the name was given in. Anki registers the name as given (T3's deviation, `tags-implicit-parent-collapse`); the sidebar passes the tree spelling, so the two agree there.
2. Set the row's `collapsed` to its entry's state (1 collapsed, 0 expanded). When that changes the value, the row also gets `usn` -1. Its `tag` spelling and `config` stay.

Deviations from Anki, shown by no scenario: Anki writes `usn` -1 even when `collapsed` already has the requested value; and when step 1 changes the given text, it registers the normalized row but leaves its `collapsed` at the new-row value. Meguri skips the unchanged row (`storage/ids-usn-mtime#S4`) and sets the state on the row step 1 found or made.

Undo (plan Open Questions, U216): the whole list is one undoable step named "Expand/Collapse", whatever states it mixes; undo restores every row it changed. Anki records no undo step for it and keeps its undo steps.

## T13. The tag tree (R1164)

Browse's sidebar shows the registry as a tree (U84's vocabulary snapshot, U216). Each node has a component name, a collapsed state and ordered children.

1. Nodes (T1): one per row, collapsed as its `collapsed`; and one per implicit parent, counted once however many rows have it as an ancestor. An implicit parent is collapsed. Each node shows the last component of its tree spelling (T1).
2. A node's parent is the node of its name's parent (T1).
3. Children are ordered by U2 of their component names, which is what sorting the full names with `::` read as U+001F gives: `lesson::extra` comes right under `lesson`, and `lesson9` after `lesson`.

| Rows | Tree |
|---|---|
| `geo::EU::fr`, `GEO::eu::de` | `GEO` › `eu` › (`de`, `fr`); `GEO` and `eu` implicit |
| `lesson`, `lesson9`, `lesson::extra` | `lesson` › `extra`; `lesson9` |
| `Bar::x`, `bar::y` (written by another client, T3) | `Bar` › (`x`, `y`) |
| `a!`, `a::b`, `B`, `b::c` | `a` › `b`; `a!`; `B` › `c` |

The sidebar runs `tag:<full name>` for a node and `tag:_*` for the Tags root (search area, U80). It gives T8 and T12 the names as this tree spells them. The sidebar's tag items, their actions and drops are `browser/sidebar` S3.6, S5, S9 and S10.

## T14. Rows, stamps and preservation (R1467)

- A new row: `usn` -1, `collapsed` 1, `config` NULL; except a row that T7 or T8 registers for a renamed or moved tag, which takes its source's `collapsed` and `config` with `usn` -1 (T7 step 5).
- A row that an operation keeps keeps its `collapsed` and `config`. Only T12 changes `collapsed`; nothing in Meguri changes `config`, which Meguri reads as opaque bytes (`model/protobuf-records`) and copies unchanged when it carries it to a renamed row.
- A removed row leaves no grave (`storage/graves`): removing a tag never reaches other devices through sync, which only adds tags.
- Written notes: `usn` -1, `mod` `now_s`. An operation that changes at least one row sets `col.mod` to `now_ms` once; one that changes nothing leaves it (`storage/ids-usn-mtime#S5`). T9 changes a row even when its count is 0.
- Rows with `usn` -1 are sent by the next sync (U107).

## T15. Undo (R896, R2020)

Each operation of this spec is one undoable step on the shared stack (U25), however many notes and rows it changes. Undo puts back every row the step changed, added or removed: notes (`tags`, `mod`, `usn`), registry rows with all their columns, and `col.mod`.

| Operation | Step name |
|---|---|
| T10 Add | Update Tag |
| T10 Remove | Remove Tag |
| T9 Delete | Remove Tag |
| T7 Rename | Rename Tag |
| T8 Reparent | Rename Tag |
| T11 Clear Unused Tags | Clear Unused Tags |
| T12 Collapsed state | Expand/Collapse |

The names are Anki's operation names, except "Expand/Collapse", which is Meguri's (T12). Anki's undo of a removed row inserts it again without its `config`; Meguri restores it whole (R1467). No scenario shows that until the undo area adds an undo op (U25).

## T16. Scenarios

Every scenario under `conformance/scenarios/tags/` has a golden recorded by the oracle. The deviations named in the last column are the entries of `conformance/deviations.yaml` that apply to the scenario.

| Scenario | Covers | Deviations |
|---|---|---|
| `tags/canon/normalize` | T2, T4 | |
| `tags/canon/spelling` | T3, T4 (AE37), rows kept by registration (T14) | `tags-implicit-parent-spelling`, `tags-implicit-parent-spelling-registry` |
| `tags/rename/basic` | T5, T6, T7, stamps | |
| `tags/rename/case-only` | T7 (case-only rename; columns of the renamed rows, step 5) | `tags-rename-carries-columns-case-only` |
| `tags/rename/refused` | T7 steps 1 and 2, normalization of the new name, literal and simple-folding matching | |
| `tags/rename/rows` | T7 steps 3, 5 and 6: rows removed, merged and registered | `tags-rename-carries-columns` |
| `tags/rename/new-casing` | T7, new name below the old one | `tags-rename-new-casing`, `tags-rename-new-casing-registry` |
| `tags/rename/existing-spelling` | T7, new name registered with another spelling | `tags-rename-existing-spelling` |
| `tags/rename/implicit-parent` | T3 and T7, new name an implicit parent (the plan's `Bar::x` case) | `tags-implicit-parent-rename`, `tags-implicit-parent-rename-registry` |
| `tags/reparent/rules` | T8 table | |
| `tags/reparent/existing-spelling` | T8, new name registered with another spelling | `tags-reparent-existing-spelling` |
| `tags/delete/children` | T9 | |
| `tags/bulk/add` | T10 Add | |
| `tags/bulk/remove` | T10 Remove | |
| `tags/clear-unused/parents` | T11 | |
| `tags/collapse/set` | T12 | `tags-implicit-parent-collapse` |
| `tags/tree/structure` | T13 | |
| `tags/find-replace/listed-notes` | Find and Replace in Tags (`browser/find-replace` F7) on listed notes; T3 registration, old rows kept (T14) | active (U86, `browser/find-replace` F12; changed 2026-11-21, implementation review) |
| `tags/find-replace/all-notes` | F7 on all notes; a child tag keeps its later components | active (U86, `browser/find-replace` F12; changed 2026-11-21, implementation review) |
| `tags/find-replace/pattern-modes` | T10 Add (a second add of a tag the note already has writes nothing, count 0); F7 pattern modes and empty results dropped (T4) | active (U86, `browser/find-replace` F12; changed 2026-11-21, implementation review) |
| `tags/find-replace/invalid-regex` | F7 step 2's refusal (F4 step 5), nothing written or registered; the same text in literal mode writes nothing (count 0) | active (U86, `browser/find-replace` F12; changed 2026-11-21, implementation review) |

Meguri's own tests (U30) cover what no scenario can show: T6's skipped unchanged notes, T8's skipped sources mixed with remaining ones, T12's deviations, T15's undo (one step restores a 10k-note rename in a 150k-note collection, `config` included), and the R1831 budget for bulk operations (U12 bench). They also cover these T3, T7 and T8 cases: a reparent whose moved rows carry their columns; two matching tags giving one new name (`foo` and `x::foo` onto `p`, where the first in T7 step 4's order gives the columns); a matching tag that no row lists (new-row columns); a merge into a row that step 3 left; and a reparent onto an implicit parent (`foo` onto `bar` with only `Bar::x` gives `Bar::foo`).
