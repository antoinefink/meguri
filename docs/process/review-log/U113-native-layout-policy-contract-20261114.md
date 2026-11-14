# U113 recovery contract — 2026-11-14

Round 3; base `270d9af31e233f6ff8d3929b2fe42abea89357b9`.
Author: spec-writer `s-20261114T082100Z-c1738ce6`.
Independent reviewer: spec-writer `s-20261114T083432Z-b1d738f4`.

The review found no P0/P1 in K13.4, V13.6 and their references in
S2.5, V2.1, V17.6, P14.4 and P17. These additions are approved under R2066.
Other sections retain their existing status.

The contract follows the recorded product decision: recover automatically when
safe; otherwise offer the existing local or iCloud collection choice. It keeps
checked live layouts after a tracking failure, requires WAL coverage before
admission, and preserves work until the chosen replacement succeeds. It adds no
native replay ledger. The local-only command and live-head holder still need
implementation; the review incorporated both clarifications.

The reviewer also checked the complete prose revision against the frozen input.
Final patch SHA-256:
`e05272ceb8e68bb7633e98b331630f495d205e5c1b7122d052bfcf461d711f7c`.
Retained report: `build/cloud-resume-diagnosis/U113-r3-contract-review-20261114.md`.

The exact public-rule approval is recorded separately in
`recovery-rule-publication-approval-20261114.md`. This contract review supplies
no implementation, focused-check, Unit, P24, gate or release acceptance.

## Narrower retention amendment

Antoine approved the fallback-first alternative and rounds 5–6 in
`cloud-recovery-fallback-approval-20261114.md` (`b71acf82`). This supersedes
only the mandatory new asynchronous holder in K13.4 and its S2.5 reference.
Available valid checked authority remains automatic. A validated missing map
uses the existing collection choice, including in the same process.

The same author and independent reviewer checked the minimal publication
patch: `40654261089da2087de8d4f8aa15d179b180ba2a5f1b7ae5f92f4787a481fafe`.
The reviewer found no P0/P1; the original Durable admission paragraph changes
only its live-holder sentence. WAL coverage, ownership, error classification,
replacement safeguards and performance limits remain.

The installed patch adds only the scoped approval labels to that text:
`661336a70f3bdf0d3a208a3723a7bd9a2559a9f82a217ca113591dbb06328357`.
K13.4's retention variation, that single admission sentence and S2.5's reference
are Approved 2026-11-14 (R2066). Other status scopes remain unchanged.
The retained review is
`build/cloud-resume-diagnosis/U113-fallback-first-reassessment-review-20261114.md`,
section “Owner-authorized minimal publication text.” Publication preflight
`private/20261114T143456Z-56499` passed before the label-only edits.

This approves the contract, not source, tests, Unit/P24/gate or release acceptance.
