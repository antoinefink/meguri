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


## Frontend build audit assessment, 2026-11-17

The fixed assessment preserves the raw npm report and records applicability separately.
The report still contains four HIGH findings from GHSA-fc5q-39xc-vjpf and exits 1.
Only the exact reviewed report, graph, copied inputs and build controls can qualify.
Unknown findings, changed inputs, malformed reports and tool errors block the operation.
Dependency versions, Rust advisory rules, notices and zero-retry checks remain unchanged.

The source starts at `8733b6eae233e68fc45feb5f11a1a84a43e4bdf5`.
Validation then moved to `8a667cd2510cf8de9b0404c8b3e0a0660eae3966` with all eight candidate files byte-identical.
The fixed inventory binds all 44 resulting web files and 14 controls.
Reviewer `2b38` read all eight changed files completely; each exact snapshot is PUBLIC.
The author is isolated Codex fixer `s-20261117T004348Z-2115dc91`.
The parent and independent reviewer closed the Docker-context and real-wrapper test findings.

| Exact changed source path | Reviewed SHA-256 |
|---|---|
| `docker/sync-web.Dockerfile.dockerignore` | `d0f1710720bc5b9d16c22f715f1094909a5978401fd22c65ecefcb07e66c60c6` |
| `server/web/DEPENDENCIES.md` | `098b5299fe78468b47d91c44ef75fec8e7bf8a1c41b49a227993f8f652228e94` |
| `server/web/scripts/run.sh` | `45ae2cae47f70726bae42af8aa3d26621353f4ade6dd457d39f19b7b36e64418` |
| `server/web/scripts/audit-applicability.py` | `974bea89755ce61df03b7a8139ec7938cdbabd141339f30ae4904c64b1cfd5ad` |
| `server/web/scripts/audit-assessment.json` | `69fc817296d3e614184f2c00d8a04a5103fcebeaed8aa7a40d225e375fb1014d` |
| `tools/mg.d/sync-web.sh` | `e96c97800779b5d2deae7b36bb0912e43cedf4e8c3b6d40b6c3f81d0206747f1` |
| `tools/tests/meguri_sync.bats` | `28f47c0344b00e5586431b8e2efca2e3f870b650d23ba5f6f268d191c4a9d4b5` |
| `tools/tests/sync_web_applicability_test.py` | `cd3ebde8aee79cf4b16199847f60340dcfe5dd691eff5442a1bac86c2f31ff80` |

Receipt paths in the following table are relative to `build/mg/`; execution rows identify that run's `result.json`.
The complete source review is `build/mg/cleanroom/20261117T195008Z-96757/codex.out`.

| Observed check | Receipt and result |
|---|---|
| Complete source review | `cleanroom/20261117T195008Z-96757`; CLEAR, no P0/P1/P2 findings; all eight files PUBLIC |
| Original finding reconciliation | `/private/tmp/meguri-frontend-assessment-finding-closure-20261117.md`; both closed, SHA `4fef3828adb9871292ba59def00a17a453b9da26fc4dcf37906ff2e6efc617c7` |
| Focused boundary and real-wrapper tests | `selftest/20261117T195110Z-2433`; PASS 3/0/0, including 21 sequential operation scenarios |
| Rebuilt Docker tool image | `sync-web-image/20261117T195247Z-12615`; PASS |
| Actual locked install | `sync-web-deps/20261117T195250Z-12962`; PASS |
| Actual audit | `sync-web-deps/20261117T195314Z-14716`; PASS |
| Actual explicit resolve | `sync-web-deps/20261117T195335Z-15694`; PASS, emitted lock unchanged |
| Actual browser installation | `sync-web-browser-install/20261117T195355Z-21769`; PASS |
| Affected tooling, Docker contexts and scanner tests | `selftest/20261117T200603Z-52773`; PASS 27/0/0 |
| Static checks | `static-checks/20261117T200626Z-53851`; PASS 3 |
| Similarity with the existing reference corpus | `similarity/20261117T201003Z-59809`; PASS 3 supported files, 6 excluded, no allowlist |
| Public source privacy scan | `private/20261117T201432Z-66874`; PASS for the eight source files |

All four actual audits bind image `sha256:c0243b40b0dd7381d6102c32df0b4051beaac5e8d5d39e5179464650306fd124`.
Each retains raw exit 1, HIGH=4, total=4 and report SHA `bbf0761816a86512d0464277dfefbc00ea8ec0827ea61c4604b353aa0fec4f3d`.
Each emits `assessed-inapplicable`; none becomes a clean raw audit or a dependency-remediation claim.
The executed receipts describe an uncommitted candidate, not a committed acceptance gate.

The original audit/package failures remain failed.
The missing-corpus error `similarity/20261117T200606Z-53062` also remains recorded.
The nine-file privacy check `private/20261117T200613Z-53496` flags only new scanner metadata; its resolution remains pending.
A complete independent review classified that scanner PUBLIC and verified exactly five added and three refreshed path/hash entries.
Its matcher, thresholds and bytes outside the map are unchanged.
The clone's token file is shared through a symlink, so it must not run a local refresh.
A coordinated supported shared-baseline operation and ordinary privacy checks remain required before the metadata commit.
Final package/client checks, formal Mobile evidence and the original coherent batch gate remain pending.
