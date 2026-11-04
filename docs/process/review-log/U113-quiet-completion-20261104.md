# U113 overlapping quiet completions

Status: WIP; no unit acceptance.
Unit start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Component start: cacb11cddb8a37993dfe683f753fc18e3368dfd4.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Fixer: s-20261103T133750Z-f276f58e.

The signed Mac run reports UnqualifiedCompletion at RecordQuietPoint before
later stale-command failures. The driver holds one qualification tuple; a new
candidate replaces an older one that can still be waiting to execute.

Export87429 adds four real-drain regressions. Core92503 fails all four at the
expected UnqualifiedCompletion. Existing guards pass before execution. The
tests cover both completion orders, new local work and an undrained fetch.

Export597 gives each command its own proof. Copies use the same state; two
commands stay distinct, including at the same timestamp. The driver records
issuance order only after a successful write. An older command cannot overwrite
a later recorded drain. Successful replay behavior and existing eligibility,
owner, cancellation and stale-command checks remain intact.

Independent review is clear. The shared projection path checks cancellation
before publishing the older command's observation. P14.3 is outside the listed
P24 open items; this change closes none of those items.

Core9089: 11 pass, including equal timestamps, backward clocks and copied commands
and the existing unqualified-completion checks. Format7613 passes. Export28966
only renames one test and removes one comment after private10510 flags them.
Private32921 and similarity33174 pass. Lint31942 passes. Core34627: 3 pass,
covering the renamed test, old owners and stale generations. Native builds,
the live workflow and the full batch gate remain required.
