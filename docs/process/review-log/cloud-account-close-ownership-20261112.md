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
admission. No unit completion or full gate is claimed.

Follow-up fixer pass, implementer view; supplied main start `9a741f22`:
successful resolutions now return cancellation on lost admission when this
original reader has a newer read ID. Resolver failures and projection failures
without that supersession retain their errors. The outer error is explicitly
bound so the nested setup-fallback catch returns it with accumulated outputs on
lost admission; valid-authority projection failures retain the inner error.
Existing authority checks, effects and task/read ownership wiring are unchanged.

Added deterministic setup-failure coverage: hold the real fallback projection,
advance actual Core read authority, close the original reader before release,
then assert the original setup error and refusal provenance. Existing newer-read
and close/freeze checks are unchanged. Source consistency review and
`git diff --check` only; compilation/tests and independent verification belong
to integration. No retries, guard changes or unrelated TwoFetch work.


## Integration result

Third independent review `cleanroom/20261112T113145Z-37984` found no P0/P1
findings and no P2 findings in the supersession delta. Earlier review covered the
original ownership wiring and first-outcome repair. Final changed-source Core
selection passed all eleven tests in `swift-test/20261112T113151Z-38411`, including
the unchanged formerly failing newer-read test and four invalidation cases.
Final app selection passed all eight tests in `swift-test/20261112T113336Z-42189`,
including the six Mixed cases, two Resume cases, two publisher cases,
the native close check and tests for retained errors.
Final delta guards passed: `similarity/20261112T113340Z-43332`, `static-checks/20261112T113344Z-43906`, `private/20261112T113421Z-45291`.
The earlier full repair source scan passed `similarity/20261112T112805Z-28862`;
the final delta scan covers both Swift files changed afterward. Commit hooks
remain enabled. This closes this bounded account-close repair; other TwoFetch
failures and integration batch base `1416a520542a8c5daa38a23bfe398da704c3d6c9`
remain open. No Unit trailer, full gate, milestone or release acceptance claim.

A separate signed Mac preview built from reviewed clean `2ce1d0be` passed
`build-app/20261112T112313Z-17235` and was reopened by exact product path.
It includes the prior Resume fix, but not this later account-close repair.
Its retained deck counts were observed; its Needs Attention state remains open.
