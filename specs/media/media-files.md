# Media files: adding, names, lookup, registration and trash

Spec: media/media-files
Requirements: R978, R980, R981, R982, R983, R984, R985, R986, R1568, R1569, R1570, R1571, R1587, R2018; core side of R1582 and R1855; supports R1572, R1650, R1702, R2057
Anki: 26.09.3
Scenarios: conformance/scenarios/media/
Status: Approved 2026-10-27 (R2066) for the CloudKit parts of M6, M7.1, M10.1, M11, M12, M15 and M16, with `sync/policy`, `sync/cloudkit-records` and `sync/cloudkit-recovery`. Every other section is in force and unchanged.

What the media store does with files: the name a new file gets (dedupe, lowercasing, hash names), how a name in a field finds its file, which files in the folder count as media and are tracked for media sync, the trash, and writes under an exact name. The name transform every new name goes through is `text/media-filenames#F1`. The change journal, staging inside an op, crash recovery, the media lock and change events are Meguri's own design, in `media/media-journal` (written by U31's implementer); this file states only the rules they must keep. The scenario vocabulary is `conformance/media-ops`.

Two sync paths use this store. Routine sync is CloudKit (KTD25): its media state lives in the CloudKit tracking store (`sync/cloudkit-records#K9`, K10), fed by the journal's file-fact feed (`media/media-journal#J6`). Anki media sync (U111) is a manual transfer only (KTD26, R1572): its state is the journal's pending flag, sync state and media USN (`sync/media-tracking`). In this file, "media sync" and "pending" mean the Anki media-sync state unless a sentence names CloudKit (M16).

## M1. Folder layout

- Media lives in one flat folder (R978): `collection.media/`, next to the collection file. Its siblings on the same volume: `media.trash/` (M10) and `media.staging/` (files written inside an op, M13), plus the journal database (`media/media-journal`). Moving a file between them is a rename, never a copy.
- `media.trash` is also the name of Anki's trash folder next to its media folder. Meguri never shares a folder with Anki and never reads or writes Anki's media database; its journal replaces it, so the journal's layout is free.
- A subdirectory inside the media folder is never media: nothing in Meguri creates one, registration skips it (M7) and Check Media reports it (U130, `maintenance/check-media#C3`).
- Fields hold raw, unencoded names (R978). Percent-encoding for display and the reference scan are `text/media-references`.

## M2. Terms

- **Transform**: the name transform of `text/media-filenames#F1`. A name is **normalized** when the transform leaves it unchanged.
- **Case key** of a name: its Unicode default full lowercase mapping, Final_Sigma rule included, exactly what Rust's `str::to_lowercase` returns. No case folding beyond that and no normalization.

  | Name | Case key |
  |---|---|
  | `Photo.JPG` | `photo.jpg` |
  | `Straße.png`, `ß.png` | `straße.png`, `ß.png` (`ß` has no lowercase change) |
  | `ẞ.png` (U+1E9E) | `ß.png` |
  | `ǅ.png`, `Ǆ.png` | `ǆ.png` |
  | U+212A KELVIN SIGN + `.png` | `k.png` |
  | `İx.png` (U+0130) | `i` U+0307 `x.png` (one character becomes two, 2 bytes become 3) |
  | `ABΣ.png` | `abσ.png`: the `.` is case-ignorable and `p` is cased, so this sigma is not final |
  | `AΣ` | `aς` (final sigma U+03C2) |
  | U+10400 `.png`, U+13A0 `.png` | U+10428 `.png`, U+AB70 `.png` |

- **Case variants**: two names with the same case key. Every name is its own case variant.
- **Lowercase candidate** L of a normalized name N: the transform applied to the case key of N. Lowercasing can make a name longer than 120 bytes, and the transform then cuts it again: `İ` × 60 (120 bytes) has the case key `i` U+0307 × 60 (180 bytes) and the lowercase candidate `i` U+0307 × 39 + `i` (118 bytes); `İ` × 39 + `.png` has the candidate `i` U+0307 × 38 + `i.png`.
- **Content hash**: SHA-1 of the bytes, written as 40 lower-case hex digits. The content hash of a file in the folder is the journal's when the file's size and modification time match the journal's record; otherwise the file is hashed, and the journal takes the new values as a registration would (M7.2).
- **Tracked file** and **syncable file**: M7.1.
- **Present files**: the tracked files of the media folder as they are now, outside changes included (M7.3), plus the names that staged files reserve (M13).

## M3. Adding a file (R980–R983, R1568–R1571)

The path for every new file whose name Meguri chooses: the editor's attach and paste, `/receive` (staged, M13), imports, restoring a trash entry whose name is taken (M10.2), Check Media's renames (U130, `maintenance/check-media#C4`). Input: a desired name D and the bytes B, with content hash H. Adding a file from disk uses the file's last path component as D.

1. N = transform(D).
2. Refusals; nothing changes:
   - N is empty: `invalid_name` (all characters were removed, such as `???`);
   - N is ignored (M8): `ignored_name`.
3. Let L be the lowercase candidate of N. The first row that applies gives the stored name:

   | Row | Condition | Stored name | Folder |
   |---|---|---|---|
   | a | a present case variant of N or of L has content hash H | that file's name; when several qualify: N itself, then L itself, then the smallest by UTF-8 bytes | unchanged |
   | b | no present file is a case variant of L | L | B written as L |
   | c | otherwise: a case variant of L is present with other content | the hash name of L (M4) | B written under it (M4 step 4) |

4. The stored name is what goes into the field (R983): callers insert exactly that name, never D or N.
5. Journal (M11): rows b and c record the new file as live, and pending for the next media sync when its size is 1 byte to 100 MiB, whatever its name (M4 explains the hash names that registration later drops). Row a writes nothing and leaves the journal as it is: an outside file it reuses was already recorded by the change check that precedes every add (M7.3).

Row a is the same-content dedupe, case-insensitive (R980, R984, R1570): `Photo.JPG` finds `photo.jpg` with the same bytes and stores nothing. Row b lowercases new names (R981) and leaves existing mixed-case files alone. Row c never overwrites (R982, R1570).

When every present file's name equals its own case key (a folder written only by this path, the normal case), these rules choose exactly the names Anki chooses on a case-sensitive volume, the Linux oracle's. M14 lists where they differ from it for folders holding mixed-case names, and where Anki on a case-insensitive volume differs.

Examples (A and B are two different contents, `<A>` and `<B>` their content hashes):

| Present before | Add | Stored | Written |
|---|---|---|---|
| `photo.jpg` = A | `Photo.JPG`, A | `photo.jpg` | nothing |
| `photo.jpg` = A | `photo.jpg`, A | `photo.jpg` | nothing |
| `photo.jpg` = A | `Photo.JPG`, B | `photo-<B>.jpg` | B |
| none | `Diagram.PNG`, A | `diagram.png` | A |
| none | `CON.JPG`, A | `con_.jpg` (transform, then lowercase) | A |
| `con_.jpg` = A | `CON.JPG`, B | `con_-<B>.jpg` | B |
| `MixedCase.png` = A | `MixedCase.png`, A | `MixedCase.png` | nothing |
| `MixedCase.png` = A | `mixedcase.png` or `MIXEDCASE.PNG`, A | `MixedCase.png` | nothing |
| `MixedCase.png` = A | `MixedCase.png`, B | `mixedcase-<B>.png` | B |
| `k.png` = A | U+212A `.png`, B | `k-<B>.png` | B |
| none | `İ` × 60, A | `i` U+0307 × 39 + `i` | A |
| `readme` = A | `README`, B | `readme-<B>.` | B |

## M4. Hash names (R982)

The hash name of a lowercase candidate L for content hash H:

1. Split L at its last `.`: the extension is what follows it, the stem what precedes it. With no `.`, the stem is all of L and the extension is empty.
2. Cut the extension to at most 10 bytes, then the stem to at most 77 − (extension bytes) bytes. Each cut keeps the longest prefix that ends on a character boundary within the limit (a code point boundary, so a cut may separate a base letter from its combining mark).
3. The hash name is stem + `-` + H + `.` + extension. The `.` is written even when the extension is empty, so a name without extension ends with a dot (`readme` → `readme-<H>.`). Nothing else applies afterwards: no transform, no `_` after a trailing space or dot, no lowercasing. The result is at most 119 bytes.
4. Writing B under the hash name: the store looks for a file with exactly that name in the folder (checking that one name on the file system, since registration may not track it, see below) and for present case variants of it. If one has content hash H, its name is the stored name and nothing is written (Anki rewrites the same bytes; Meguri never rewrites unchanged content, R1571). If one has other content, which happens only when something other than this path put it there, it goes to the trash (M10.1) and B is written under the hash name.

A hash name that ends with a space or `.` (no extension, or an extension cut after a space) is not normalized, so registration never tracks it (M7.1). The add records the file like any other, so it is pending at first; the next registration finds a recorded name it does not track and turns the entry into a pending deletion (M7.2). Media sync never uploads such a file, while lookup still finds it by its exact name (M5), so the card shows it. Anki behaves the same way, and the goldens record those deletions.

| L | Hash name |
|---|---|
| `photo.jpg` | `photo-<H>.jpg` |
| `readme` | `readme-<H>.` |
| `aς` | `aς-<H>.` |
| `.png` | `-<H>.png` |
| `._` | `-<H>._` |
| `a.b.c` | `a.b-<H>.c` |
| `q.abcdefghijklmnop` | `q-<H>.abcdefghij` |
| `x.abcdefghi jkl` | `x-<H>.abcdefghi ` (ends with a space) |
| `x.éééééé` (12-byte extension) | `x-<H>.ééééé` |
| `x` × 115 + `.jpg` | `x` × 74 + `-<H>.jpg` |
| `z` × 116 + `.jpg` | `z` × 74 + `-<H>.jpg` |
| `日` × 38 + `.png` | `日` × 24 + `-<H>.png` (72 bytes; a 25th would pass 74) |
| `a` × 73 + ` b.png` | `a` × 73 + ` -<H>.png` |
| `i` U+0307 × 38 + `i.png` | `i` U+0307 × 24 + `i-<H>.png` (the cut falls between `i` and U+0307) |

## M5. Lookup (R984)

Resolving a name to the file that holds it, for rendering (U67), Check Media (U130, `maintenance/check-media#C6`, C11), the trash (M10), the integrations (U143) and media sync (U111):

1. Take the name in NFC.
2. A regular file with exactly that name is the result: a tracked file (M7.1) from the journal, or any other regular file found by checking that one name on the file system (an untracked hash name, M4; an empty or oversized file).
3. Otherwise the tracked case variants of the name: none → not found; one → that file; several (possible only on a case-sensitive volume) → the one whose name equals the name's case key, else the smallest by UTF-8 bytes.

`mixedcase.png`, `MIXEDCASE.PNG` and `MixedCase.png` all resolve to a tracked `MixedCase.png`, on every platform. Lookup reads the journal and checks at most the one exact name on the file system; it never lists or hashes the folder (R1855).

## M6. Exact-name writes (R1587)

The path for bytes whose name is fixed by someone else: bundled static files such as an integration's `_`-prefixed fonts (`ensure_static_media`, R1587, AE66), files ingested from an external folder under their own names (U143), Check Media's inline-image extraction under its `paste-<sha1>` names (U130, `maintenance/check-media#C7`), CloudKit blob installs (`sync/cloudkit-records#K9.3`, U114), and, where U111's `specs/sync/media-sync.md` says so, media sync downloads of normalized names. No lowercasing and no hash name: the name stays as given.

Input: name N and bytes B with content hash H.

1. Refusals; nothing changes: N not normalized → `invalid_name`; N ignored (M8) → `ignored_name`.
2. A present case variant of N with content hash H: nothing is written and the journal does not change; the result is that file's name. Calling this any number of times with the same bytes changes nothing (AE66: 100 calls, zero journal changes, no file written).
3. A present case variant of N with other content: that file goes to the trash (M10.1), then B is written as N. If the old name equals N, its journal entry takes the new hash; if it differs only by case, the old name gets a deletion and N a new entry. Both are pending (M11).
4. No present case variant: B is written as N and recorded, pending.

To decide step 2 the store may trust the journal's hash when the file's size and modification time still match what the journal recorded; otherwise it hashes the file.

For a CloudKit blob install (`sync/cloudkit-records#K9.3`), steps 2 and 3 apply only when the present file's exact name equals N. A present case variant whose exact name differs from N is a **case collision**: the install refuses with `case_collision`, writes nothing, trashes nothing and records no deletion, so the journal and the feed (`media/media-journal#J6`) do not change. CloudKit tracking then resolves it or reports it under needs attention (`sync/cloudkit-records#K9.3`, K15.9). The other callers keep steps 2 and 3 as written.

## M7. Which files are media: tracking and registration (R985, R1855, R1582)

### M7.1 Tracked and syncable files

A directory entry of the media folder is a **tracked file** when all hold:

- it is a regular file: directories, symbolic links and other kinds are skipped;
- its name is valid UTF-8;
- its name form: on Apple platforms a name that is not NFC is taken in its NFC form (the volume finds the file by either form); on other platforms a name that is not NFC is skipped;
- that name is normalized (`text/media-filenames#F1`); otherwise it is skipped until Check Media renames the file (R1646, U130);
- the name is not ignored (M8).

A tracked file is **syncable** when its size is at least 1 byte and at most 104,857,600 bytes (100 MiB). Only syncable files are ever pending for media sync (M11). Empty and larger files stay tracked: lookup finds them and they render, but they never sync; Check Media reports the large ones (R1645).

Names starting with `_` are ordinary media here; only Check Media treats them specially (R985, U130).

CloudKit uses the same range: a media-name entity starts only for a syncable file; a live entity whose file leaves the range stays and is marked unsyncable (`sync/cloudkit-records#K7`, K9.4). If CloudKit's asset limit proves lower than 104,857,600 bytes (`sync/cloudkit-records` K23 A2), K7 sets its own limit and this rule stays for manual Anki transfers.

### M7.2 Registration

Registration brings the journal in line with the folder:

1. Each tracked file that the journal does not record, or whose size or modification time differs from the record, is hashed, and its record takes the new size, time, hash and syncable state.
2. Pending changes follow from the sync view before and after:
   - a syncable file that is new, or whose hash changed, or that was not syncable before → pending, live;
   - a name the journal records as live that is no longer a syncable tracked file (removed, renamed away, emptied, grown past the limit, never tracked, such as M4's hash names ending in a space or `.`) → pending deletion;
   - anything else keeps its pending flag as it was: a file rehashed with an unchanged hash stays clean if it was clean.

### M7.3 When registration runs

- **Change check.** The store keeps a change marker for the folder: the folder's own modification time as of the last registration or of the store's last write that started with the marker in step. Creating, removing or renaming an entry moves the folder's time. When the current time equals the marker, the check ends there: no listing, no hashing (R1855; a 30,000-file folder answers in well under 0.5 s). When it differs, the store lists the folder and registers (M7.2), hashing only files whose size or time changed.
- The change check runs before a media sync (U111) and before every store operation that decides on present files: adds (M3), exact-name writes (M6), trashing and restoring (M10). So these see files placed in the folder from outside.
- After its own change to the folder, the store records the folder's new time as the marker only if the marker matched the folder just before that change. Otherwise an outside change made earlier would be hidden; it stays visible to the next check.
- A **full comparison** (list, compare every tracked file's size and time, hash what differs) runs only on request: Check Media (U130), a media resync, and the conformance observation `media_pending`. It runs off the writer, with progress and cancellation (R1855).
- A program that rewrites a file in place without creating, removing or renaming an entry does not move the folder's time, so only a full comparison finds that change. Anki has the same limit.
- Deletion tombstones are indexed so that listing pending changes costs the same with 20,000 tombstones as with none (R1582).

## M8. Ignored names (R985)

A name is ignored when, ignoring case, it consists of exactly:

- `thumbs`, then any one character, then `db`; or
- any one character, then `ds_store`.

An optional `:` before `thumbs` matches too, but `:` never survives the transform. "Ignoring case" is a regular expression's Unicode simple case folding: `S`, `s` and U+017F LATIN SMALL LETTER LONG S all match `s`; every other letter of these words matches only its two ASCII cases. The "any one character" is any character at all, `.` included.

| Ignored | Not ignored |
|---|---|
| `Thumbs.db`, `THUMBS.DB`, `thumbs_db`, `thumbsXdb`, `thumbſ-db` | `thumbs.db.png`, `thumbs.dbx`, `thumb.db`, `my-thumbs.db` |
| `.DS_Store`, `.ds_store`, `xds_store`, `ſDS_STORE` | `ds_store`, `..ds_store`, `my.ds_store` |

Ignored files are never tracked (M7.1), so they are never pending or synced and lookup never finds them. The add and exact-name paths refuse them (`ignored_name`).

## M9. Writes into the folder

- A file enters the media folder only by a rename from a temporary file on the same volume, through U32's temp-and-rename helper, so a crash never leaves a partial file under a final name.
- The rename never replaces an existing entry (a rename that fails when the target exists). Every replacement in this spec moves the old file to the trash first.
- If the volume refuses the rename because it treats an existing entry as the same name (a spelling the case key does not unite, such as `ς` and `σ`, a directory, or a file the journal has not seen yet), the store registers the folder (M7.2) and decides again. If the volume refuses again, an add stores B under the hash name (M4) instead; when that is refused too, or for an exact-name write (M6), the operation fails with `name_taken` and changes nothing.
- The volume's case sensitivity is probed when the collection opens and kept for diagnostics and for Check Media (U130). None of the rules in this file depends on it: Meguri treats names case-insensitively on every volume.

## M10. Trash (R2018)

The trash holds every media file Meguri removes. Emptying it is the only permanent deletion of media bytes, and runs only when the user asks (U130's Empty Trash, `maintenance/check-media#C17`); nothing empties it automatically (R1650). The trash is never synced and is not part of the media sync state. It survives relaunch.

### M10.1 Moving files to the trash

`trash(names)` takes each listed name on its own, in list order:

- If it resolves (M5) to a file, the file moves to the trash as a new entry recording the resolved name, content hash, size and the time. If the journal records that name as a live syncable file, the entry becomes a deletion, pending. If an entry with the same name is already in the trash, both stay: the trash keeps every version.
- If it resolves to nothing but the journal still records the name as live (the file vanished without a registration), the entry becomes a deletion, pending.
- Otherwise the name is skipped. A missing name never stops the names after it.

The exact-name form, `trash_exact(name)`, moves one entry by its exact listed name: the directory entry as the folder listed it, never put in NFC and never resolved to a case variant (M5 steps 1 and 3 do not apply). When a directory entry has exactly that name, it moves to the trash as a new entry recording that exact name, its content hash, size and the time, and the entry becomes a pending deletion only when the journal records that exact name as a live syncable file. When no directory entry has exactly that name, nothing moves, the journal does not change and the call reports the name as gone. The caller's re-check of the name runs in the same journal-serialized step as the move (`media/media-journal#J5`), so nothing changes the entry between the two. Its one caller is Check Media's fix (U130, `maintenance/check-media#C10` step 5). Decided 2026-10-27 (data safety first, from C10): resolving an old name by lookup could pick the kept file itself and trash referenced bytes; the exact-name move cannot.

Every Meguri path that removes a media file goes through this (R2018, U130's enumeration): Check Media fixes and Delete Unused, server deletions in media sync (U111), incoming CloudKit media-name tombstones and content conflicts (`sync/cloudkit-records#K9.4`, K15.9; U114), AnkiConnect's `deleteMediaFile` (U143), replacements by M4 step 4 and M6 step 3, and M13's launch recovery.

### M10.2 Restoring

`restore()` restores every entry, the most recently trashed first. For an entry with name T and bytes B:

- If no present file is a case variant of T, the bytes move back to the folder as T; the entry is recorded as live, pending if syncable.
- Otherwise B is added under the desired name T by the add rules (M3): identical content already present is dropped (nothing written), different content gets the lowercase candidate or the hash name. The trash entry is then removed.

Restoring never overwrites a file (R1649). With two versions of `a.jpg` in the trash and the name free, the newer version becomes `a.jpg` and the older is added after it, so it becomes `a-<hash>.jpg` (or is dropped when identical).

### M10.3 Emptying and size

- `empty()` deletes every trash entry permanently. The media folder and the journal do not change. (Check Media's Empty Trash uses identity forms of these two calls and deletes only the entries its preflight listed: `maintenance/check-media#C17`.)
- `summary()` returns the number of entries and their total size in bytes (Check Media shows it before emptying, `maintenance/check-media#C17`).
- `summary_with_ids()` returns what `summary()` returns, the count and the total size, together with the identities of the entries it counted (`media/media-journal#J3`), all read in one step, so the count and size are exactly those of the identities returned.
- `empty_entries(identities)` deletes permanently only the listed entries, one at a time, and skips an identity the trash no longer holds; it returns the count and bytes it deleted. A cancel is honoured between entries, and the entries already deleted stay deleted. An entry trashed after the identities were read is never deleted by it. The media folder and the journal do not change. These two forms are what Check Media's Empty Trash uses (`maintenance/check-media#C17`); it never calls `empty()`.

## M11. What makes a file pending for the next media sync (R986)

An entry is pending (dirty) after:

- an add or exact-name write that wrote a file (M3 rows b and c, M4 step 4, M6 steps 3 and 4);
- a registration that found a new, changed or removed syncable file (M7.2), including the one-file registration of M2's content hash and the full comparison;
- a move to the trash (deletion) and a restore (live again).

Nothing else marks an entry, and an entry whose name and content are unchanged is never marked: adding identical bytes again, `ensure_static_media` with identical bytes, and a registration that rehashes an unchanged file leave the journal as it was (R1571, AE66). A deletion is pending even when the file never reached the server; Anki sends such deletions too. A live pending entry always has a size from 1 byte to 100 MiB (M7.1).

Media sync (U111) clears the flag after the server accepts the change; the journal's sync fields are `media/media-journal`'s.

This flag is the Anki media-sync flag, used only by manual transfers (R1572). CloudKit does not read it, clear it or depend on it: M16.

## M12. Events

Each operation's change set lists the media names it added and removed, so views can refresh; files that media sync downloads, and files that a CloudKit blob install writes (`sync/cloudkit-records#K9.3`), are announced as `MediaArrived(names)` (U111, U114, U67). The event shapes are `media/media-journal`'s.

## M13. Files written inside an op (C9, R1702)

Inside an op (for example `/receive`, which adds a note and its media atomically, R1702), files are written to `media.staging/` and moved into the folder only after the op's transaction commits. The naming rules see staged files:

- a name chosen for a staged file (by M3 or M6) is present, with its content hash, for every later decision until its op ends;
- when the op commits, the file moves to that name and the journal records it (M11);
- when the op rolls back, its staged files are deleted: they never entered the folder and nothing references them, so no media file is left behind (R1702). The names are free again;
- after a crash, launch recovery moves the staged files of an op it judges uncommitted to the trash (M10.1) under their names, never deleting them, so a wrong judgement can be undone (R2018).

Editor attachments outside a note transaction are written at once; a file whose note is never saved is an orphan that Check Media reports. The manifest, launch recovery and the crash rules are `media/media-journal`'s.

## M14. Differences from Anki

Anki 26.09.3 decides with the files of the volume it runs on, so on a case-sensitive volume (the Linux oracle's, which records the goldens) it keeps case variants apart, and on a case-insensitive one it returns the spelling it was given rather than the file's. Meguri treats names case-insensitively everywhere (R984, R1570) and keeps every trashed version (R2018). Where a golden records the difference, `conformance/deviations.yaml` holds it:

| Situation | Anki (case-sensitive volume) | Meguri | Deviation |
|---|---|---|---|
| `MixedCase.png` = A present; add `mixedcase.png` or `MIXEDCASE.PNG` with A | writes a second file `mixedcase.png` | `MixedCase.png`, nothing written | `media-case-variant-same-content` |
| `MixedCase.png` = A present; add `MixedCase.png` with B | writes `mixedcase.png` | `mixedcase-<B>.png` | `media-case-variant-other-content` |
| `Pic.png` trashed, `pic.png` added, then restore | restores `Pic.png` beside `pic.png` | adds it as `pic-<hash>.png` | `media-restore-case-variant` |
| the same name trashed twice | the newer version replaces the older one in the trash | both kept; restore gives the older one a hash name | `media-trash-keeps-versions` |

Differences no golden records, because Anki fails or the scenario vocabulary avoids them:

- Add with a name that the transform empties: Anki fails with an I/O error; Meguri refuses with `invalid_name`.
- Add with an ignored name: Anki stores `thumbs.db` and marks it pending (then its next registration turns it into a deletion); Meguri refuses with `ignored_name`.
- Add of an empty or larger-than-100-MiB file: Anki marks it pending, then its next registration turns it into a deletion; Meguri never marks it.
- Trashing by a case variant (`Photo.jpg` for `photo.jpg`): Anki on a case-sensitive volume does nothing; Meguri trashes `photo.jpg`.
- Trashing a list in which a name has no file: Anki stops moving files at that name, yet marks every listed name it records as deleted, moved or not; Meguri handles each name on its own.
- Trashing an outside file that no registration has seen yet: Anki records no deletion; Meguri registers first (M7.3), so it records one.
- With two case variants both present (case-sensitive volume) and the bytes matching one that is neither N nor L: Anki writes a hash name; Meguri reuses that file (M3 row a).

On a case-insensitive volume (the native macOS oracle of `conformance/float-determinism`, on APFS), Anki agrees with Meguri where a case variant holds other bytes: an add writes the hash name of the lowercase candidate (`MixedCase.png` present, other bytes added as `MixedCase.png` → `mixedcase-<B>.png`), and a restore whose name a case variant holds adds the hash name (`media-restore-case-variant` gives exactly Anki's folder and pending entries there). Where a case variant holds the same bytes, Anki differs from both Meguri and its Linux self:

- it returns the spelling it was given (`Photo.JPG` for a present `photo.jpg`, `mixedcase.png` and `MIXEDCASE.PNG` for `MixedCase.png`, `Ǆ.png` for `ǆ.png`) and writes nothing, so the field holds a name that only a case-insensitive volume resolves;
- the next media sync would send a deletion for each such spelling, since the folder lists the file under its own name only.

Meguri returns the file's own name and records nothing (M3 row a, M11), on every volume. Seven of the thirteen media scenarios differ on that oracle for these reasons; the goldens stay the Linux oracle's (`conformance/media-ops#F13`).

## M15. Scenarios

Neutral scenarios under `conformance/scenarios/media/` use the vocabulary of `conformance/media-ops`:

- `add/ae36`: AE36.
- `add/hash-names`: M4's table, hash-name reuse.
- `add/lowercase`: M2's case keys and lowercase candidates, through adds.
- `case/same-content-variant`, `case/other-content-variant`, `case/restore-variant`: the M14 deviations.
- `trash/trash-restore-empty`, `trash/restore-conflict`, `trash/twice`: M10.
- `register/ignored`, `register/size-limit`, `register/outside-changes`: M7 and M8.
- `corpus/names-01`: 2,000 generated adds with case variants and collisions (`conformance/corpora/media/generate.py`), the naming differential.

Meguri-only behavior has no golden and is tested in Meguri's own tests (U31's test files): M5 lookup, M6 (`ensure_static_media`), M9, M10.1 `trash_exact` (the exact name, never put in NFC or resolved to a case variant; `gone` changes nothing; a pending deletion only for that exact live syncable name), M10.3 `summary`, `summary_with_ids` (count and size exactly those of the identities returned) and `empty_entries` (an entry trashed after the identities were read survives, a missing identity is skipped, a cancel between entries keeps the entries already deleted), M13 staging and its crash cases, the M7.3 change-check cost and the tombstone index, and the refusals.

## M16. CloudKit media state (R1572, R2057, KTD25)

- CloudKit media state (media-name entities, blob upload and download state, acknowledgements, unsyncable marks, tombstones) lives only in the CloudKit tracking store (`sync/cloudkit-records#K9`, K10.3). The journal holds none of it.
- Every store write that changes a name's file facts, including M3 rows b and c, M4 step 4, M6 steps 3 and 4, M7.2, M10.1, M10.2, M13's publish and recovery, the move of a local file to its hash name (`sync/cloudkit-records#K15.9`) and Check Media renames (U130), is also an item of the journal's file-fact feed (`media/media-journal#J6`). CloudKit tracking learns local media changes only from that feed and from its reconcile scan (`sync/cloudkit-records#K9.4`, K11.4).
- **Feed hook** (decided 2026-10-27, `sync/cloudkit-records#K11.1` "When the feed is read"). After each of the store operations listed above, and after each registration step (M7.2), whose journal transaction committed at least one feed item, the store calls CloudKit tracking's feed consume (`consume_media_feed`) once, after that commit and outside the journal transaction. The call consumes the feed up to the journal's current counter for the usable binding and returns; with no usable binding it does nothing, and the items wait for the next consume (at open, `resume()` or a Sync Now capture). A tracking failure in it is CloudKit's (`sync/policy#P15.2`) and never fails or rolls back the store operation. The store calls that CloudKit tracking itself makes for a media apply (`sync/cloudkit-records#K9.3`) call it too. Decided 2026-10-27, so that no item of an apply is read after its intent is removed: that consume runs before the store call returns to tracking, while the apply's intent exists, so it reads the call's items under the intent's expected states. The apply's own items make no dirty mark, and a local write that one of the apply's calls replaced gets its conflict copy in that consume (K9.3 "Feed items", K16). So a picture added through the editor, a Delete Unused or a Check Media fix reaches CloudKit tracking as a local change at once.
- The Anki media-sync state (M11's pending flag, `sync/media-tracking`'s sync state, media USN, change marker and baseline owner) is separate. Clearing a pending flag after an Anki server accepts a change, an Anki media reset (`sync/media-tracking#T11`, after a logout, a server or account change or a sanity mismatch) and every other manual-transfer step leave CloudKit tracking, the feed's retained items and its floor unchanged (J6).
- So a file that Meguri removes (absence cause `removed`, `media/media-journal#J6`) while an Anki media transfer runs, or before an Anki logout, still reaches CloudKit as a tombstone, and a file whose content changes then still reaches it as a new version. A file that disappears outside Meguri (deleted in Finder, evicted, left out by a restore: cause `missing`) makes no tombstone; CloudKit tracking treats it as a missing local file (`sync/cloudkit-records#K9.4`).
