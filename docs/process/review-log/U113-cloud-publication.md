# U113 publication: tests first

Status: enabled RED; no Unit trailer or P24 acceptance.
Phase start: `efcf26cabdcb42653e6adff2811249aff1d97340`.
Tests applied after: `6fce8827`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Nine tests plus the child entry require the real CoreHost and shared CloudKit
service. No prebound fixture or direct tracking seed is used. They cover read-only
discovery, explicit publication, all-kind scan and real acknowledgements, own
control operations, conditional Active switch, lost results, retained epochs,
restart/cancel authority, other builds, prior intents, backup provenance and
independent full-width key bytes. Media and adoption dependencies stay enabled.

Author: `cleanroom/20261028T183320Z-98685`.
R1 found three P2s: typed codec use, the wrong backup artifact in one check,
and a synthetic baseline labelled as the main start. Repair
`cleanroom/20261028T185127Z-12692` fixes them; independent R2 is clear.
`cleanroom/20261028T202850Z-35755` removes only flagged comments and one
engine-test diagnostic. Exact-delta review is clear; no assertion is weakened.

Observed before production application:

- `core-format/20261028T203154Z-37124`: pass.
- `core-test/20261028T203355Z-38732`: expected RED. Only two unresolved
  import groups: platform key-byte inputs and the publication coordinator
  harness. Zero tests run; this is not a pass.
- `private/20261028T203326Z-38232`: both contract-test targets pass.
- `similarity/20261028T203324Z-37793`: both targets pass.

The named P24/Q11 checks precede the next production build.
P24 remains open. No Unit trailer is added.

### Host regressions before the production phase

Phase start: `f94238ea`. Production export
`cleanroom/20261029T053520Z-6982` is partial and remains unapplied.
R1 found three P1s: startup account authority, non-atomic parent references and
an unrestricted engine decision write. Repair `cleanroom/20261029T060815Z-33924`
closes those in source. R2 still requires Cancel after account/file changes and
the decision fault-point repair. Production is not accepted or built.

Only the new host test target is applied here. R2 independently clears it as
enabled regression evidence using real CoreHost, Service results and crash
workers. It covers full-width key failure, actual initial publication, unfinished
adoption account checks, decision authority/restart, stale submission, independent
parent failure and lost Active replies. These tests supplement the original
nine contracts; they do not replace or disable them.

- `core-format/20261029T062458Z-44066`: pass.
- `core-test/20261029T062631Z-45013`: expected RED, two missing import groups
  for production publication APIs and the transport helpers. Zero tests run.
- `private/20261029T062634Z-45272`: pass.
- `similarity/20261029T062636Z-45561`: one file, zero findings or exemptions.

The next repair updates the test crash-point names and adds cancellation cases
before production application. Reset publication stays unavailable before writes
until its required tests and implementation land. Adoption, media, native SDK
wiring and the full workflow remain required. The batch retains its original base.
