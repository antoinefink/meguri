# Media observation foundation — 2026-11-09

- Implementer session `s-20261109T094715Z-c7c7c45e`; supplied start base
  `6ff93f53`, original U117 `f7a83f2b`, original batch base
  `1416a520542a8c5daa38a23bfe398da704c3d6c9`. Root start record:
  `build/unit-intake/U117-cloud-fake/media-observation-start-20261109.json`
  (not present in this filtered source view).
- Read implementer/cloud AGENTS and approved
  `conformance/cloudkit-acceptance#Q5`, `media/media-files#M1,M11,M16`,
  `media/media-journal#J1,J2,J6`, `sync/policy#P3,P14.4`.
- Added retained-Host observation only: complete journal names/facts, paged
  Runtime media entity metadata, hashed binding/account/scope/epoch identity,
  manual pending state kept separate from CloudKit dirty marks, and actual
  regular-file SHA-256/SHA-1/byte counts through production `open_file`.
  Byte reads use a 64 KiB buffer and the production 100 MiB file bound plus
  one detection byte. No file-count/aggregate inspect cap, payload retention,
  service inspection, raw-name output, database seeding or new core API.
- The eight `cloud_media_checks` tests specify observations of synthetic
  collections and production media writes using independently computed hashes.
  Assertions include byte edits that retain size, mtime and journal metadata;
  absent files; symlink and replaced-root refusals; enumeration of 2055 names;
  bounded reads; sparse files exceeding the size limit; closed owners; an
  unbound Runtime; and diagnostics/serialization without fixture text. These
  tests do not use the child driver and have not been executed in this session.
- Missing proofs stay typed: Runtime missing-name details, Runtime ledger name
  dispositions (withheld/superseded/file_missing and policy pending scope),
  install/trash history, and an original-root-pinned read descriptor. The public
  file reader guards terminal/root symlinks but cannot prove original directory
  ownership across concurrent ancestor/root swaps; capture/rechecks reject
  observed root substitutions only. Construct before fault mutations and call
  at an idle device barrier; these separate reads are not an atomic snapshot.
- Runtime entity paging is implemented, but its bound-device branch has no
  new execution evidence. The new tests cover journal/file evidence;
  parent integration must supply actual bound Runtime and missing-list proof.
- No builds/tests run in this filtered session. Root owns focused Docker checks,
  independent review, provenance/privacy/similarity and the original batch gate.
  No Q5 status, install safety, convergence or acceptance result is asserted.
- One source consistency check passed: assigned-file whitespace/final newlines
  and exactly one `pub mod media_observation;` declaration. No compile result.
- Parallel checker/driver/test files and all previous logs remain untouched.

## Focused integration evidence

Core `20261109T203114Z-84705` passes all eight tests with zero skips.
Filtered `83082` imports the correct SHA-1 trait without changing dependencies,
hash algorithms, test bodies or observation safeguards. Independent delta review
is clear. Lint `20261109T203235Z-85278` passes all four checks; privacy and
similarity checks also pass. Runtime-bound paging and all missing full-Q5
requirements remain open, together with the original integration gate.

## Bound Runtime witness — source handoff

- Implementer `s-20261109T094715Z-c7c7c45e`, supplied base `ad2166e3`,
  media component `2c7e2b83`, original unit `f7a83f2b`, original batch
  `1416a520542a8c5daa38a23bfe398da704c3d6c9`. Supplied root start record:
  `build/unit-intake/U117-cloud-fake/media-bound-start-20261109.json`
  (absent from this filtered own-source checkout).
- Added one behavior case in `cloud_media_checks`:
  `published_and_adopted_bound_runtime_pages_preserve_actual_media_facts`.
  Its `cloud_child_entry` is required subprocess IPC infrastructure only.
  The eight prior bodies, assertions and budgets are unchanged.
- Two generated collections begin unbound. Real publication precedes 251
  production exact-name writes (1,004 bytes total, four distinct contents).
  Existing SDK callbacks drive the upload and real adoption, each with the
  existing 10,000-step bound, no sleeps or retries. Service pages are 32,
  matching native adoption limits; the generic default remains unchanged.
- After clean child closure, each actual Host reopens, verifies identity and
  reconciles through the public Runtime. Direct entity queries require pages
  of 250, 1 and EOF. Ordinary Service reads compare each entity's opaque ACK
  envelope and system fields with the actual published record. The observer
  must retain the real binding/epoch/generation, verified usable state, complete
  hashed name sets and content-hash counts. Each entity is joined to actual
  file SHA-256/SHA-1/size, independently checked against the tiny written bytes;
  both Hosts must hold identical observed file facts. Repeated observations
  preserve binding, generation, Runtime counter, journal and tracked names.
- No observer/core/driver/checker/API behavior or Swift/tooling changed. Missing
  Runtime name lists and dispositions, original-root-pinned reads, and
  install/trash history remain typed missing proof. These idle, separate reads
  are not an atomic snapshot. Whole Q5 convergence, P24 closure and acceptance
  remain incomplete; this witness makes none of those claims.
- No builds or tests ran here. Root owns the new-case focused run, independent
  review and provenance/privacy/similarity guards, plus the original batch gate.
  Selector: package `meguri-testkit`, target `cloud_media_checks`, exact case
  `published_and_adopted_bound_runtime_pages_preserve_actual_media_facts`.
- Source consistency check passed: the eight prior bodies and budgets match
  the filtered baseline exactly; one behavior case and its IPC entry are added.
  Assigned-file whitespace and final newlines were checked; no compile result.

Focused execution `20261109T204610Z-91925` compiled and ran the new case.
It failed at the reopened Host readiness assertion before observer checks.
Independent source review is clear; execution proof remains open. No assertion
or readiness condition has been relaxed.
