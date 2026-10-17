# Backup triggers: interval rule, change rule, forced backups (U127)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to write
`specs/package/backup-triggers.md` (T2.1, T2.2, T4.1; the rest is Meguri
design) and `specs/conformance/backup-trigger-ops.md`. No code was copied; the
rules were re-derived in the spec writer's words, then checked by black-box
probes of the pinned `anki` wheel in the oracle image (2026-10-17, image
`meguri-oracle:26.9.3-953389e9799027f3`, network off, libfaketime frozen clock
as in `entrypoint.sh`, UTC):

- an empty folder and a changed collection: a backup that is not forced is
  made; a folder holding only `notes.txt`, only a subdirectory, or only a
  `.meguri-tmp-x` file: none is made; a forced one is;
- interval 0: a second backup with no change is skipped, a note added in the
  same millisecond as the reference is no change, a later note is, an undo
  that puts `col.mod` back is no change, a raw write moving `col.mod` an hour
  back is a change; after close and reopen with no change a backup is made;
- forced backups with no change in the same opening: skipped, also a minute
  later;
- interval 30 with entries present: skipped at +1 min, and still skipped at
  +29 min 59 s, +30 min, +5 h and +10 h of fake time, because libfaketime
  shifts every reported file time by the fake clock's lead over the real one
  so entries never age in the oracle (backup-trigger-ops H2);
- intervals 1, 71582788 and 4294967295 block on a fresh foreign file;
  1073741824 does not (the 32-bit product wraps to 0);
- each `package/backup-triggers` scenario replayed step by step on the pylib
  API gave the outcomes its comments state.

| Spec section | Anki file | Read for |
|---|---|---|
| backup-triggers T2.1 | `rslib/src/collection/timestamps.rs` (change since the last backup, the reference taken from the collection stamps), `rslib/src/collection/mod.rs` (the reference kept in the open collection's state), `rslib/src/collection/backup.rs` (change check before the interval check, for forced backups too; reference taken before the write thread starts) | the reference, its lifetime per opening, the inequality test |
| backup-triggers T2.2 | `rslib/src/collection/backup.rs` (recent-backup check over every directory entry's creation time, modified time on Android only, entries in the future dropped, 32-bit minutes × 60) | which entries count, the comparison, the wrap |
| backup-triggers T2.2, backup-trigger-ops H1 | `rslib/src/backend/collection.rs` (backup request: awaits any earlier backup task, waits when asked), `pylib/anki/collection.py` (backup call with force and wait flags; returns whether a backup was made) | the driver call and its boolean |
| backup-triggers T3 (comparison only), T4.1 | `qt/aqt/main.py` (5-minute periodic timer; close backup not forced, skipped when the quick check fails or while restoring; "Create Backup" forced, "unchanged" tooltip when skipped; restore through the collection package import with no backup first, auto sync off until the profile closes; close backup after the closing sync), `qt/aqt/sync.py` (forced backup before a full download only), `qt/aqt/dbcheck.py` (forced backup before Check Database), `qt/aqt/import_export/importing.py` (forced backup before a `.colpkg` import) | what Anki forces and when it backs up |
| backup-triggers T8 (comparison) | `qt/aqt/main.py`, `qt/aqt/import_export/importing.py` | restore is the `.colpkg` import; stamps are `engine/collection-replace` R7 (`package.md`) |

No Anki test covers the triggers (the two thinning tests in
`rslib/src/collection/backup.rs` belong to U126's retention scenarios), so no
harvest row is classified with this unit.
