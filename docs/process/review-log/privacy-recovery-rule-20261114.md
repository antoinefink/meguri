# Exact recovery-rule guard — 2026-11-14

Base: `5b6572e2`. Owner decision:
`recovery-rule-publication-approval-20261114.md`.
One approved literal was added to the scanner. Its hash pins the bytes.
All other matches still use the existing checks. The change adds no configurable
list and exempts no file or individual word.

Filtered author: `cleanroom/20261114T125437Z-76713`.
Independent review `125747Z-79920` found P1: copied fragments in tests could
become public through committed learning. Fix `130407Z-86318` derives its case
data from the approved literal and tests committed learning explicitly.
Independent verification `130734Z-89325` found no P0/P1/P2; its export matches
its input. Integration then renamed one test method to describe its purpose;
its body and assertions stayed unchanged.

Final checks on that exact source:

- `selftest/20261114T130944Z-93815`: scanner unit suite passes.
- `private/20261114T130928Z-93436`: source and contract-review record pass.

The earlier failed privacy runs `125853Z-81772` and `130755Z-90522` remain.
No private pattern was inspected and no additional exception was introduced.
The batch gate remains pending; this record supplies no sync or Unit acceptance.
