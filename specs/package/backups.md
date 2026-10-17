# Backups: names, writer, listing and retention

Spec: package/backups
Requirements: R1629, R1633, R1634, R1635, R2009, R1540 (server-copy names and listing); budgets R1848, R1852; supports R2004, R2015, R2017
Anki: 26.09.3
Scenarios: conformance/scenarios/package/backups/

Meguri's backups are files Anki can restore: collection-only packages in the latest format, named after the local time they were made, kept in a backups folder and thinned by Anki's retention rule with the limits of the synced `backups` config key. This file fixes the names (B2), the local time they use (B3), the writer (B4), retention (B5), the limits (B6) and the listing (B7). The package itself is `package/container#P11`. When backups happen (periodic, forced before destructive operations, "Back Up Now") and restore are U127's (`package/backup-triggers`). The scenario vocabulary is `specs/conformance/backup-ops.md`.

## B1. The backups folder

- The host passes the folder's path when it opens the collection, separately from the collection directory (KTD12, U151): `Documents/Backups` on iOS and iPadOS, where the Files app shows it (R1629), and a folder in the app's private container on the Mac (Show Backups, U163a). The core never derives the path from anything else.
- The writer creates the folder, and any missing parent, when it does not exist: the user can delete it in the Files app, and every later backup would fail otherwise.
- The folder may hold anything: Meguri's backups and server copies (B2), U32's unpublished work (`.meguri-tmp-…` files, `.meguri-stage-…` directories, `storage/durability-and-files#F4`), files the user put there, and backups that Anki or another Meguri device made and the user copied in. Only B2's names mean anything to this spec. Every other entry is foreign: the listing ignores it and retention never deletes it.

## B2. File names

### B2.1 Backups

`backup-YYYY-MM-DD-HH.MM.SS.colpkg`: the literal `backup-`, the year in four digits, `-`, the month in two, `-`, the day in two, `-`, the hour (00 to 23) in two, `.`, the minute in two, `.`, the second in two, then the literal `.colpkg`. Digits are ASCII and zero-padded. The date and time are the local wall time at which the backup was made (B3).

Examples: `backup-2030-01-15-10.00.00.colpkg`; `backup-2030-01-16-05.00.00.colpkg` (05:00 in the morning).

### B2.2 Server copies (R1540)

Copies of a sync server's collection that U109 keeps before a one-way upload over a self-hosted server. Meguri's own files: Anki has no such copy and its retention ignores these names.

| Name | Content | Listing kind |
|---|---|---|
| `server-copy-YYYY-MM-DD-HH.MM.SS.colpkg` | a collection-only package of the server's collection (`package/container#P11`, finalisation `Replace`) | `server_copy` |
| `server-copy-YYYY-MM-DD-HH.MM.SS.unverified.anki2` | the raw bytes of a server collection that failed the integrity check or the open gate, so it could not be wrapped as a package | `server_copy_unverified` |

The date-time fields follow B2.1 and give the local time of the copy (B3). Retention never deletes a server copy of either kind.

### B2.3 Recognising a name

A directory entry is a backup, or a server copy, when all of these hold:

- it is a regular file (not a directory, not a symbolic link, not any other file type);
- its whole name matches B2.1 or a row of B2.2 exactly, letter case included, with nothing before or after;
- every field has exactly its width in ASCII digits;
- Require a valid Gregorian date. Accept hours 00–23 and minutes and seconds 00–59. The year has four digits.

Anything else is foreign (B1).

| Name | Meguri | Anki 26.9.3 |
|---|---|---|
| `backup-2029-06-01-12.00.00.colpkg` | backup | backup |
| `backup-2029-02-29-12.00.00.colpkg` | foreign (no such date) | foreign |
| `backup-2029-06-01-24.00.00.colpkg` | foreign | foreign |
| `backup-02029-06-01-12.00.00.colpkg`, `backup-2029-06-01T12.00.00.colpkg`, `backup-2029-06-01.colpkg` | foreign | foreign |
| `Backup-2029-06-01-12.00.00.colpkg`, `x-backup-2029-06-01-12.00.00.colpkg`, `backup-2029-06-01-12.00.00.colpkg.bak`, `backup-2029-06-01-12.00.00.apkg` | foreign | foreign |
| `backup-2029-6-1-12.00.00.colpkg`, `backup-2029-06-01-12.0.0.colpkg` (one-digit fields) | foreign | backup |
| `backup-29-06-01-12.00.00.colpkg` (year 29), `backup-+2029-06-01-12.00.00.colpkg`, `backup--2029-06-01-12.00.00.colpkg` (year −2029) | foreign | backup |
| `backup-2029-06-01- 12.00.00.colpkg` (a space before a number) | foreign | backup |
| `backup-2029-06-01-12.00.60.colpkg` (second 60) | foreign | backup |
| `server-copy-2029-06-01-12.00.00.colpkg` | server copy | foreign |

The reference parser also accepts shorter numeric fields, signed years, leading spaces and second 60.
Meguri accepts only B2.3 names and preserves every other spelling.
Neither writer produces those alternate spellings. See deviation `backups-noncanonical-names-ignored` and `package/backups/name-variants`.

## B3. Local time

- The clock gives the current instant, and the host gives the local UTC offset at that instant (Swift's time zone offset for that date; rust-stack §5 time). The core has no time-zone database.
- **Name of a new file.** Take the instant, drop the fraction of a second, add the offset, and write the wall date and time of the result in B2.1's fields.
- **Today**, for retention, is the wall date of the current instant plus the current offset, when retention runs. Yesterday is the day before it.
- **Date of a file.** The date written in a name is the file's day for retention, whatever the offset is now. A change of time zone never renames a file or moves it to another day.
- Example: at 2030-01-15T20:00:00Z, with offset +09:00 (Asia/Tokyo), the new backup is `backup-2030-01-16-05.00.00.colpkg` and today is 2030-01-16; with offset −05:00 (America/New_York) it is `backup-2030-01-15-15.00.00.colpkg` and today is 2030-01-15 (scenarios `package/backups/offset-east`, `offset-west`).
- **Wall times that do not exist.** Anki reads names in the device's time zone with its daylight-saving rules: a name whose wall time falls in the hour skipped when daylight-saving time starts (2029-03-11 02:30 in New York) is not a backup for Anki, so it never deletes it. Meguri, which knows only the current offset, reads it as a backup like any other. A device in that zone never writes such a name. Deviation `backups-dst-gap-name-thinned` (scenario `package/backups/dst-gap-name`). Wall times that occur twice (the hour repeated when daylight-saving time ends) are ordinary names for both.

## B4. The writer

Writes one backup of the open collection and then thins the folder. It runs as a background job and never blocks study (R1635, R1848): it never takes the collection's writer, and answers keep committing while it runs.

1. Make sure the folder exists (B1).
2. Take the backup's instant and its name (B3).
3. **Free name.** If an entry with that name already exists in the folder, use the next second's name, and so on, until a name is free, at most 60 seconds past the instant. If none is free, fail with `DestinationExists` and write nothing.
4. Write the package with `package/container#P11`: a snapshot of the live collection taken with `VACUUM INTO` on the maintenance reader (it contains every commit made before the snapshot's read began, never the live file's bytes, and never the whole file in memory), the disk preflight of P10, the temporary file with U32 purpose `backup`, the read-back verification, and finalisation `ExclusiveCreate` under the chosen name: the publication never replaces an existing file.
5. **Lost race.** If publication fails with `DestinationExists` (another writer took the name after step 3), start again at step 2 with a new instant and a new snapshot. After three attempts in all, fail with `DestinationExists`.
6. **Retention.** Once the file is published (synced to disk with its directory, P11), run B5 with the new file's name protected, and delete the files it selects.
7. **Result**: the new file's name, its size in bytes, the instant it is named after, and the names retention deleted. U127 builds "Back Up Now"'s report (R1632) from it.

- **Failure.** Any failure before publication returns P13's error (`InsufficientSpace`, `DiskFull`, `Io`, `Cancelled`, `DestinationExists`). Nothing appears under a backup name, the temporary file and the staging directory are removed (P11), retention does not run and earlier backups keep their bytes.
- **Cancel** is honoured until publication. A published backup stays, and its retention runs.
- **Retention failures** never fail the backup: a file that cannot be deleted is logged with its name and skipped, a file already gone counts as deleted, and a folder that cannot be read for retention is logged. The result lists only the files actually deleted.
- **Concurrent writers.** Two backups may run at once (a periodic backup and "Back Up Now"). Steps 3 and 5 give them distinct names, and each retention deletes only files that existed when it read the folder and that it did not protect; the other writer's new file is always dated today, so no retention selects it.
- Example: two forced backups start in the same second S, with a deck deletion committed between their snapshots. Both files exist: one named after S without the deletion, the other named after S + 1 second with it.

Anki differs here without a scenario showing it: it reads the whole collection file into memory after a checkpoint, writes the package straight to the final name (a crash leaves a partial file there), and a second backup in the same second overwrites the first.

## B5. Retention (R1633)

A pure decision over names: given today (B3), the names of the backups in the folder (B2.1, recognised by B2.3), the limits (B6, read when the backup starts) and the protected names, it returns the backups to keep and the backups to delete. Server copies and foreign entries never enter it.

### B5.1 The rule

Process backup names from newest to oldest by their encoded date and time.
Keep the most recently retained backup as the anchor; initially there is none.
Start three remaining counts from the daily, weekly and monthly limits.

1. A backup dated yesterday, today or later is kept and becomes the anchor.
2. For an older backup, choose the first positive remaining count: daily, then weekly, then monthly.
   Keep it if no anchor exists, or its chosen period precedes the anchor period.
   Compare dates, Monday weeks or calendar months according to that chosen count.
   On retention, decrement that count and set this backup as the anchor. Otherwise delete it.
3. Delete an older backup when every remaining count is zero.

- A week starts on Monday: two dates are in the same week when the Monday on or before each is the same date. Sunday 2030-01-13 and Monday 2030-01-14 are in different weeks.
- A month is a calendar month of a year.
- Protected names are never deleted. They take part in the walk like any other backup; if the walk selects one for deletion, it is kept anyway, and the walk is not changed by that. The writer protects the backup it has just published. U127 protects a safety backup until the operation it guards has committed.
- Retention deletes only what step 2 or 3 selected: a file whose name is a backup name and is a regular file (B2.3). It never deletes a server copy, a foreign entry, a directory or a symbolic link.

### B5.2 What the rule gives

- Every backup from yesterday and today (and any dated later) stays.
- With a daily limit d above zero, the next d older days that have backups each keep their newest backup.
- Then the weekly limit counts weeks strictly older than the week of the last backup kept before it, keeping the newest backup of each; the week of the last daily backup is not counted again. The monthly limit does the same with months older than the month of the last backup kept before it.
- A zero limit skips its stage. Besides the backups dated yesterday or later, retention keeps at most daily + weekly + monthly backups.
- Running retention again with the same today and limits on what it kept deletes nothing.
- Older backups no more numerous than the daily limit, each on its own day, all stay. Two backups of the same day older than yesterday never both stay.

### B5.3 Examples

Today Tuesday 2030-01-15, limits daily 1, weekly 1, monthly 1 (scenario `package/backups/period-boundaries`):

| Backup | Decision | Why |
|---|---|---|
| 2030-01-15 10:00:00 | keep | today |
| 2030-01-14 09:00:00 | keep | yesterday; anchor |
| 2030-01-12 20:00:00 | keep (daily) | first older day; anchor |
| 2030-01-12 08:00:00 | delete | weekly now; same week as the anchor (Monday 2030-01-07) |
| 2030-01-08 12:00:00 | delete | same week |
| 2030-01-07 12:00:00 | delete | same week |
| 2030-01-06 12:00:00 | keep (weekly) | week of Monday 2029-12-31 is earlier; anchor |
| 2030-01-02 12:00:00 | delete | monthly now; same month as the anchor (January 2030) |
| 2029-12-20 12:00:00 | keep (monthly) | December 2029 is earlier |
| 2029-12-02 12:00:00, 2029-06-01 12:00:00 | delete | all three limits exhausted |

AE69 retains 66 files under defaults 12/10/9.
Input: hourly backups from 2028-12-11 09:00:00 through 2030-01-15 09:00:00, plus 2030-01-15 10:00:00.
The retained set is 35 files from January 14–15; 12 daily files at 23:00 from January 2–13;
10 weekly files at Sunday 23:00 for Monday weeks October 22–December 24, 2029;
Monthly: nine files, the final backup in each month from 2029-01 through 2029-09.
The daily anchor is in the December 31 week; the weekly anchor is in October.
Scenario: `package/backups/retention-hourly-400-days`.

## B6. Limits: the `backups` config key (R1634)

The key is an ordinary synced key of the `config` table (`storage/config-table` C1; sync can replace it, C9). Its value is a JSON object with four members (B6.1 also reads an array):

| Member | Meaning | Default |
|---|---|---|
| `daily` | B5's daily limit | 12 |
| `weekly` | B5's weekly limit | 10 |
| `monthly` | B5's monthly limit | 9 |
| `minimum_interval_mins` | the minutes between periodic backups (U127, `package/backup-triggers`) | 30 |

### B6.1 Reading

- The value is read when it is, as `storage/config-table` C3 reads JSON, either an object that has each of the four members exactly once, or an array of exactly four elements taken in the table's order; and each of the four values is an integer (C3) from 0 to 4294967295. Member names compare exactly. An object's other members are ignored.
- Otherwise all four take their defaults together, never some: the key is missing, `val` is not a BLOB, the text is not JSON, the value is another kind, a member is missing, repeated (an exception to C3's "last occurrence counts"), a float, negative, above 4294967295, or not a number.
- A read never writes.

| Stored text | Read as (daily, weekly, monthly, interval) |
|---|---|
| (no key) | 12, 10, 9, 30 |
| `{"daily":5,"weekly":4,"monthly":3,"minimum_interval_mins":60}` | 5, 4, 3, 60 |
| the same with spaces, other member order, or an extra member `"extra":[1,{"a":2}]` | 5, 4, 3, 60 |
| `[5,4,3,60]` | 5, 4, 3, 60 |
| `{"daily":4294967295,"weekly":4,"monthly":3,"minimum_interval_mins":60}` | 4294967295, 4, 3, 60 |
| `{"daily":5}`, `{}`, `[5,4,3]`, `[5,4,3,60,1]` | 12, 10, 9, 30 |
| `daily` as `5.0`, `1e1`, `-0`, `-1`, `4294967296`, `"5"` or `null` (others valid) | 12, 10, 9, 30 |
| `daily` twice, `Daily`, or `minimumIntervalMins` in place of `minimum_interval_mins` | 12, 10, 9, 30 |
| valid text stored as TEXT, `null`, `"x"`, not JSON | 12, 10, 9, 30 |

### B6.2 Writing

The limits edit (U133a's call, U163a's Backups page) sets all four values, each an integer from 0 to 4294967295.

1. Start from the stored value's members when it is a JSON object stored as a BLOB (when a name repeats, its last occurrence counts, C3); otherwise from no member.
2. Set `daily`, `weekly`, `monthly` and `minimum_interval_mins` to the new integers. Every other member keeps its value and kind (C6).
3. Write as `storage/config-table` C4: when the result is the same value as the stored one, nothing is written; otherwise the row gets the result as a BLOB, `usn` −1, `mtime_secs` now, and `col.mod` moves.
4. The text is C5's compact form with one change of order: `daily`, `weekly`, `monthly`, `minimum_interval_mins` first, in that order, then the other members in C5's sorted order. Without other members this is the text Anki writes: `{"daily":3,"weekly":2,"monthly":1,"minimum_interval_mins":45}`.

- The write is a Settings write: no undo entry, and a write that changes the key clears the undo and redo history, as `browser/config-keys` K10.2 says for Settings writes. It reports a config change naming `backups`. Anki's preferences save records an undoable step instead; no scenario observes undo here.
- Anki writes only the four members and drops every other member. Meguri keeps them (R2017). Deviation `backups-limits-unknown-members-kept` (scenario `package/backups/limits-write-unknown-member`).
- The core accepts the whole range, as Anki's does. Anki's desktop settings offer 0 to 9999 for the three counts and 5 to 9999 minutes for the interval; U163a uses the same ranges.

## B7. Listing

Lists what Restore can offer (R1637, U127, U133a), without opening any file.

- Each regular file of the folder whose name B2.3 recognises gives one entry: its file name, its kind (`backup`, `server_copy` or `server_copy_unverified`), the date and time written in its name (year, month, day, hour, minute, second: local wall time, B3), and its size in bytes from the file system.
- Order: newest first by that date and time; equal date-times by file name in ascending byte order.
- Foreign entries (B1) are left out, U32's temporary files and staging directories among them.
- A folder that does not exist gives an empty listing. A file that disappears while the folder is read is left out. Any other failure to read the folder is `Io`.
- The listing never reads file contents, so a damaged file is listed like any other. Restore inspects the file before using it (`package/container#P12`, U127).

## B8. Budgets

- A backup of a 150,000-card collection, retention included, takes at most 2 s of background work (R1848), measured by `core/crates/meguri-package/benches/backup.rs`.
- Peak memory stays within R1852 (300 MB on iPhone at 500k cards): SQLite writes the snapshot, it is compressed in bounded chunks (`package/container#P8`), and retention and the listing hold names, never contents.
- The backup never makes an answer wait (R1635): answers on the interactive lane commit while it runs.

## B9. Scenarios and tests

| What | Where |
|---|---|
| AE69: default limits over 400 days of hourly backups | `package/backups/retention-hourly-400-days` |
| Fewer backups than the limits, on distinct days: all stay | `package/backups/fewer-than-limits` |
| Daily-to-weekly and weekly-to-monthly anchors | `package/backups/period-boundaries` |
| Monday-start weeks | `package/backups/week-boundary` |
| Names and today from the offset (+09:00, −05:00) | `package/backups/offset-east`, `offset-west` |
| Foreign files, server copies, future-dated backups; all limits zero | `package/backups/foreign-files` |
| Spellings Anki reads and Meguri does not (deviation) | `package/backups/name-variants` |
| A name in the skipped daylight-saving hour (deviation) | `package/backups/dst-gap-name` |
| Reading the limits: defaults, shapes, all-or-nothing | `package/backups/limits-read` |
| Writing the limits: text, same value, shapes replaced | `package/backups/limits-write` |
| Other members kept by the limits write (deviation) | `package/backups/limits-write-unknown-member` |
| T1: Meguri's backup listed and restored by Anki; the dump equals the source's (R2009) | `package/backups/restore-in-anki` |
| B5 properties over generated folders: idempotent, never deletes yesterday or later or a protected name, at most daily + weekly + monthly older backups kept, deletes only recognised backup names; expected sets from B5.1 written out in the test, never from the code | `core/crates/meguri-package/tests/backup_retention.rs` (proptest) |
| B2.3 table (Meguri's column; `noncanonical_names_are_foreign`), a name in a skipped hour read as a backup (`dst_gap_name_is_a_backup`), B6.1 table | same file |
| B6.2: the text of each write, no write for the same values, an unknown member kept byte for byte when it was stored in compact form (`limits_write_keeps_other_members`) | same file |
| 400 days of hourly backups plus a server copy of each kind: both copies stay and the listing labels them | same file |
| Folder with U32 temporary files and staging directories: never deleted, never listed | same file |
| Two forced backups in the same second with a deck deletion committed between them: two files, the first without the deletion (B4) | `core/crates/meguri-package/tests/backup_writer.rs` |
| Name taken (B4 step 3) and a lost race (step 5) | same file |
| Disk full during the write (U32 fault point): no `backup-*` file, error returned, earlier backups byte-identical; injected low space: `InsufficientSpace` before any write | same file |
| Folder missing: created; a backup written | same file |
| Answers on the interactive lane during a backup: each commits without waiting for it; the backup holds every answer committed before its snapshot | `core/crates/meguri-package/tests/backup_writer.rs` |
| 150k cards: at most 2 s, peak memory recorded | `core/crates/meguri-package/benches/backup.rs` |

Goldens: none of the `package/backups` scenarios has one yet. They need the oracle ops and observations of `specs/conformance/backup-ops.md` (F2 to F6), which the oracle maintainer adds before recording them with `tools/mg oracle record package/backups`. Every expected value in B2.3, B3, B5.3 and B6 was checked against black-box runs of Anki 26.9.3 in the oracle image (`oracle/provenance/package.md`).
