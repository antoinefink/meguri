# Backup ops and observations for scenarios

Spec: conformance/backup-ops
Requirements: R1629, R1633, R1634, R2009, R2017
Anki: 26.09.3
Scenarios: conformance/scenarios/package/backups/

The scenario vocabulary for backups (U126): the ops `add_backup_files`, `force_backup` and `save_backup_settings`, and the observation kinds `backup_files` and `backup_settings`. They belong to the package area (`dsl.md` D7.2). The behavior observed is `specs/package/backups.md`.

## F1. The backups folder of a scenario

- Every scenario has one backups folder: the directory `backups` in its session directory, empty when the scenario starts. Each executor creates it before the first step that needs it.
- Meguri passes that path to the core as the collection's backups folder (`package/backups#B1`).
- The folder is part of the session directory, so it crosses a handoff (`dsl.md` D9.2).

## F2. `add_backup_files`

Puts files into the backups folder, as a user or an earlier backup would have left them.

- **Argument**: exactly one of
  - `{names: [<file name>, ...]}`: the files, in any order;
  - `{series: {from: <wall time>, until: <wall time>, every: <duration>}}`: the backup names (`package/backups#B2.1`) of the wall times `from`, `from + every`, `from + 2 × every`, ... up to and including `until`. Wall times are `YYYY-MM-DDTHH:MM:SS` with no offset and are stepped as plain calendar arithmetic, without any time-zone rule (the series never skips or repeats an hour). `every` is a D7.1 duration of at least one second; `until` is not before `from`.
- A file name is not empty, has at most 255 bytes of UTF-8, contains no `/` and no NUL, is not `.` or `..`, and does not begin with `.meguri-` (U32's names for unpublished work, which Meguri's launch sweep removes). Names may hold spaces and any other character.
- **Effect.** Each named file is created as an empty regular file. One step creates at most 20,000 files; names repeated within the step, and names that already exist in the folder, are `invalid_scenario`. The collection does not change.
- **Output**: none; the step records `{}`.

## F3. `force_backup`

- **Argument** `{}` or `{copy_as: <file name>}`. `copy_as` follows the `.colpkg` name rule of `package-ops.md` E1.
- **Effect.** A forced backup of the open collection into the backups folder, followed by its retention, both complete when the step ends.
  - Oracle: Anki's collection backup with the interval check off, waiting for it to finish (its backup thread writes the package and then thins the folder).
  - Meguri: the writer of `package/backups#B4`, retention included. None of U127's checks (minimum interval, changed since the last backup) runs.
  - With `copy_as`, the executor then copies the new backup's bytes to that name in the session directory, replacing a file of that name, so that `package_listing` and `import` (`package-ops.md` E2, E3) can read it as a package an `export` wrote.
- **Output** `{force_backup: {name: <the new backup's file name>}}`.
- Anki's other failures are run errors (`anki_error`); Meguri's are executor errors. No rejection kind exists.

## F4. `save_backup_settings`

- **Argument** `{daily, weekly, monthly, minimum_interval_mins}`: all four, integers from 0 to 4294967295.
- **Effect.**
  - Oracle: Anki's preferences save carrying only the backup group (no scheduling, reviewing or editing group), so that only the `backups` key can change.
  - Meguri: the limits write of `package/backups#B6.2`.
- **Output**: none; the step records `{}`.
- Anki rewrites the key whenever the new compact text differs from the stored bytes; Meguri compares values (`storage/config-table` C4). Scenarios run this op over a stored value that Anki's own save wrote, or over one that differs from the new value, or apply deviation `config-same-value-other-bytes`.

## F5. `backup_files` (observation kind)

- **Request**: `backup_files` (no argument).
- **Value**: the names of the regular files in the backups folder, sorted by their UTF-8 bytes. Directories and other entries are not listed.
- Never changes the folder or the collection.

## F6. `backup_settings` (observation kind)

- **Request**: `backup_settings` (no argument).
- **Value** `{daily, weekly, monthly, minimum_interval_mins}`: the limits as the executor reads them now, defaults included.
  - Oracle: the backup group of Anki's preferences.
  - Meguri: the read of `package/backups#B6.1`.
- Never writes.

## F7. Scenario rules

- **One backup per opening.** Allow at most one `force_backup` after each start, `open`, `reopen`, `import` or `handoff`.
  The reference skips a later unchanged forced backup within that opening. Meguri always writes it.
  Refuse extra calls with `invalid_scenario`.
- **Time zone.** Names come from the scenario's clock and zone (`dsl.md` D5): the oracle's worker runs in `clock.tz`, Meguri's executor passes the zone's offset at the current instant (`package/backups#B3`).
- **No collision.** A seeded name never equals the name a `force_backup` takes at that instant: Anki would overwrite the seeded file and Meguri would take the next second (`package/backups#B4`).
- **Skipped hours.** Anki and Meguri read a name whose wall time falls in an hour skipped by daylight-saving time differently (`package/backups#B3`): only `dst-gap-name`, with its deviation, holds one, and no series crosses a skipped hour. Names in a repeated hour are read alike.

## F8. Scenarios and what runs them

| Scenario | Shows | Needs on Meguri |
|---|---|---|
| `retention-hourly-400-days` (AE69) | default retention over 400 days of hourly backups | new collection, `add_backup_files`, `force_backup`, `backup_files` |
| `fewer-than-limits`, `period-boundaries`, `week-boundary`, `offset-east`, `offset-west`, `foreign-files` | B5 and B3 cases | the same, `setup.config` |
| `name-variants` (deviation `backups-noncanonical-names-ignored`), `dst-gap-name` (deviation `backups-dst-gap-name-thinned`) | names Anki and Meguri read differently | the same |
| `limits-read` | B6.1 | new collection, `set_config`, `db_exec`, `backup_settings` |
| `limits-write`, `limits-write-unknown-member` (deviation `backups-limits-unknown-members-kept`) | B6.2 | new collection, `set_config`, `save_backup_settings`, `db_exec`, `backup_settings` |
| `restore-in-anki` (live, T1) | Anki lists and imports Meguri's backup; the dump equals the source's (R2009) | `open`, `timing`, `db_dump`, `force_backup`, `backup_files`, `handoff` |

- **Capabilities** (`dsl.md` D8): the ops `add_backup_files`, `force_backup` and `save_backup_settings`, and the kinds `backup_files` and `backup_settings`. Meguri declares them with U126's adapter in `core/crates/meguri-testkit/src/exec/package.rs`.
- The oracle driver implements F2 to F6. Every scenario has a golden, recorded with `tools/mg oracle record package/backups`.
