# Prior-owner intents during cloud adoption

Status: WIP; no unit acceptance.
Unit start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Component start: a6cadda2.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Fixer: s-20261103T133750Z-f276f58e.

Export17753 adds four real-driver adoption cases: media installation and name
resolution, each through completion and Ready-boundary process death. The
The testkit creates the I/O failure and intent, then publishes another epoch.
The checks cover bounded pages, backup rows, media trash and reopen. Production
is unchanged; the new seed helper preserves default callers.

Independent review finds two P2 test errors. Export26237 keeps the old name at
the I/O fault and requires the new name at Ready and reopen. It also requires
both REMOTE and LOCAL bytes in trash after scan and reopen. Review is clear.
Core31327 passes all five target checks; core-lint32716 passes.

Private32129 flags two unpublished test identifiers. Export39988 renames only
those identifiers. Review confirms unchanged bodies and coverage. Core43932
passes all five checks with target-based selection; privacy also passes.

Together with Core95005, these tests cover the publication and adoption
prior-owner intent subproofs of P24/Q11 row495. They do not close the entire row:
preview wording, wider cleanup/crash cases and actual SDK evidence remain.
