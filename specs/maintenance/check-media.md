# Check Media and the media trash actions

Spec: maintenance/check-media
Requirements: R1645, R1646, R1647, R1648, R1649, R1650, R1651, R2018; supports R989, R1584, R1585, R2008, R2020, AE79
Anki: 26.09.3
Scenarios: conformance/scenarios/maintenance/check-media/ (listed in C22; none recorded yet)

What Check Media finds and fixes, and the four actions its report offers: Tag Missing, Delete Unused, Restore Deleted and Empty Trash. The media store it works on (names, the add path, lookup, tracking, the trash) is `media/media-files`; the journal, the media-job lease and the CloudKit file-fact feed are `media/media-journal`. This file states what Check Media decides on top of them. Where Meguri departs from Anki 26.09.3, C20 says so.

## C1. Terms

- **Store, journal, tracked file, syncable file, case key, case variants, transform, normalized, lookup, trash**: `media/media-files` M2, M5, M7.1, M10. **Lease**: the media-job lease (`media/media-journal#J5`), shared with Anki media sync.
- **Limit**: 104,857,600 bytes (100 MiB). A file is **oversize** when its size is above the limit, **empty** when its size is 0.
- **Reference**: one match of `text/media-references#M4` in a note field that is not remote (M5 there), resolved by C6. Its **name** is the resolved name. References in templates (C9) are kept apart: they protect files but are never missing.
- **Renamed map**: the old exact name → new name pairs that C4 and C5 produce in one run, with T5's NFC renames (C2 step 2). Its keys are exact names as the folder lists them. T5's pairs serve C6 and the report only; only C4's and C5's old names are trashed (C10 step 5).
- **Report**: C12. One run that finishes produces one report and replaces the earlier one. A cancelled or failed run keeps the earlier report (C2); closing the collection drops it.

## C2. The run

Check Media is a job (`engine/jobs#J1`): it returns a handle with a cancel token and progress, runs on the job pool, and ends with one `JobFinished`. It takes no long-operation slot (`engine/lifecycle`, Check Media row), so reviews, edits and CloudKit work go on while it runs.

Steps, in order:

1. **Lease.** Take the lease. While media sync holds it, wait (phase `waiting_for_media`). Holding it keeps media sync out until the run ends, so no Anki download or upload overlaps a rename. CloudKit media work takes no lease and may run meanwhile (`media/media-journal#J5`); C14 and C10 step 5 check each name again for that reason (C24).
2. **Registration.** A full comparison of the folder (`media/media-files#M7.3`, `sync/media-tracking#T4`, T5), hashing only files whose size or time changed (R1573, R1651). It renames non-NFC names to NFC (T5). Decided 2026-10-27: for Check Media the full comparison also returns T5's old → new pairs and the non-NFC names that T5 step 4 skipped, an extension of T6's report (which gives only counts) that U31 provides. The pairs go into the renamed map, for C6 and the report only: a T5 rename is made in place, so its old name has no file of its own and is never trashed. C3 classifies the skipped names. Phase `registering`.
3. **Classify** the folder's entries (C3).
4. **File fixes** (phase `fixing_files`): renames of untracked names (C4), then case merges (C5). Each new name is written by the add path; no old file is trashed yet.
5. **Note scan** on a collection snapshot, taken after the run subscribes to the change stream (C24 step 1) (phase `scanning_notes`, done and total notes): every note in ascending id order, its references resolved (C6), inline images extracted (C7), LaTeX names collected (C8). The scan records, per note, the planned substitutions.
6. **Fix op** (C10), when step 5 planned at least one substitution. Phase `committing`.
7. **Trash the old names** of C4 and C5 (C10 step 5).
8. **Report** (C11, C12).

The lease is released at every exit: the end, a cancel, an error. `JobFinished` carries the report id, `Cancelled` or the error.

Cancellation (`engine/jobs#J4`) is honoured until the fix op is submitted: during the wait, the registration (whose committed batches stay, T7), the file fixes and the note scan. A cancelled run changes no note, makes no trash of C10 step 5 and produces no report; the earlier report stays (C1). Files already written by step 4 or by C7 stay; they hold bytes that exist under another name too, and the next run finds them by content (M3 row a, M6 step 2) and ends exactly as an uncancelled run would. A file that the store's own rules replaced during those steps (`media/media-files#M4` step 4, M6 step 3) stays in the trash, as every store replacement does. From step 6 on the run is not cancellable.

Errors: an I/O error on one entry before step 6 ends the run with an error naming the file and the system error; nothing in the collection has changed. A fix op that fails (C10 step 2's refusal, a write error) is rolled back and ends the run with its error; step 7 does not run, so no old name is trashed, and files written by step 4 or C7 stay, as after a cancel. A run that ends with an error produces no report and keeps the earlier one. In step 7 an error on one old name does not end the run: the name is listed under `fix_skipped` with the reason `error` and the system error, and the run ends with its report.

Decided 2026-10-27: Check Media runs only on the user's request (U163). It never runs at open, on a timer or after a sync.

## C3. Folder entries

Every entry of the media folder falls in exactly one class, decided in this order after step 2:

| Class | Entries | In the report |
|---|---|---|
| ignored | names `media/media-files#M8` ignores (`.DS_Store`, `thumbs.db`, ...) | nowhere |
| non-UTF-8 | names that are not valid UTF-8 (possible on Linux only) | counted (`non_utf8`) |
| directory | subdirectories | `dirs` |
| other | symbolic links and every other kind that is not a regular file | `other` |
| oversize | regular files above the limit, tracked or not | `oversize` |
| empty | regular files of 0 bytes, tracked or not | `empty` |
| tracked | tracked files of 1 byte to the limit | C5, then C11 |
| renamable | every other regular file of 1 byte to the limit: its name is not normalized, or it is not in NFC and T5 left it out | C4 |

Oversize and empty files are never renamed, merged, offered for deletion or reported missing (C11). Tracked oversize and empty files are present for references.

A non-NFC file that `sync/media-tracking#T5` step 4 left out (another entry already holds its NFC name, on a volume that tells the two forms apart) is renamable: C4 merges or renames it. Its `renamed` entry, or its `invalid` entry, is the report of the skipped file that T5 step 4 asks for.

## C4. Renames (R1646)

The renamable files, in descending order of their exact names' UTF-8 bytes, one at a time:

1. Read the bytes B.
2. Add B through the add path (`media/media-files#M3`) with the desired name D = the exact old name. The add applies the transform, lowercases (row b), reuses an identical present file (row a) or writes a hash name (row c).
3. A refusal (`invalid_name`: the transform empties the name; `ignored_name`: the transformed name is ignored) leaves the file as it is and lists it under `invalid`.
4. Otherwise the stored name S goes into the renamed map: old name → S. The old file stays in the folder until step 7 of C2.

So `foo[.jpg` becomes `foo.jpg`, `Bar?.PNG` becomes `bar.png`, `con.jpg` becomes `con_.jpg`, a 130-byte name is cut to at most 119 bytes (`text/media-filenames#F1` step 6: the stem to 120 − extension − 2 bytes, then `.` and the extension), and a non-NFC file whose NFC name is already present (a volume that keeps both) is merged into it when the bytes match, or gets the add's other name when they differ.

Every rename copies the bytes: for a moment the file exists twice. Decided 2026-10-27: copy, then trash the old file after the fix op, so that a crash at any point leaves every byte reachable under a name the folder or the trash holds.

## C5. Case variants (R1648)

Case variants of tracked files exist only on a case-sensitive volume (the Linux container; the store never creates them, `media/media-files#M3`). For each case key held by two or more tracked files:

1. Group the files by content hash.
2. In each group of two or more, keep one name K: the one equal to the case key when the group has it, else the greatest by UTF-8 bytes. Every other name of the group goes into the renamed map → K, and its file is trashed at C2 step 7. Nothing is written.
3. When two or more names with different contents remain, nothing more changes: the names, with their hash and size, are listed under `case_conflicts`. All of them stay, and references to any spelling keep resolving by lookup (M5). Decided 2026-10-27 (R1648, data safety first): different content is reported, never renamed.

`Cat.jpg` and `cat.jpg` with the same bytes: `cat.jpg` stays, `Cat.jpg` → `cat.jpg`, references spelled `Cat.jpg` are rewritten (C6). With different bytes: both stay, both are listed.

After C5 the map is resolved: a value that is itself a key takes that key's final value. C4's add can reuse a present case variant that C5 then maps away (M3 row a takes the smallest by UTF-8 bytes, C5 keeps the greatest): renamable `CAT[.jpg`, `Cat.jpg` and `cAT.jpg` with the same bytes give `CAT[.jpg` → `Cat.jpg` and `Cat.jpg` → `cAT.jpg`, so `CAT[.jpg` → `cAT.jpg`. C6, C10 and the report use the resolved map.

## C6. References in notes (R1645, R989)

For each note (ascending id), each field (in order), each reference of `text/media-references#M4` (media tags, then sound tags) whose raw name is not remote (M5 there):

1. n0 = the decoded name. If n0 is an inline image (C7), n1 = the name C7 gives; otherwise n1 = n0.
2. n2 = n1 in NFC.
3. If the renamed map has the key n1 (exact), n3 = its value; otherwise, if it has the key n2 (exact), n3 = its value. The keys are the folder's exact names (C1), so a non-NFC name that C4 renamed (C3) is found by its own spelling before its NFC form: a field that spells it exactly gets that file's new name, not the name of its NFC sibling. Otherwise, decided 2026-10-27 (R1645): if one or more keys have the case key of n2, n3 is the value of the one M5 step 3's rule picks (the key equal to that case key, else the smallest by UTF-8 bytes). So `Foo[.jpg`, which names the file `foo[.jpg` on a case-insensitive volume, follows that file's rename and never falls to step 4, which could pick another file.
4. Otherwise, if n2 = n0 (steps 1 to 3 changed nothing): let T be the transform of n2 (`text/media-filenames#F1`). If T differs from n2 and lookup (`media/media-files#M5`) finds a file for T, n3 = T. This repairs fields that still name a file under the name it had before an earlier rename. Otherwise n3 = n2.
5. If n3 differs from n0, plan a substitution in the field: R is n3, with `&`, `<`, `>`, `"` and `'` written as `&amp;`, `&lt;`, `&gt;`, `&quot;` and `&#x27;` when the raw name differs from the decoded name (the raw name used entities), and n3 as is otherwise. In the match text (from its `<` to its `>`, or the whole sound tag), every occurrence of the raw name is replaced by R, left to right; then every occurrence of the old match text in the field is replaced by the new match text. Later references of the same note work on the field as already changed; a reference whose match text is no longer in the field changes nothing.
6. The note references n3.

Examples, with the renamed map `foo[.jpg` → `foo.jpg`:

| Field before | Field after | Name |
|---|---|---|
| `<img src="foo[.jpg">` | `<img src="foo.jpg">` | `foo.jpg` |
| `<img src="Foo[.jpg">` | `<img src="foo.jpg">` (by case key, step 3) | `foo.jpg` |
| `<img src="a&amp;[b].jpg">`, renamed `a&[b].jpg` → `a&b.jpg` | `<img src="a&amp;b.jpg">` | `a&b.jpg` |
| `[sound:cafe` U+0301 `.mp3]` | `[sound:café.mp3]` | `café.mp3` |
| `<img src="a:b.jpg">`, `ab.jpg` present | `<img src="ab.jpg">` | `ab.jpg` |
| `<img src="a:b.jpg">`, `ab.jpg` absent | unchanged | `a:b.jpg` (missing) |
| `<img src="https://x.org/p.png">` | unchanged | none |

## C7. Inline images (R1647)

- A decoded name is an **inline image** when it is, in full: `data:image/`, an extension `jpg`, `jpeg`, `png`, `gif`, `webp` or `avif`, `;base64,`, then a payload of one or more characters none of which is LF. The prefix, extension and `;base64,` match case-insensitively (Unicode simple case folding, so U+017F counts as `s`).
- The payload, with Unicode whitespace removed at both ends, must decode as RFC 4648 base64: the standard alphabet, `=` padding, a length that is a multiple of 4, no other character, and zero trailing bits. A payload that does not decode, and decided 2026-10-27 one that decodes to no bytes, makes the name no inline image: it stays a plain reference (and is reported missing, as in Anki).
- The image's name is `paste-` + the SHA-1 of the decoded bytes in 40 lower-case hex digits + `.` + the extension as written (`DATA:IMAGE/JPEG;BASE64,...` gives `.JPEG`). Check Media writes it with the exact-name write (`media/media-files#M6`): identical bytes already present under that name or a case variant write nothing, and the present file's name is the one used. The write happens during the note scan, before the fix op; the field then references the name (C6 step 5).
- Each reference extracted counts once in `extracted`, also when the same image appears twice.
- Images in other formats (`data:image/svg+xml;base64,...`, `data:text/plain,...`) are not extracted.

## C8. LaTeX names

Notes made in Anki can reference images Anki generated from LaTeX. Meguri never generates them (`render/card-render-pipeline`), but Check Media counts them, so Delete Unused never offers them (data safety first).

For each field of a note, after C6's substitutions:

1. The scan text: when the field contains `{{c` (exactly, case-sensitive), it is empty if the field has no LaTeX match (below); otherwise it is, for each cloze number of the field in ascending order (`render/cloze#C5`), the `cloze:` output for that number on the front, then on the back, by `render/cloze#C6` steps 1 and 2 without the C8 MathJax pass, all joined. Otherwise the scan text is the field.
2. A **LaTeX match** is, case-insensitively: `[latex]`, then the shortest run of one or more characters of any kind (line breaks included), then `[/latex]`; or the same with `[$]` and `[/$]`; or with `[$$]` and `[/$$]`. Matches are leftmost-first, the three forms tried in this order at each position, never overlapping.
3. The LaTeX text of a match: its run for `[latex]`; `$` + run + `$` for `[$]`; `\begin{displaymath}` + run + `\end{displaymath}` for `[$$]`.
4. In it, each `<br>`, `<br/>` and `<div>` (ASCII case-insensitive, exactly these spellings) becomes LF, then `text/strip-html#H4` applies.
5. The name is `latex-` + the SHA-1 of the result's UTF-8 bytes in lower-case hex + `.svg` when the note type's `latexsvg` is true (`model/schema11-json`), `.png` otherwise.

These names are references of the note (C11): never rewritten, never in NFC, and reported missing when absent, as in Anki. Meguri offers no Render LaTeX action (decided 2026-10-27; it never generates images).

`[latex]x<br>y[/latex]` gives the LaTeX text `x` LF `y`. `[$]a<b>b</b>[/$]` gives `$ab$`.

## C9. Templates and CSS

- **Underscore files.** A tracked file whose name starts with `_` is never unused (R1645): the convention for files that templates and CSS use, such as fonts and scripts. Fields can still reference it, and a missing one is reported missing like any other.
- **Literal template references.** Decided 2026-10-27 (data safety first): the references of each template's front and back text by `text/media-references#M4`, not remote, whose decoded name contains no `{{`, protect the file they name, by case key, from being unused. They never make a file missing and never tag a note. Anki does not read templates here: it reports such a file unused (C20). CSS is not read; `_` files cover it.
- **Field references in media tags.** A template side is listed under `field_templates` when it has a media tag or sound tag (`text/media-references#M2`, M3) whose raw name contains `{{`, then one or more characters, none of them LF, then `}}` (the pair on one line), or a LaTeX match (C8) whose run contains the same. `[$]{{` LF `Front}}[/$]` does not count. Such references depend on note content that Check Media cannot see (R989); the report says so. Each entry is the note type's name, the template's name and the side (`front` or `back`), ordered by note type name (`unicase`), template ordinal, front before back.

## C10. The fix op

The note writes of one run are one op:

- Catalog name `check_media_fix` (`engine/op-model#O9`), source `Maintenance`, `Local` stamping. Undo class: not destructive. Its undo effect is that of `Maintenance` (`engine/undo` M2): clear all when it changed something; a run that changes no note submits no op and leaves the history alone. It declares a long write (O8) when it plans more than 1,000 notes; its write is not cancellable.
- No safety backup (`package/backup-triggers#T6.1` lists none for it): it only rewrites references to bytes that exist, and every replaced file is in the trash (R2020).

Steps:

1. **Re-derive** (`engine/op-model#O7`). For each planned note, read the live row and apply the note's planned substitutions to its live fields, in the planned order. A substitution whose old match text is not in the live field changes nothing. References added since the snapshot are left for the next run.
2. Write each note whose fields a substitution changed; no other note is written, whatever its fields hold. Every field of such a note then loses its control characters by `text/checksum-and-sort-field` C1 step 1 (CR included, so CR LF becomes LF); NFC (C1 step 2) does not apply, whatever `normalize_note_text` says, as in Anki. `flds` is the result, `sfld` and `csum` are computed from it (C2 to C4 there), `mod` is now in seconds, `usn` -1. Tags, `guid`, `mid`, `flags` and `data` stay, and no card is generated. `col.mod` becomes now in milliseconds. A note to write whose field count differs from its note type's (at least 1) makes the fix op fail with an error naming the note, as Anki's whole run fails: nothing is written, and Check Database repairs the note (`notes/note-records#F1`). The scan (C2 step 5) ends with the same kind of error at a note whose note type does not exist (C8 needs it), as Anki's does.
3. The change set lists the notes; CloudKit and the views see a local change like any other op's.
4. After the commit, the report's note references are the planned names (step 5 of C2), not a new scan.
5. **Trash the old names** of C4 and C5 (C2 step 7), never T5's (C2 step 2): C4's first, then C5's, each in UTF-8 byte order. Each goes by its exact name: the directory entry the folder listed, never put in NFC and never resolved to a case variant (`media/media-files#M5` steps 1 and 3 do not apply). The move is that of `media/media-files#M10.1` for that one entry (`trash_exact`): a new trash entry with the exact name, its content hash, size and the time, and a pending deletion only when the journal records that exact name as a live syncable file (C5's old names; C4's old names are untracked and get none). Every old name is hashed first, away from the writer (the journal holds no facts for C4's untracked old names). Then the names are checked again and moved in batches of 25, each batch in one writer step (C24), each name's check in the same journal-serialized step as its move. A name is skipped, and listed under `fix_skipped` with the reason, when:
   - no directory entry has exactly that name: `gone`;
   - the file under that exact name does not have the content hash that C4 read or that C5's group has, or its size or modification time changed since it was hashed: `changed`;
   - the new name (the resolved value, C5), taken exactly, no longer holds a file with that content hash: `target_changed` (its journal facts are trusted while its size and modification time still match, as in M6);
   - a reference of a live note (C24) or a literal template reference (C9) names the old name as the volume compares names: after NFC, exactly on a case-sensitive volume, by case key on a case-insensitive one: `referenced`. On a case-insensitive volume a reference spelled `Foo[.jpg` reaches the file `foo[.jpg` by M5 step 2, so it protects it; a template that names `logo[1].png` literally keeps that file, since templates are never rewritten. C5 runs only on case-sensitive volumes, so the kept name never matches its old names. A non-NFC old name never matches: a reference spelled that way resolves to the NFC sibling by M5 step 1, never to the old file.

   A skipped old file stays in the folder; the next run handles it again. Decided 2026-10-27 (data safety first): this exact-name form of M10.1 is a store call that U31 provides next to `trash(names)`. Resolving an old name by lookup could pick the kept file itself (a non-NFC name resolves to its NFC sibling, a `Cat.jpg` removed from outside to the kept `cat.jpg`), pass the bytes check and trash referenced bytes with a CloudKit tombstone; the exact-name move cannot.

Decided 2026-10-27: the substitution order, the escaping and the replacement of every occurrence of the match text follow Anki, so a field changes byte for byte as Anki would change it.

## C11. Used, unused and missing

After the fixes, from the journal and the planned references:

- **Present names**: the tracked files, empty and oversize ones included, as the journal holds them after C4 and C5 (the old names of the renamed map excluded).
- A reference is **satisfied** when its name's case key equals the case key of a present name (R1645: case-insensitive on every volume).
- **Unused**: present names of 1 byte to the limit, not starting with `_`, not in the renamed map's keys, whose case key no note reference and no literal template reference (C9) has.
- **Missing**: every distinct reference name (as C6 step 6 or C8 gives it, exact spelling) that is not satisfied, except names that are pending downloads:
  - names the Anki media-sync state holds `awaiting` (`sync/media-tracking#T9`; R1584);
  - names whose case key is that of a live media-name entity of the active CloudKit binding whose local file does not hold the entity's content (the download candidates of `sync/cloudkit-records#K9.3`; tracking keys these entities by case key, K9.4). Decided 2026-10-27: U114 provides this as a read-only query on its tracking store (K10.3 `entities`), which returns those case keys; U130 depends on it for this list only. Without iCloud sync the list is empty.

  These are listed under `pending_download` instead, and their notes are not tagged (C20).
- **Missing notes**: the notes with at least one missing reference.

Check Media never records a deletion for a missing file and never removes a journal record for one. Registration in step 2 drops the facts of files that vanished from outside with the cause `missing` (`media/media-journal#J6`), which is never a deletion for CloudKit.

## C12. The report

| Field | Content | Order |
|---|---|---|
| `unused` | names, each with `sha1` and `size` | UTF-8 bytes |
| `missing` | names | UTF-8 bytes |
| `missing_notes` | note ids | ascending |
| `pending_download` | names left out of `missing` (C11) | UTF-8 bytes |
| `renamed` | old name → new name (C4, C5, and the NFC renames that the registration of C2 step 2 made, `sync/media-tracking#T5`) | old name, UTF-8 bytes |
| `case_conflicts` | groups of C5 step 3: case key and the names with `sha1`, `size` | case key |
| `invalid` | renamable files the add refused (C4 step 3), with the refusal | UTF-8 bytes |
| `dirs`, `other`, `oversize`, `empty` | names (C3) | UTF-8 bytes |
| `non_utf8` | count | |
| `extracted` | count of inline images extracted (C7) | |
| `field_templates` | C9 entries | C9 |
| `fix_skipped` | old names C10 step 5 skipped, with the reason (`gone`, `changed`, `target_changed`, `referenced`, or `error` with the system error, C2) | UTF-8 bytes |
| `notes_changed` | count of notes the fix op wrote | |
| `trash` | entry count and total bytes (`media/media-files#M10.3` `summary`) | |
| `hashed` | files hashed by the registration (`sync/media-tracking#T6`) | |

- The engine keeps the report until the next run that finishes or the collection closes (C1), under a report id. U133 pages each list (`page(report, list, offset, limit)`); counts come with the summary. The UI offers Delete Unused only when `unused` is not empty, Tag Missing only when `missing_notes` is not empty, and Restore Deleted and Empty Trash only when the trash is not empty.
- The tag Tag Missing adds is `missing-media` (exact). All other wording is Meguri's own (U163).

## C13. Tag Missing (R1649)

- Input: the report id. Output: the number of notes written.
- A report that a finished run replaced, or that closing the collection dropped (C1), is refused with `StaleReport`; nothing changes.
- Adds the tag `missing-media` to the report's `missing_notes` with `tags/tags#T10` Add: one undoable op, step name "Update Tag" (`conformance/undo-ops` V4 `update_tag`), as Anki's tag add is. Notes deleted since the report are skipped; a note that already has the tag is not written. Undo removes the tag from exactly the notes it was added to.
- It needs no lease and no confirmation.

## C14. Delete Unused (R1649, R2018, R2008)

- Input: the report id. It acts on the report's whole `unused` list. Output: `trashed` (names), `skipped` (name and reason), `backup` (the safety backup's name, or none).
- The UI confirms first, with the count. When the Anki media-sync state has a baseline owner and either a media USN of 0 or an `awaiting` entry (media not fully in sync with an Anki server), the confirmation also says that deleted files can come back from that server (R1585, `sync/media-sync`). The preflight call that gives the count returns this flag.

Steps:

1. Take the lease. A report that a finished run replaced, or that closing the collection dropped (C1), is refused with `StaleReport`; nothing changes.
2. Preflight (`media/media-journal#J5`): subscribe to the change stream and read the base references (C24 step 1), then check each name as step 4 does. A name that fails is skipped now with its reason. This check only spares the backup when nothing remains; step 4 checks every name again.
3. When at least one name remains, take the safety backup (`package/backup-triggers#T6.1`, "Delete Unused media"). `SafetyBackupFailed` ends the action; nothing is trashed. A collection backup holds no media: it protects the collection; the files are protected by the trash.
4. Trash the remaining names in batches of 25, in report order, each batch in one writer step (C24), so a reference committed during the backup or between batches is seen. In it, each name is checked in the same journal-serialized step as its move: skipped as `changed` when the journal's facts for it differ from the report's `sha1` and `size`, as `gone` when it has none; skipped as `referenced` when a live note's reference (C24) or a literal template reference (C9) has its case key; otherwise moved by `media/media-files#M10.1`. Progress: names done and total. Cancel is honoured between batches; what was trashed stays trashed and restorable.
5. Each trashed name is a pending Anki media-sync deletion when the journal recorded it live (M10.1) and a feed item with the cause `removed` (`media/media-journal#J6`): CloudKit sends a media-name tombstone (`sync/cloudkit-records#K9.4`), unless a present name of the same case key remains (C18), and other devices move their copy to their own trash.

Delete Unused changes no collection row and leaves the undo history as it is. It is destructive and not undoable as a file action, so it is confirmed first and preceded by a backup (R2020).

## C15. The media reference index

The derived-store component of U130 (`storage/derived-store#D4`), name `media_refs`, version 1:

- One row per note reference: the case key of its name, the name, and the note id. The names are those of C6 steps 1 and 2 (decoded, NFC; an inline image counts as its data string) and the LaTeX names of C8. Remote references are left out.
- Capture: the notes of each commit's change set (added, changed, removed). A change set that lists a note type whose `latexsvg` changed (before-image against after-image) answers "rebuild": the LaTeX names of all its notes change (C8 step 5). Build and catch-up: from a snapshot, every note.
- Delete Unused reads it for its base references (C24 step 1) only when it is current for that snapshot (`storage/derived-store#D8`); otherwise it scans the notes. Check Media's own scan (C2 step 5) always reads the notes, since it rewrites them.

## C16. Restore Deleted (R1649)

- Input: none. Output: for each restored entry, its trash name and the stored name, or `dropped` when identical bytes were already present.
- Takes the lease, then restores every trash entry by `media/media-files#M10.2`, newest first: never overwriting a file, an entry whose name is taken added by the add rules. Progress: entries done and total; cancel is honoured between entries. No confirmation, no backup, no collection change.
- References are not rewritten: a field naming `b.jpg` keeps naming the file now under `b.jpg`, even when the restored bytes went to a hash name.
- Each restored name is a feed item with its file facts: CloudKit sees the file again as a local change.

## C17. Empty Trash (R1649, R1650, R2018)

- Preflight: the entry count and total bytes of `media/media-files#M10.3` `summary()`, plus the identities of the entries it counted (`media/media-journal#J3`). The UI shows the count and the size and asks for confirmation.
- Input: the preflight's entry identities. Output: the count and bytes deleted.
- Store calls: this action uses two identity forms of M10.3 that U31 provides for it: `summary_with_ids()`, which returns M10.3's count and size together with the identities counted, and `empty_entries(identities)`, which deletes only the listed entries and skips an identity the trash no longer holds. M10.3's `empty()`, which deletes every entry, is never called by Check Media.
- Takes the lease, then deletes those entries permanently, one at a time. Decided 2026-10-27 (data safety first): an entry trashed after the preflight (an incoming CloudKit tombstone, a media sync deletion) is kept; the user confirmed only what was shown. Cancel is honoured between entries; deleted entries are gone.
- The media folder, the journal and the feed do not change. No collection change and no backup: a collection backup holds no media, and the trash is the user's last copy, which is why only this explicit action removes it.
- Nothing else empties the trash, ever (R1650).

## C18. What the journal and CloudKit see

| Action | Collection | Anki media-sync state (`sync/media-tracking`) | Feed (`media/media-journal#J6`) | CloudKit |
|---|---|---|---|---|
| Registration (C2 step 2) | none | T4.3 | facts of new and changed files; vanished files absent, cause `missing` | uploads; a vanished file is a missing local file, never a tombstone |
| T5 NFC rename | none | the NFC name registered | the NFC name present | a local change of that name |
| C4 rename, C7 extraction | none | the new name pending | the new name present | upload of the new name |
| C5 merge, C4 old name trashed (C10 step 5) | none | a tracked old name: pending deletion of that exact name | a tracked old name: absent, cause `removed`; an untracked one has no record | a C5 old name: no tombstone, the entity stays live with K's spelling (below); a tracked old name whose case key no present name holds: tombstone |
| Fix op | notes rewritten | none | none | note changes |
| Tag Missing | tags | none | none | note changes |
| Delete Unused | none (backup taken) | pending deletions | absent, cause `removed` | tombstones, except for a case key a present name still holds (below) |
| Restore Deleted | none | pending live entries | present again | a local change; upload if needed |
| Empty Trash | none | none | none | none |

A file that disappears from the folder outside Meguri is never a deletion, whatever Check Media reports about it: it is `missing` in the feed and in the report.

Decided 2026-10-27 (data safety first): tracking keys media-name entities by case key, and the name that is present gives the entity's spelling (`sync/cloudkit-records#K9.4`). So a `removed` item for one spelling makes no tombstone while another present name holds the same case key: the entity stays live with that name's spelling and content. A C5 old name always shares its case key with the kept name K, which stays present with the same bytes, so a merge never sends a tombstone; another device that holds K keeps it. The same holds for Delete Unused (C14 step 5) when it trashes one of two case variants. K9.4 does not yet say this in so many words; that spec takes the sentence with U114.

## C19. Budgets (R1651)

On the reference Mac, with U11's synthetic media (30,000 files, 1.5 GB) and collection, nothing changed since the last registration and a warm file-system cache:

- a whole run, from the lease to the report, takes under 5 s and hashes no file (`hashed` 0);
- after 10 files are rewritten from outside with new sizes, a run hashes exactly 10 files;
- a page of 100 report rows comes back in under 50 ms.

The bench is `core/crates/meguri-engine/benches/check_media.rs`; the perf report records it.

## C20. Differences from Anki

Anki 26.09.3 on Linux (the oracle that records goldens) matches references to files exactly, by the folder's own case sensitivity. Proposed deviation ids are added with the scenarios (C22).

| Situation | Anki | Meguri | Deviation |
|---|---|---|---|
| A rename or case merge | deletes the old file permanently | moves it to the trash after the fix op | `check-media-rename-trashes-old` |
| Case variants with different content (case-sensitive volume) | keeps the greatest name by UTF-8 bytes and adds each other one under its lowercase form: under that name when no file has it exactly (`CAT.jpg` next to `Cat.jpg` becomes `cat.jpg`), under a hash name when one does; references are rewritten to those names | keeps them all, lists them in `case_conflicts` | `check-media-case-variant-kept` (needs case-variant setup, C21) |
| Identical case variants without a member equal to the case key | keeps the greatest name and writes a lowercase copy for the others | keeps the greatest name only | `check-media-case-variant-merge-name` (same) |
| A reference whose case differs from the file's name (case-sensitive volume) | the reference is missing and the file unused | the reference is satisfied | `check-media-case-insensitive-references` |
| A reference whose case differs from a renamed file's old name (`Foo[.jpg` for the file `foo[.jpg`, case-sensitive volume) | not rewritten, missing | rewritten to the file's new name (C6 step 3) | `check-media-case-insensitive-references` (same) |
| A referenced name not yet downloaded (Anki-sync `awaiting`, or a CloudKit blob not installed) | missing, its note tagged | listed under `pending_download`, not tagged (C11) | `check-media-pending-not-missing` |
| An inline image just extracted | reported missing in the same run | present | `check-media-extracted-not-missing` |
| A file named only by a literal template reference | unused; when renamed, its old name is deleted | not unused; when renamed, its old name stays (`fix_skipped` `referenced`, C10 step 5) | `check-media-template-references` |
| A referenced empty or oversize file | missing | present (listed under `empty` or `oversize`) | `check-media-empty-oversize-present` |
| `thumbs.db` and other ignored names besides `.DS_Store` | unused | not listed | `check-media-ignored-names` |
| A run that changes nothing | moves `col.mod` and empties the undo history | changes nothing | `check-media-no-change-no-mod` |
| A renamable file whose name the transform empties | the whole check fails with an I/O error | listed under `invalid` | none (Anki records no outcome) |
| A base64 payload that decodes to no bytes | writes an empty `paste-` file | not extracted | none |
| A non-NFC name on Apple | not renamed, registered under its NFC form | renamed (T5) | none: goldens are Linux's, where Anki renames |
| A non-NFC name with upper-case letters (Linux) | adds it under its NFC form lowercased and deletes the old file (NFD `Café.jpg` becomes `café.jpg`) | T5 renames it in place to its NFC form, case kept (`Café.jpg`) | `check-media-nfd-keeps-case` (scenario `rename/nfd-upper`) |
| A field spelling a non-NFC name exactly, next to its NFC sibling with other bytes (a volume that keeps both) | rewritten to the NFC sibling's name | rewritten to the renamed file's new name (C6 step 3) | none: Meguri's tests |
| A symbolic link in the folder | treated as the file it points to: can be unused or renamed | class `other`, never renamed, trashed or offered | none: Meguri's tests |
| A renamable name whose transform is an ignored name (`thumbs.db?` on Linux) | stored under the transformed name | listed under `invalid` (`ignored_name`), file unchanged | none: Meguri's tests |
| Delete Unused | no backup; trashes the list as reported | backup, each name checked again (C14) | none: Meguri's tests |
| Empty Trash | no confirmation; deletes every entry | confirmation with size; deletes only the confirmed entries | none: UI and Meguri's tests |
| Render LaTeX | offered when a `latex-` name is missing | not offered | none |
| Cancel | rolls back note changes; file changes stay | the same, and no report (C2) | none |

Restore conflicts follow `media/media-files#M14` (`media-restore-case-variant`, `media-trash-keeps-versions`).

## C21. Scenario vocabulary (`conformance/media-ops#F14`)

The ops `check_media`, `delete_unused_media` and `tag_missing_media`, their outputs, the scenario rule for the steps between `check_media` and `delete_unused_media`, and the case-variant setup (the tag `media-case-variants`) are defined only in `conformance/media-ops#F14` (F14.1 to F14.4); this file gives no shapes of its own. The existing setup `media_files`, ops `media_outside`, `trash_media`, `restore_media_trash`, `empty_media_trash` and observations `media_dir`, `media_trash`, `media_pending` are reused.

- Note fields are observed with `db_dump` (`conformance/observations` O8) or the notes observation.
- Goldens come from the Linux oracle only (`conformance/media-ops#F13`).

## C22. Scenarios (for the oracle maintainer)

Under `conformance/scenarios/maintenance/check-media/`, each with its own note type. "Seed" means `media_files`, then `media_pending` (so Anki has registered the files, `conformance/media-ops#F6`), then the step.

| Scenario | Setup and steps | Pins |
|---|---|---|
| `report/basic` | files `a.jpg` (referenced by `<img>`), `b.jpg` (unreferenced), `snd.mp3` (`[sound:]`), `_font.ttf` (unreferenced), directory `x`, a 125,829,120-byte `big.bin`, `.DS_Store`, an empty `e.txt`; fields referencing `c.png` (absent) and `https://example.org/r.png`; `check_media`, `media_dir` | C3, C11: `unused` [b.jpg], `missing` [c.png], `dirs` [x], `oversize` [big.bin] |
| `rename/invalid-chars` | files `foo[.jpg`, `Bar?.PNG`, `con.jpg`, referenced as written by `<img>`; `check_media`, `media_dir`, `db_dump` notes | C4, C6: renames and field rewrites; trash differs (`check-media-rename-trashes-old`) |
| `rename/long-name` | a referenced 130-byte name | C4: cut to at most 119 bytes by `text/media-filenames#F1` step 6, neither unused nor missing |
| `rename/nfd` | file `cafe` U+0301 `.jpg` (NFD), one field referencing NFC `café.jpg`, one referencing the NFD spelling; `check_media`, `media_dir`, `media_trash`, `db_dump` notes | C2 step 2, C6: one rename, NFD reference rewritten to NFC. T5 renames in place, so the trash stays empty: this replaces the plan's U130 test expectation of "the old file in the trash", since no second copy of the bytes ever exists |
| `rename/nfd-upper` | as `rename/nfd` with `Cafe` U+0301 `.jpg` and both spellings of `Café.jpg` | T5 keeps the case: `Café.jpg`; Anki writes `café.jpg` (`check-media-nfd-keeps-case`) |
| `rename/entities` | file `x&[y].jpg` referenced as `<img src="x&amp;[y].jpg">` | C6 step 5: `x&amp;y.jpg` written back |
| `refs/transform-repair` | file `ab.jpg`, a field `<img src="a:b.jpg">` | C6 step 4 |
| `refs/html-forms` | `<source>`, `<audio>`, `<object data=>`, quoted and unquoted `src`, a `>` inside another attribute, `[sound:a &amp; b.mp3]` with file `a & b.mp3` | C6 with `text/media-references` M2 to M4 |
| `inline/extract` | fields with a valid PNG payload (own bytes), the same payload twice, `DATA:IMAGE/JPEG;BASE64,` (upper case), an invalid payload, an `svg+xml` payload | C7: `paste-` names and `extracted`; Anki's missing list differs (`check-media-extracted-not-missing`) |
| `latex/names` | a `[latex]`, a `[$]` and a `[$$]` field, one with `<br>`; files for some of the `latex-` names; a note type with `latexsvg` true; a cloze field with LaTeX inside a cloze | C8 |
| `templates/field-refs` | a template `<img src="{{Front}}">` on the back, `[sound:{{Audio}}]` on the front; a template naming `_logo.png` literally | C9 `field_templates`, `_` rule |
| `templates/literal-ref` | a template `<img src="logo.png">`, file `logo.png` | C9 deviation `check-media-template-references` |
| `refs/case-insensitive` | a field `[sound:ABC.mp3]`, file `abc.mp3` | C11 deviation `check-media-case-insensitive-references` |
| `refs/empty-oversize` | fields referencing an empty file and an oversize file | C3 deviation `check-media-empty-oversize-present` |
| `actions/tag-missing` | `report/basic`, then `tag_missing_media`, `undo_status`, `undo`, `db_dump` notes | C13: tag `missing-media`, step `update_tag`, undo removes it |
| `actions/delete-unused` | seed; `check_media`, `delete_unused_media`, `media_dir`, `media_trash`, `media_pending` | C14: `b.jpg` in the trash, a pending deletion |
| `actions/restore-conflict` | `actions/delete-unused`, then `media_outside` a different `b.jpg`, `restore_media_trash`, `media_dir` | C16: the present `b.jpg` keeps its bytes, the trashed one becomes `b-<sha1>.jpg` |
| `actions/empty-trash` | `actions/delete-unused`, `check_media` (trash count), `empty_media_trash`, `media_trash`, `media_dir` | C17: trash empty, folder unchanged |
| `rerun/no-change` | `rename/invalid-chars`, then `check_media` again | C4 to C7 find nothing; avoid observing `col.mod` (`check-media-no-change-no-mod`) |
| `case/same-content`, `case/other-content` | case-variant setup (C21): `Cat.jpg` and `cat.jpg`, same and different bytes, references to both spellings | C5 (deviations `check-media-case-variant-kept`, `check-media-case-variant-merge-name`) |

## C23. Meguri-only tests

In `core/crates/meguri-engine/tests/check_media.rs`:

- the lease: a media sync started during a run waits, then runs; no name is both renamed and downloaded;
- cancellation at each phase before the fix op: no note changes, no trash of C10 step 5, no report, the earlier report kept and usable, the lease released; a store replacement made by M6 step 3 during the scan stays in the trash; a rerun ends as an uncancelled run;
- C10 step 2: a rewritten note's other field holding CR LF is stored with LF and NFC is not applied; a note not rewritten keeps its control characters; a planned note with a wrong field count fails the fix op, nothing is written and no old name is trashed;
- C2 step 2: an NFC rename by T5 is in `renamed` and is never trashed or listed under `fix_skipped` (APFS and Linux);
- C5's resolved map: the `CAT[.jpg`, `Cat.jpg`, `cAT.jpg` chain ends with every field naming `cAT.jpg` and nothing under `fix_skipped`;
- C6 step 3 by case key: `Foo[.jpg` next to `foo[.jpg` (content B) and `foo.jpg` (content A) is rewritten to B's new name, never to `foo.jpg`;
- C24: a reference committed during Delete Unused's backup, between two batches, or between the fix op and C10 step 5 (a note edit and a `CloudIncoming` apply) makes the name `referenced`; a literal template reference does too;
- a crash after C4's copies, after the fix op and after step 7 (the U250 fault hooks): every byte reachable, the rerun converges;
- C10 step 1: a note edited between the scan and the op keeps the edit and gets the substitutions that still apply; a CloudKit apply that adds a reference to an old name during the run makes C10 step 5 skip it;
- C10 step 5's exact names (Linux): a non-NFC file merged into its NFC sibling trashes only the non-NFC entry, and the sibling keeps its file, facts and feed state; a merged `Cat.jpg` removed from outside before step 7 gives `gone` and leaves the kept `cat.jpg` in place; each `fix_skipped` reason;
- C6 step 3: a field spelling a T5-skipped non-NFC name exactly is rewritten to that file's new name;
- C14: a name referenced after the report is skipped as `referenced`; a name rewritten from outside as `changed`; backup failure trashes nothing; cancel between batches;
- C17: an entry trashed after the preflight survives Empty Trash;
- C11: `awaiting` names and CloudKit pending blobs are listed under `pending_download` and never tagged;
- C3: symbolic links, non-UTF-8 names (Linux), `invalid` refusals;
- C15: the index matches a full scan after random edits, and Delete Unused falls back to a scan while it is not current;
- R2018's enumeration: every path that removes a media file (Check Media fixes, Delete Unused, server deletions in media sync, incoming CloudKit tombstones and conflicts, AnkiConnect `deleteMediaFile`, the store's replacements, staging recovery) leaves the bytes in the trash;
- C18's feed causes: Delete Unused and a merge give `removed`, a file removed outside gives `missing`; a merge makes no CloudKit tombstone and the entity stays live with K's spelling;
- C15: a `latexsvg` change makes the index rebuild;
- C19's bench.

## C24. Checking names again at the move (`media/media-journal#J5`)

Decided 2026-10-27 (data safety first): Delete Unused (C14 step 4) and the fix trash (C10 step 5) check each name in the same step as its move, against every commit made before that step. A reference that a note edit or a `CloudIncoming` apply commits before the move is always seen; a journal-serialized check alone orders the check against journal writes, not against collection commits.

1. **Base.** The action subscribes to the change stream (`engine/change-stream` S3), then takes one collection snapshot P and holds, per note, the references at P: for the fix trash, P is the note scan's snapshot (C2 step 5) and the references are the planned ones (C10 step 4); for Delete Unused, they come from `media_refs` (C15) when it is current for P (`storage/derived-store#D8`), otherwise from a scan of every note (C6 steps 1 and 2 without the renamed map, and C8). The literal template references (C9) are read at P too.
2. **Writer step.** Each batch of 25 names runs on the collection writer as one op that writes no collection row: a plain op of source `Maintenance` with no catalog entry, as the barriers of `engine/op-model#O1.1` are. It changes nothing, so it leaves the undo history alone (`engine/undo` M2) and publishes no event. No commit lands while it runs.
3. **Catch-up.** First in that step, every change set waiting in the subscription is taken. Every op committed before the step has published its change set by then, since the writer runs the observers and publishes before it starts its next op (`engine/change-stream` S2, S3); U130 tests this. Each note listed as added or changed is read through the writer's connection, and its references replace its base ones; a removed note drops its own. A note type listed there has its templates read again (C9); when its `latexsvg` changed, every note of that type is read again (C8 step 5).
4. **Per name.** Then each name is checked (C10 step 5, C14 step 4) and moved in one journal-serialized step. The order is always the writer, then the journal; a journal step never waits for the collection writer (U31's store calls and U114's installs keep it so), so the two cannot deadlock.
5. **Bound.** A step targets the 200 ms of an op without a declared write (`engine/op-model#O6`): no file is hashed in it (C10 step 5 hashes first; in the step it compares size and modification time). A note type whose `latexsvg` changed can make one step longer; that is rare.
