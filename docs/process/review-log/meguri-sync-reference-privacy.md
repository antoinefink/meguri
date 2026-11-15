# Reference harness privacy review

The staged reference integration scan reported 12 files at
`build/mg/private/20261115T161925Z-65580/result.json`.
An independent oracle-maintainer reviewed all 12 files and their complete hashes.
The files contain original tooling, synthetic observations and development evidence.
No actual collection or private token spelling was read during that review.

Under the owner's delegated maintenance authority, the scanner records their
exact repository paths and complete SHA-256 hashes. The original source commits
remain unchanged. Edited bytes or identical content at another path remain scanned.
The existing synthetic regressions cover mutations and index/disk disagreement.
The original review-note grant is unchanged. No production path was added.

Detailed independent receipt: `/private/tmp/meguri-sync-U3-privacy-review.md`.
Registry and dry-run checks passed two cases at
`build/mg/selftest/20261115T162003Z-67719/result.json`.
Static checks passed at `build/mg/static-checks/20261115T161925Z-65583/result.json`.
The similarity wrapper check passed with no supported source files in scope at
`build/mg/similarity/20261115T162007Z-68081/result.json`;
reference code remains quarantined and excluded by the existing scan scope.
Updated integration gates remain required.
