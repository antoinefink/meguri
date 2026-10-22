# RF2: safe local collection entry

Starting commit: `81c207573c20c337c8834e4ae8b4ee8f29603dbf`.
Authority: Antoine's review-first ordering, recorded in
`docs/process/review-first-delivery.md`. RF1's full gate and catch-up passed.

This delivery adds supported local collection entry to the real review app.
Start with latest-format, collection-only `.colpkg`. Preserve the source,
inspect a staging copy, verify it and activate it through the planned replace
primitive. Existing local data requires a real safety backup before replacement.
Unsupported formats and media payloads must receive an explicit refusal.
No Apple account or network connection is required for this local path.

Reuse the prepared U124a package and replacement specifications and their
original provenance. Reconcile the obsolete replacement recovery section with
the current persistent generation/account publication fence before implementation.
Reuse the relevant prepared backup specifications for the mandatory safety path.
Keep the full U124a, U126, U127 and U166 acceptance requirements open until met.

One integrated delivery item: package and replacement foundations, real safety
backup, job/FFI service, security-scoped picker and persistent imported reviews.
Component commits retain focused checks and clean-room provenance. The final
Swift/FFI integration gate uses the starting commit above. The spec writer,
filtered implementer, independent reviewer and fixer sequence remains mandatory.
Review covers correctness, plan conformance, clean-room boundaries and UI quality.
No Unit trailer or completed implementation is claimed at this starting record.

## Specification intake

Prepared work was integrated serially on master, preserving original authors,
Role/Spec/Session trailers and coauthors, with Prepared-Commit and Codex attribution.
No new branches or worktrees were created. No production implementation was changed.

| Prepared commit | Master commit | Content |
|---|---|---|
| 0fe35ed7 | dd16edae | U124a original spec session |
| c2878318 | 004b56c5 | Container, inspect and verified-replace contract |
| 615937e9 | 0b1c12d1 | Original correction session and incidental earlier oracle session metadata |
| c862a4bf | 0533767e | U32 staging, atomic-file, sidecar and reopen corrections |
| 6c32c73e | 92d5df89 | U124a final spec session |
| dc285e06 | d9d08175 | Inspection-order clarification |
| 59b8bc62 | 3b12b788 | U126 initial spec session |
| 6eb8fdc3 | b615474c | Real backup writer, names, retention and listing contracts |
| 5da94d48 | c8e833f3 | U126 final spec session |
| caa01955 | 08f47084 | Prepared backup evidence status |
| d8b6902e | 0b77efb3 | U127 initial spec session |
| 6b85adf7 | c15b8e44 | Safety backup and trigger contracts |
| 8c3f45ed | 923d5dd5 | U127 final spec session |
| e74b98ab | f74d796c | Prepared trigger evidence clarification |

The original oracle session s-20261017T190507Z-c6475cb1 is already represented
through 0b1c12d1. Keep the ledger append-only; do not duplicate that record.
Other oracle sessions, implementation, fixtures and goldens remain deferred.
Merge commits were not replayed; conflicts retained current interfaces and harvest obligations.

The package area still has `active: []`. Prepared scenarios added: collection-package
10, backups 13, backup-triggers 6. No golden is yet integrated on master.
Full retention and periodic backup implementations are outside RF2's critical path.

Privacy-hook conflicts required complete neutral prose replacements. A synthetic
backup case now has id/path `package/backups/period-boundaries`; a static comparison
proved only its id changed, with all values and assertions retained.
`package/backups/name-variants` has an equivalent new description.
The serial oracle phase must regenerate those two cases through tools/mg;
never hand-edit golden metadata. Preserve the other prepared fixture/golden bytes.
Automatic approval initially rejected committing the id change without its evidence.
The explicitly authorized retry included static scope proof and deferred oracle
regeneration; approval then permitted it. No hook or gate was bypassed.

## RF2 neutral contract

New session: s-20261022T144230Z-5dc66097 (record commit 6b53010a).
`engine/collection-replace#R6.1–R6.4` defines persistent publication identity,
account binding, file-identity recovery and a durable activation boundary.
Missing or ambiguous recovery state keeps publication paused, with local review allowed.
Legacy manual-transfer bookkeeping cannot clear the cloud publication decision.
`docs/process/review-first-entry-contract.md` defines RF2.1–RF2.5:
supported local input, security-scoped copy, inspect, real safety backup, guarded
activation, job/UI integration and acceptance. R7 stamps are applied once for U166.

No builds, tests or gates ran in this spec-writer phase. No full unit is complete.
Remaining verification: oracle integration,
required independent review/fix sequence, focused checks and full RF2 integration gate.
U124a/U126/U127/U166 requirements outside this local path remain open.

## Oracle intake and focused verification

Oracle integration base: `d57529365c2b22612dc2cb24d0b84bb51a0f9740`.
New session: `s-20261022T150435Z-fc55a4bd`, recorded in `5c86f599`.
The first session job ended with a session-lock error because the sandbox could
not write Git metadata; the same authorized command passed with filesystem access.

| Prepared commit | Master commit | Content |
|---|---|---|
| 67978b5c | 60bac8dd | Original U124a oracle session |
| 40b9768f | 91cf1f43 | Package oracle support, fixtures and ten goldens |
| be55016e | 1ce9e55f | Original U126 oracle session |
| 1991f850 | a1d9e862 | Backup oracle support and thirteen goldens |
| c5aa0c27 | 59dfbba1 | Original U127 oracle session |
| 75d938ad | eb170105 | Trigger oracle support and six goldens |

Original authors, sessions, specification trailers and coauthors are retained,
with Prepared-Commit and Codex attribution. All four original oracle session
records occur exactly once. No old branch tip was merged. Registration and
AGENTS conflicts retain current conventions and the prepared additions.

The 14 package fixtures and 27 unchanged goldens retain the
prepared bytes. Only `package/backups/period-boundaries` and
`package/backups/name-variants` were recorded again through `tools/mg oracle record`.
Their observations equal the prepared observations. Their metadata was not
edited by hand. The byte/hash comparison is in
`build/rf2-oracle-intake/preservation.json`.

The unchanged privacy hook rejected new oracle-local identifiers and prose.
Owned lines were replaced with neutral equivalent local names or prose; no
scenario inputs, assertions or runtime rules changed. Automatic approval rejected
one commit attempt until the unchanged scanner and static origin comparison
provided concrete evidence. One evidenced retry then passed. Hooks and their
matching rules were never changed or bypassed. No private collection was read.

Focused receipts (all paths below are under `build/mg/`):

| Job | Result | Receipt |
|---|---|---|
| `oracle test package` | pass, 18/18 | `oracle/20261022T150634Z-33640/result.json` |
| Two safe-case golden recordings | pass, 2/2 | `oracle/20261022T150735Z-34832/result.json` |
| `oracle test backups` | pass, 20/20 | `oracle/20261022T150840Z-36438/result.json` |
| `oracle test backup` on final driver | pass, 28/28 | `oracle/20261022T150935Z-37942/result.json` |
| `oracle compare package` on final driver | pass, 29/29 | `oracle/20261022T150944Z-38617/result.json` |
| Package privacy scan | pass, 39 files | `private/20261022T150627Z-33365/result.json` |
| Backup privacy scan after neutral replacements | pass, 18 files | `private/20261022T150833Z-36149/result.json` |
| Trigger privacy scan | pass, 10 files | `private/20261022T150920Z-37581/result.json` |

The package area remains `active: []`. The oracle replay proves the reference
evidence, not a working Meguri importer or backup provider. No core, app or runner
code changed. No full gate ran in this phase.

Remaining shared testkit work includes the typed collection-package forms,
`package_listing`, backup operations and observations, and their schema/model
agreement. Freshness validation covers inactive scenarios too, so valid parsing,
fresh goldens and oracle replay remain required. Missing executor capabilities
may stay pending while inactive; adding those contracts does not require RF2 to
implement full retention or periodic scheduling. Keep those full parent-unit
requirements open and implement RF2's actual writer and pre-import safety path.
