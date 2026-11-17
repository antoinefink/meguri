# Current client evidence and remaining acceptance

U11 has actual Desktop31, browser10 and independently reviewed retained8 results.
Actual Mobile10, Main U11 integration and the final coherent-batch gate remain
incomplete. These results grant no complete U11, app or full-goal acceptance.

Authority: `docs/plans/2026-11-15-1028-feat-meguri-sync-server-plan.md`, U11;
neutral C11/C12, K6/K7 and local-delivery L1–L7. Original coherent-batch base:
`480df4526514e294f5a3a16dd4ab83a1fbcc7f47`. It remains the pending gate base.
U9, R17, F5, AE8, backup/restore and production deployment remain deferred.
The original broad catalog remains research scope, not completed compatibility.

## Source and evidence boundaries

Unit starts and role sessions remain as originally recorded in
`conformance/peers/retained-sync/README.md` and the committed session ledger.
The worker's recorded fixer input is `b3d271decd94a230579bc4418a85439a3b3e7358`;
the runtime's recorded start is `81c99a427b025497bffc06acec0fb496f97231b6`.
This log does not invent a new start or replace prior commits and failure logs.
Original worker author: `s-20261116T225101Z-bbe6c19b`; runtime author:
`s-20261116T225100Z-296ac8bc`; bounded worker fixer:
`s-20261117T004348Z-2115dc91`.

The checked public template API is separately owned. Its four-file patch is
verified byte-identical in Main `f4770d47eab63e8965e5514a01f052ef653a9fde`
and root cherry-pick `106aa7f5877086f458eeab3a37bc3e9fbfc451b7`, with 22
focused note-type cases passing. U11 adds no other app or Core provider work.

Receipts below are relative to `/private/tmp/meguri-sync-local-docker-20261116/`.
Each source identity belongs only to its own run.

| Evidence | Actual source | Result and receipt |
| --- | --- | --- |
| Desktop backend | clean `d4faec349ed27d383f47b853f613f24fb7e1a9bc` | 31/31 passed; `build/mg/sync-server-interop/20261117T075652Z-6001/result.json` |
| Browser | same clean `d4faec…` | 10/10 passed: nine Node cases and one Chromium case; `build/mg/sync-web-verify/20261117T075919Z-25851/result.json` |
| Retained peer build | clean `224f7d8e03fcee5a7593bdb2e70b935442010d78` | passed; `build/mg/sync-server-retained/20261117T162921Z-88288/result.json` |
| Retained package | same clean `224f7d…` | passed; `build/mg/sync-server-package/20261117T162935Z-89814/result.json` |
| Actual retained HTTP lane | same clean `224f7d…` | selected/completed/matched8, different0, unavailable0; `build/mg/sync-server-interop/20261117T163339Z-10727/result.json` |
| SDK regression | dirty `6dd9084263fc5cd15c3dee23303247b11f7bd2fd` | 84/84 passed; `build/mg/sync-server-reference/20261117T161005Z-41131/result.json` |
| Fixed-fixture FSRS Rust units | clean `53dcc5261ae3e5d31840b15ad5fdcf4f95a81387` | 4/4 passed; `build/mg/sync-server-retained/20261117T134907Z-45770/result.json` |

Desktop and browser used runtime image
`sha256:15ad92b4478c651b0f79de2fa5dddbfb6655cd4bac7009af0b0a2261c1c145ca`.
Their independent artifact review is
`/private/tmp/meguri-sync-u11-packaged-desktop-browser-artifact-review-20261117.md`,
SHA256 `97204f71db1e1528f1b2aa6a8e4bfc23c645f242d3f965ef40f95a7583286faa`.
These results remain evidence for `d4faec…`; they are not relabeled as `224f7d…`.

The retained package used runtime image
`sha256:1992121c312801d709818dfbf7113c7bcddbc16d456bd24e5fff39e6f141f7f9`.
Its peer executable SHA256 is
`e7e88f073e0da90b841973f26c0f12152bbf0a5499de64afc545104312a2b094`.
Independent actual-artifact review is **CLEAR** for this exact source and run:
`build/final-client-unit/retained-eight-independent-actual-review-20261117.json`,
SHA256 `b29e06157f2a04c0e6d66c19a221130b7446e93ad6ba25ac456e8dc9cd4d7502`.
Its eleven subreports cover all eight cases. Wire bodies are absent; checked
methods, statuses, byte counts and completion counters remain. Receiver cursor
freshness follows fresh-profile semantics, unused mounts, empty before-state and
receive-only wire. Media-only comparisons preserve the client's own twelve
tables, without equating unrelated collection identities. Actual reviewed
ownership/removal/absence helpers establish cleanup; no new live Docker
inspection was performed. This is not a compatibility seal, Mobile result,
complete-catalog result, full gate or complete-unit acceptance.

The SDK84 run is truthfully dirty. Its five SDK input files are verified
byte-identical to final `224f7d…`; this does not change its recorded source identity.
The four Rust unit tests are actual execution, separate from build-only and
Python/Bats checks. Their Rust source is unchanged in the later retained source.

## Focused checks and preserved failures

Parent records 32 focused guard, digest, Docker-context and scanner cases passing.
The ordinary privacy, static and similarity checks also passed; similarity used
the original `480df452…` base. Retained logs:

- `build/final-client-unit/reverse-baseline-guards-selftest-20261117.log`
- `build/final-client-unit/reverse-baseline-guards-private-20261117.log`
- `build/final-client-unit/reverse-baseline-guards-static-20261117.log`
- `build/final-client-unit/reverse-baseline-guards-similarity-20261117.log`

Existing U1–U8 evidence includes the previously accepted 188 server tests and
full gate. Those proofs remain reusable where their assertions apply. They are
not a newly executed full suite on `224f7d…`, or a replacement for the final gate.

All actual failures remain unchanged. Bounded repairs retained the original
eight cases, whole-row/history/media comparisons and real public operations:

| Finding and repair | Retained evidence |
| --- | --- |
| Fixture reader lease and capture ordering: release readers before mutation; freeze successful immutable outputs before later measurements. | Original run `build/mg/sync-server-interop/20261117T095439Z-74345/`; runtime closure review `/private/tmp/meguri-sync-u11-retained-runtime-closure-review-20261117.md`; baseline closure receipt `/private/tmp/meguri-sync-retained-baseline-repaired-closure-receipt-20261117.json`. |
| No-change setup required one declared ordinary retained settlement after Desktop settlement, before the measured baseline. NoChanges, exact-state and zero-transfer checks remain. | Original no-change mismatch in `20261117T095439Z-74345`; `/private/tmp/meguri-sync-retained-baseline-repaired-closure-receipt-20261117.json`. That run had six completed cases: five matched, one differed, two were unavailable. |
| Ordinary review refused because the imported fixture had not acknowledged the FSRS policy. One public enable operation now precedes rich mutations. The fixed three-new-card/no-history proof permits no card/history changes during enablement. | Actual failures `20261117T115059Z-87146` and `20261117T122804Z-88488`; `/private/tmp/meguri-sync-retained-fsrs-fixed-review-receipt-20261117.json`; actual four Rust units above. |
| Review statistics must change only the reviewed card's original home deck and exact original-name ancestors, with actual queue/time values. Siblings, defaults and unknown bytes still fail closed. | `/private/tmp/meguri-sync-retained-home-stats-closure-review-receipt-20261117.json`; complete strict positive/negative regression remains enabled. |
| Reverse admission used a valid incoming baseline but compared it with the wrong expected slot. The repair selects the correct baseline context and preserves exact equality. | `build/final-client-unit/retained-reverse-refusal-boundary-20261117.json`, intended old-guard red `reverse-baseline-context-red-20261117.log` (160806Z-32024), `reverse-baseline-full-application-20261117.json` and `reverse-baseline-green-20261117.log`. The separate `reverse-baseline-original-red-20261117.log` (155414Z-2413) failed setup on a missing selection input before the target guard; it is not that regression's red. |

The retained-selection image omission and its meaningful digest-test omission
were also fixed through separate filtered author/review runs. Final complete4
review receipt: `/private/tmp/meguri-sync-retained-selection-digest-test-review-receipt-20261117.json`,
SHA256 `e9ac2df2a819143bec1faf2e04b6a0f4d7cde7dd89661b26607e4ec3828b2ab0`.
It binds the exact public COPY, deny-all exception, digest input, and existing
mutation/restoration test. It supplies source-review credit only.

R31 ordinary measurements are source-specific to clean `d4faec…` and image
`15ad92…`, in its `segments/final-{full,normal,media}/candidate-observations.json`.
Collection no-change made one metadata request: 2ms, 45 request/126 response bytes.
One review made seven ordinary requests: 24ms summed request time, 1100/334 bytes.
A 135168-byte upload took 26ms; media mutations made 46 requests: 218ms, 2330/2572
bytes. The envelope records Linux/aarch64, 14 CPUs, cgroup limits/current memory
and client CPU/RSS. These are request observations, not whole-sync latency or
server resource peaks. They establish no workload-capacity or benchmark claim.

## Separate package and device evidence

Main U10 package regression passed 4/4 on clean
`c145b92861fd67799cbb852cc7b74ed2a78d8136`:
`/Users/antoine/code/meguri/build/mg/sync-server-package-test/20261117T154144Z-246/result.json`.
Its tracked U10 update is `989b86c37735728483577ea61a9864f1427a8083`. Main's
MIT-era package evidence is distinct from root `224f7d…`, whose core/model
metadata and notices still retain MPL-2.0. Neither receipt changes the other's
license facts or tested source, and neither completes U11.

The owner reports successful login and card, review and image transfer between
the Test profiles iPhoneTest and Mac MeguriTest. Their contents were not read.
This is basic smoke evidence only. Exact installed AnkiMobile and iOS builds
remain unconfirmed; no formal Mobile10
producer, complete case receipts or independent final device review is claimed.
The live endpoint remains frozen at `9b79d2bc7e3e1b7ce15f21e686f09aaade7204d5`.
This log contains no credentials, personal-profile content, image bytes or
private device data.

## Remaining delivery conditions

- Produce and independently review actual exact-build Mobile10 evidence under
  the frozen selection, including ordinary bidirectional collection/media and
  replacement flows. Basic owner smoke does not replace these cases.
- Close the current-scope requirement-to-proof mapping, including useful existing
  security/fault/resource proofs and the source-specific R31 measurements above.
- Resolve exact final-source/image applicability across the distinct evidence
  sets, complete required provenance/privacy/similarity checks, and pass the
  coherent U10–U11 gate from original base `480df452…`.
- Integrate the reviewed work onto `master` without a PR, preserving individual
  commits and failures. Current app acceptance, original app milestones and
  full-goal closure remain separate and incomplete.
