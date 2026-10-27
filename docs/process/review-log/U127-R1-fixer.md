# U127 R1 ownership-cycle fix

- Fixer session: `s-20261025T162405Z-9dc656e4`.
- Starting filtered snapshot: `291b6c9581fc1b84e8a09bdb86640838f3d09447`;
  resumed the supplied U127 structural safety and U29/U28/U49 seed.
- Finding: `U127-BACKUP-ENGINE-OWNERSHIP-CYCLE` (R1 P1).
- Change: the production backup provider retains `WeakEngine`, upgrading only
  for an operation and returning an unpublished safety failure after owner drop.
  Composition-root ownership, safety registration, overrides, publication,
  retention and metadata paths remain in place.
- Regression: hold the existing export handoff to refuse host drop's close,
  drop host/process/engine/handoff owners without calling close, assert writer
  exit and expired-provider refusal, then reopen with an independent host and
  read the committed witness. No sleeps, retries or relaxed assertions.
- Handoff verification is limited to one `git diff --check`. No jobs, tests,
  builds, network access, history reads or commits; independent R2 review,
  compilation and integration checks belong to root. No acceptance claim.
- Preserve the complete supplied seed, including T6 admission and schema
  confirmation removal; export it together with this correction.

## R2 corrections

- Same fixer session and supplied full U127 seed; preserve newer master.
- `before_deck_delete` now uses the existing upgrade/error guard and retains
  the local engine through collection access and backup completion.
- Removed only the new expired-provider `before_manual_transfer(None, ...)`
  assertion; retained its mechanical weak-owner upgrade and all other lifecycle
  regression assertions.
- One narrow field-use and patch consistency check; no jobs, tests, builds,
  network, history reads or commits. Root owns independent R3 and compilation.
  No acceptance claim.

## R5 corrections for root's execution and independent R6

- Same fixer session `s-20261025T162405Z-9dc656e4`; starting filtered snapshot
  `a861bd68af4d797d0539984fd47fb7dfc5677684`. Continued the complete supplied
  seed, including structural safety, weak ownership and U49 snapshot cleanup.
  Read current R6.5/T8 and O8/O9 clarification; no normative specification edits.
- Activated restore reevaluation errors now carry `RestoredBackup` and report
  the candidate active, retaining the name, safety name, generation and fence.
  Package `restore.rs` tests inject failure after activation and check cards,
  safety and Pending before and after reopen.
- Maintenance retains deferred arm/recheck requests through suspension and
  drains them once on resume. A suspend-cancelled attempt keeps its deadline
  without inventing a recheck. Engine `maintenance_scheduler/periodic.rs`
  covers actual suspend/resume with rollover and an offline mod change, plus
  retained armed deadlines and the existing cancellation regression. `Policy`
  owns a disjoint temporary backup root through its reopen cases.
- Quit creates its existing absolute two-second token before lane admission.
  Both maintenance and provider lanes use condition-variable notifications,
  token wakeups and that same deadline. Suspension also interrupts admission
  waiters. No polling, retry, added idle timer, detached work or owner drain is
  used to acknowledge another operation's budget. Cancellation futures release
  their waker mutex before a deadline can interrupt them.
- Blocked-periodic regression: engine `maintenance_scheduler/periodic.rs`.
  Blocked real forced writer: package `backup_service.rs`, at the existing
  `storage.backup.before_rename` hook. It compares the retained temporary file's
  inode and bytes before releasing the owner, then proves that inode publishes.
  Their nextest isolation entries retain the production two-second budget.
- Package T7 verifies the Restore WAL authority, then passes it to the existing
  initial replacement. In `device_state/retained_wal.rs`, the R6 intent holds
  SHA-256 digests, lengths and native identities for source and copied files,
  plus complete directory bindings checked without following links. Its required
  Phase values stop main-only readers until WAL reconciliation and directory
  synchronization are durable. Storage moves the old WAL through a pinned
  directory with its existing syscall that refuses an occupied destination.
  The intent saves disposition before moving the WAL aside or moving it back.
  Recovery proves the main file first and refuses ambiguous identical bytes;
  it returns the WAL before opening the old database and keeps it away from a
  candidate. The raw copies and old WAL recovery files stay available. Before
  candidate rename, R5 still requires the live WAL to be absent or empty.
- The existing engine `replace.rs` crash worker and parent now cover saving
  intent, file actions and metadata updates, moving the old WAL aside and back,
  main rename, activation and completion, including two reopen controls. Cases
  also exercise main-only reader refusal, substituted files, missing or damaged
  evidence, changed directories, links, both WAL paths present or absent, and
  identical main bytes that cannot identify the active file. Package `restore.rs`
  exercises successful Failed and NoCollection restores with a nonempty WAL,
  byte-exact safety copies, failure before activation and rollback on cancel.
  These cases extend the existing recovery harness.
- Maintenance's evaluation closure returns directly. Rewrote the full prose
  region preceding the Cancelled Wake helper; assertions/code there are intact.
- New raw-boundary fault points: `replace.wal.after_authority`,
  `replace.wal.after_retire`, `replace.wal.after_retired_record`,
  `replace.wal.before_restore_move`, `replace.wal.after_restore_move`,
  `replace.wal.after_resolution`, `storage.recovery_move.before_directory_sync`;
  restore result hook: `restore.before_maintenance_reevaluation`.
- All new/extended tests are source only. No jobs, tests, builds, network,
  forbidden sources/history or commits. No Unit trailer or U127 acceptance claim.
  Root still owns compilation, formatting (rustfmt is absent here), focused
  execution including every previous passing case and six prepared scenarios,
  native durability evidence, faultpoint catalog integration, the real 150k Quit
  budget, provenance/privacy/similarity, full gates and independent R6. No absent
  normative contract remains identified; the expanded recovery matrix is unrun.
- Source consistency receipt: PASS, one scoped `git diff --check` plus textual
  whitespace/API-wiring assertions across the 16 affected paths, including the
  new recovery module. This is not compilation, formatting or test evidence.
