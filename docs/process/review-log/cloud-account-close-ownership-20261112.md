# Account-close ownership repair — 2026-11-12

Role: fixer, implementer view. Supplied starting main commit: `2ce1d0be`.
The complete prepared repair was retained. No history, network, other agents,
forbidden sources or private scan data were consulted.

The supplied trace supports the three observed `retire=true` Mixed failure rows:
intentional close preceded original-task cancellation and nonadmission with
matching backend, generation and revision; unqualified cancellation then made
failure sticky before cleanup. It does not establish that sequence for all six
Mixed rows. The prepared repair preserves provider/fence provenance, binds
refusal to its original preparation and records first cancellation ownership.

Two P1 corrections in this pass:

- Provider launch, installation and waiting now run inside the existing task
  cancellation handler. The first-result latch and post-install cancellation
  handling remain; an earlier provider error still wins over later cancellation.
- Both fallback refusal guards return the caught resolver error with the
  accumulated outputs when acceptance or admission fails. Authority checks stay
  in place and stop subsequent Core/adapter work on those returns.

New deterministic coverage pauses the owner between provider launch and install,
orders cancellation and actual detached failure both ways (including provider
`CancellationError`), advances actual Core authority to reject fallback while
local admission remains valid, closes during fallback result delivery, and
closes during setup-failure projection. Two internal scheduling seams observe
launch and the actual fallback result; neither supplies Core authority/results.
The flagged projection-test comment was rewritten. Existing tests, predicates,
limits and privacy guard remain enabled and unchanged. No other TwoFetch work,
policy/schema changes, retries or guard exceptions were added.

Integrator-reported prior focused results: Core4
`swift-test/20261112T112006Z-6882` and App8
`swift-test/20261112T112046Z-11391` passed for the seed. They do not verify these
P1 corrections. This runner performed source consistency review and
`git diff --check` only; no compilation or tests were run here.

Still required from integration: compile and run the new focused cases; retain
Core4/App8 and the unchanged Mixed, Resume, publisher, command-failure,
originalGuardRefusal and SDK checks; independent review and required
provenance/privacy/similarity checks. Other TwoFetch failures remain open.
No full gate, milestone completion or final acceptance is claimed.


Integration checkpoint: new Core8 passed in
`swift-test/20261112T112709Z-25601`; staged-output privacy check passed
`private/20261112T112734Z-27180`; changed-source similarity passed
`similarity/20261112T112805Z-28862`.
Independent review `cleanroom/20261112T112704Z-25201` found two remaining P1
issues: successful-read supersession must retain cancellation semantics, and the
nested fallback catch must preserve the earlier resolver error after lost
admission. They remain open. No unit completion or full gate is claimed.
