# Explicit WAL flush observation repair

Starting commit: `218ab2e2543a2c4822b79c1ac3520230ca3c1998`.
Pending integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The interrupted full gate's core tests reported two lifecycle cases missing
WAL observations after explicit flush. Storage synced, but only automatic
flush recorded the test event. No lost production write was demonstrated.

Filtered R1 repair `cleanroom/20261028T104602Z-28298` moved recording into
coverage and added a CountingVfs check for one event per actual full sync.
Independent R1 review was clear. Focused run
`core-test/20261028T160609Z-93896`: 56 passed, two failed.
P1: event completion preceded the terminal callback; terminal failure was absent.

R2 repair: `cleanroom/20261028T160911Z-95605`.
Coverage, sync count, retry policy and deadlines stay unchanged. Independent R3 review is clear
of P0-P2 findings. Original assertions and the new native-sync test remain.

- `core-test/20261028T161216Z-98601`: all 38 lifecycle/durability tests pass,
  zero skipped, including the four previously failing cases.
- `core-format/20261028T161150Z-98128`: pass.
- `core-lint/20261028T161310Z-99993`: all four checks pass.
- `private/20261028T161219Z-98867`: pass.
- `similarity/20261028T161310Z-130`: final source passes.

The cancelled gate `gate/20261028T103614Z-93058` remains incomplete. Its core
run had 3761 passes, three failures and ten skips; conformance was cancelled
on the user's pause. Focused repairs do not establish a full gate pass. Keep
the original base and all remaining CloudKit and acceptance requirements.
