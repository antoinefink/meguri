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
