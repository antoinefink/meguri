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
