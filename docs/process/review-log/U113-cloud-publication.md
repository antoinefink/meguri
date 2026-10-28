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
