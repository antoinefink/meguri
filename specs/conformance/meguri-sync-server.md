# Meguri Sync contracts and current-client probes

Spec: conformance/meguri-sync-server
Requirements: Meguri Sync U2, U3, U11; R4, R6–R18, R21–R33; F1–F6; AE1–AE8; KTD5–KTD16
Anki: 26.09.3
Scenarios: conformance/scenarios/sync/server/ (planned; no execution claimed)

## C1. Evidence authority and machine format

This is a neutral observation contract, not a reference report or an implementation. The baseline is 26.09.3; supported targets are Desktop 26.09.4 and AnkiMobile 26.10 with exact builds recorded. This unit establishes no observations of either target. **B** is an existing neutral baseline fact; **M** is independently chosen Meguri Sync policy; **P** is an unresolved target observation. Each new contract labels its behaviors; the matrices below enumerate P dependencies. M rules affecting native clients also need compatibility evidence, without being relabeled reference policy.

`conformance/schema/meguri-sync-server.schema.json` describes neutral case/evidence manifests, not executable commands. No source, SQL program, shell, private input, bearer value or raw reference transcript belongs in them. Table IDs below are permanent case-family IDs; each expands into named variants and separate applicable targets. A sealed result identifies its exact client/server lane, sanitized fixture identity, expected/observed values, evidence hashes and owning spec IDs. Unobserved, unavailable, failed, matched and independently sealed results are distinct. Baseline citations are never current-release passes. Schema validation alone cannot establish unique case IDs, count consistency, matching outcomes or artifact/source binding; C11 requires semantic validation in the MG harness before credit.

U3 seals a target clause only after the separate oracle-maintainer supplies observations and independent spec review accepts them. Keep reference source, drivers, raw reports and provenance out of the implementation view. A gap becomes an explicit probe, never a translation of implementation source. Additional actual transport/schema forms require a bounded reviewed neutral clause before compatibility code depends on them.

## C2. Observation protocol

Use synthetic accounts, note types, fields, CSS and media generated for tests; no stock templates/assets or private collections. Identify fixtures by SHA-256, schema and byte/count summary. Use two existing independent clients and a fresh receiver for convergence. Distinguish client-to-reference-server from client-to-Meguri-Sync evidence; Rust-to-Rust or retained-Meguri tests never substitute for an actual target. Record peer roles, ordered actions, baseline expectation, statuses, stamp/table/content observations and restart results. Hash media bytes; counts alone do not prove content. Prove no-change/one-review traffic by method/byte/tuple counts without logging secrets or note content.

Boundary cases include immediately below/at/above each declared limit. Failures compare committed logical rows/schema/stamps and media metadata/blob hashes before/after, then prove ordinary retry/reconciliation with a fresh client. Record publication and acknowledgement separately when losing a reply; an error does not prove rollback. Concurrency uses deterministic phase barriers and exercises both linearizations. Resource reports include settings/hardware, memory, temporary bytes, connection peaks and timing; no hosted-scale claim is implied.

Every execution uses its `tools/mg` lane with temporary synthetic state. Automated reference/runtime work runs in Docker and never contacts a public sync service. AnkiMobile device evidence is separate; missing access/setup is unavailable with the actual external step, never inferred from Desktop. Preserve the original 26.09.3 oracle pin.

## C3. Transport probe families (P)

| ID | Required neutral observations and variants | Clauses |
|---|---|---|
| P-T01 | Exact version/build, routes/methods, header v/k/c/s/Unicode/client identity, exact JSON integers and native login request/result for each target | server-transport T2,T3 |
| P-T02 | Actual raw zstd framing, `{}` versus `_pad`, Content-Length/chunked, anki-original-size, null/integer/text/ZIP result decoding; additional forms only if actually emitted | server-transport T2,T3 |
| P-T03 | Missing/malformed/duplicate header members, invalid version, unknown/revoked key, empty/changed session, endpoint case/verb, no browser-cookie authentication | server-transport T2,T3 |
| P-T04 | Malformed/truncated/multiframe compression, size/nesting boundaries, >2^53/signed-64 IDs, strict versus loose tuple numbers, duplicate envelope members, unsupported schema refusal without mutation | server-transport T4; server-collection C4,C6 |
| P-T05 | Private HTTPS proxy framing, actual client handling of 400/403/409/413/429/503, valid HTTP version acceptance and no redirect; distinguish original policy from reference observations | server-transport T2–T4 |

## C4. Initial/full/schema probe families (P)

| ID | Required neutral observations and variants | Clauses |
|---|---|---|
| P-F01 | New account col mod/scm/usn/ls/crt/ver, required tables/indexes/config/default metadata; initial meta hostNum/empty/media_usn; fresh/fresh first sync and no-card metadata | server-collection C1 |
| P-F02 | Exact target upload/download versions, page/WAL/self-contained form, actually needed migration, before/after col and row/config/template/grave stamps; upload then repeated download; no double preparation | server-collection C6 |
| P-F03 | Equal mod/different scm, equal scm/different mod, all card-presence combinations, metadata/media without cards; existing-server migration, fresh receiver and initial direction/confirmation actions | server-collection C1; web W9 |
| P-F04 | Notes/cards/reviews/scheduling/decks/notetypes/fields/templates/presets/config round trips; unknown JSON at every supported level, unknown/opaque protobuf on full versus normal JSON channels, untouched/changed bytes, pinned unicase quirk | server-collection C3,C4,C6 |
| P-F05 | Corrupt/missing-table/newer/actually required older schema, harmless foreign objects versus unsafe triggers/views, stricter safe refusals affecting target-produced files; no silent deletion or false success | server-collection C6 |
| P-F06 | Full upload/download overlapping normal round with same/different key, retirement/refusal and stale finish, old-generation visibility, lost stamped-download response, media unchanged by full transfer | server-lifecycle L2,L3 |

Require exact input/output metadata, not a statement that a client works. If an original empty file needs additional metadata for a target, report the minimum interface values as a gap; do not copy stock objects to satisfy it.

## C5. Incremental probe families (P)

| ID | Required neutral observations and variants | Clauses |
|---|---|---|
| P-N01 | Account/session binding, same/different-key start, wrong/no session, valid/repeated/out-of-phase/after-done requests, abort absent/wrong/matching, malformed input, expiry/restart and transaction visibility/error retirement | server-lifecycle L2 |
| P-N02 | USNs below/equal/above minUsn, negative/ahead cursor, outgoing snapshot/response-before-merge; 0/249/250/251/500 tuples, last-empty chunk, grave split; no skipped/repeated outgoing IDs | server-collection C2 |
| P-N03 | Graves present/absent/pending edited; independent notes/cards/decks, repeated/conflicting types, default deck/hierarchy, same-round deletion then tuple/object; grave USNs and final rows | server-collection C3 |
| P-N04 | Object mod older/equal/newer and pending/nonpending, changed field/template counts, missing parent, normal/filtered replacement, case and repeated '+' collisions, preset duplicate names and resulting stamps | server-collection C3 |
| P-N05 | Tag normalization/spelling/parents/collapsed/config; lnewer true/false, conf/crt missing/null/present, config whole replacement/omitted keys/stamps, no keywise merge | server-collection C3 |
| P-N06 | Card/note pending-USN threshold, mod older/equal/newer, nonpending overwrite, accepted/ignored invalid notes; checksum/sfld storage affinity, controls/NFC/flags/data, missing type/field count | server-collection C4 |
| P-N07 | Repeated IDs within/across chunks with supplied-USN effects: absent/nonpending/pending and mod sequences 30→10, 10→20, 10→30; earlier accepted invalid note then valid; first review collision wins; same ID in distinct tables | server-collection C4 |
| P-N08 | Sanity due/grave zeroing, six-table mismatch/malformed counts and retirement, finish mod/ls/usn/scm/crt/row stamps including same-clock rounds, abort/error/restart rollback | server-collection C5 |
| P-N09 | Lost finish replies before/after durable commit, restart and ordinary client retry, review/grave/object/config replay; preserve documented tie divergence without claiming universal convergence | server-collection C3–C5 |
| P-N10 | No-change method/byte counts, one-review incremental tuple/media counts, no unnecessary full transfer; fresh receiver content/history/scheduling equality. Retained-Meguri results use M-I01, never substitute for these current-client facts | server-collection C1–C6 |

Original admission/timeout policy may differ at an unobserved reference edge. Resolve required target traffic before credit; do not widen the service based only on malformed artificial traffic. Record policy differences explicitly.

M-I01 is the separate retained-peer family: exercise the retained Meguri binary's normal/full/media exchanges against the candidate, recording exact binary identity, outcomes and known client blockers. Its classification is meguri_policy and its lane is retained_peer; it proves only that retained-peer result. It cannot seal any P-family observation or Desktop/Mobile acceptance.

## C6. Publication/fault families (M)

| ID | Required variants and assertions |
|---|---|
| M-L01 | Same-account/cross-account rounds, full versus normal, media versus collection; no cross-account data/paths, displaced connection commit or shared-writer/network-duration blocking |
| M-L02 | Upload faults in stream/decode/validate/durability/before-after manifest/before-after OK; download before-after stamp commit and 10/50/90% streamed bytes; restart selects complete valid generation, pinned readers safe |
| M-L03 | Media faults in decode/blob write/durability/before-after metadata commit/ack; complete invalid batch unchanged, referenced bytes durable, replay safe |
| M-L04 | Password change/reset/local recovery/disable during every M-L02/M-L03 phase and normal start/mutator/sanity/finish. Both barrier orders; no old publication/credential issuance after revocation acknowledges |
| M-L05 | Admin invitation/reset creation versus actor reset/disable/sign-out; target reset versus newer reset/disable; slow old password proof versus hash/generation change; stale authority never issues usable credentials/capabilities |
| M-L06 | Disk-full/short-write/fsync/rename/manifest/SQLite busy failures, interrupted cleanup/contradictory manifest, decompression/temp/concurrency budgets; no false success or silent empty replacement/current-state deletion |
| M-L07 | Double root owner, stable lock during restore, inconsistent initialization and owned orphan sweep; serving closed until recovery, unrelated files/links untouched |

Use deterministic test-only phase hooks with independently observed publication outcomes. Hook names belong to tests, not the wire contract. P-F06/P-N01 separately check target implications; these storage mechanisms are original policy.

## C7. Media probe families (P)

| ID | Required neutral observations and variants | Clauses |
|---|---|---|
| P-M01 | All five requests/envelopes/results, begin sk requirements, no collection session/begin requirement, concurrent normal round, exact USN/hash representation | server-media M1 |
| P-M02 | Upload index array/deletion positions, download object map, stored/deflated ZIP/CRC, missing/repeated/unknown members, real-name/reference duplicates, target safe filename forms | server-media M2 |
| P-M03 | 25/26 names, 50 KiB metadata, 255-byte name, 0/100 MiB file boundaries, 2,621,440 fill crossing and incompressible-body overhead, NFC/case/backslash/control/path behavior | server-media M2 |
| P-M04 | Effective add/replace/delete versus identical/absent no-ops, repeated names in one batch, processed prefix/all, USN increments/tombstones/SHA-1 actual bytes | server-media M3,M4 |
| P-M05 | Strict-above ascending pages of 0/1/999/1000/1001 states, subsequent same-name update, negative/ahead/equal cursor, intervening writer, sanity equal count/different bytes and missing-disk integrity | server-media M3,M5 |
| P-M06 | Ordered download prefix/empty request/missing after known, bytes/metadata, concurrent replace/delete, interrupted reply/lost upload ack/replay/restart | server-media M4,M5 |
| P-M07 | Two-way server-wins conflicts, deletion before first confirmation, pending deletion/live file, reset/rescan, case variants, fresh receiver byte hashes and no-change no retransmission; client limitations explicit | server-media M1–M5 |

## C8. Backup/client recovery families (M/P)

| ID | Required result |
|---|---|
| M-R01 | Stopped backup with stable lock/recover/checkpoint/closed databases, complete account/collection/media manifest; live/interrupted/corrupt/missing-reference refusal |
| M-R02 | Empty restore only; path/link/duplicate/hash/version rejection; interruption closed, stable lock identity, restored state/references preserved, bearer credentials/capabilities invalidated |
| M-R03 | Explicit historical password/enabled-state review, local admin recovery then admission opening across restart; no remote bypass or claim post-backup revocations survive |
| P-R01 | Exact Desktop 26.09.4 full-transfer/media-reset/rescan procedure and menu/forms, explicit authoritative direction, renewed credentials, complete content/hash equality |
| P-R02 | Exact AnkiMobile 26.10 device-build equivalent; missing device/setup is unavailable and blocks affected acceptance |
| P-R03 | Both procedures with delayed reconnect, equal mod/different scm, ahead/equal/reused/overtaken cursor, equal counts/different bytes, local edits/reviews and old hostkey; HTTP/counts not recovery evidence |

Service stopped-storage recovery and client recovery are separate claims. AE8 needs actual supported-target evidence; unavailable Mobile remains incomplete.

## C9. Browser policy families (M)

| ID | Required compiled-interface/API cases |
|---|---|
| M-W01 | Init token path/0600/no logs, missing state refusal, interrupted repair/rotation, manual/fragment token entry/clear/no GET redemption, simultaneous setup one identity and durable closure |
| M-W02 | Seven-day invitation/thirty-minute reset edges, malformed/expired/consumed/replaced/revoked tokens, validation/conflicts leave available capability, concurrent same-token/different-token username claims |
| M-W03 | Provision failure/restart/resume one ID/hash/reservation, exact retry proof versus changed credentials, no access before ready, expired/revoked issuer cannot publish or recycle reserved name |
| M-W04 | Admin also member, account collection/media isolation, member denied administration, hostkey denied browser and cookie denied native, list excludes secrets, disabled denied/data intact, sole-admin refusal |
| M-W05 | Login/logout/current-password change, fixed expiry, reset single-use/newer replaces/revokes all old credentials/data intact, local recovery no anonymous bypass, M-L05 races |
| M-W06 | Private HTTPS proxy/backend HTTP cookies, exact unsafe Origin including setup/login/reset, missing/null/wrong/repeated Origin and spoofed forwarding rejection, no permissive CORS |
| M-W07 | Fragment clear/no-referrer/no-store/no GET consumption, only explicit secret creation/claim, no external/demo/OAuth/email flows, exact username/password policy, generic errors and all request/hash/attempt budgets |
| M-W08 | All route/result/error shapes, busy/recovery feedback, keyboard focus/labels/announcements, copy/manual select, responsive compiled Rust-served assets; zero retries, screenshots/a11y evidence |
| M-W09 | Disabled reset stays disabled; no implicit enable/delete/promote endpoint; self reset/disable with another admin, sole admin refusal, sign-out/actor authority races |

## C10. Acceptance and lane accounting

| Outcome | Necessary evidence |
|---|---|
| F1/AE1 | M-W01,M-W03,M-W06,M-L07 |
| F2/AE2 | M-W02,M-W03,M-W04,M-W07 |
| F3/AE3–AE6 | P-T/P-F/P-N/P-M, M-L01–M-L06, M-W04; M-I01 retained Meguri evidence counted separately from Desktop/actual Mobile |
| F4/AE7 | M-W02,M-W05,M-W09,M-L04,M-L05; before/after data hashes |
| F5/AE8 | M-R01–M-R03, P-R01 Desktop, P-R02 Mobile and P-R03 separately for both actual targets |
| F6 | M-W05,M-W08 and verified P-F03 connection instructions |
| R11/R31 | P-N10/P-M07 plus initial/full/media-recovery reproducible timing/resource report |
| R4/R6/R29/R32 | Role/provenance/review/privacy/similarity, exact package/notices/assets hashes, synthetic fixtures; no application acceptance implied |
| R33 | Existing dependency/package evidence records justified external packages, locked direct/transitive graphs, license/security review and notices; audited ownership/license boundaries for existing core reuse remain intact |

Planned jobs: `tools/mg sync-server-reference`, `sync-server-interop`, `sync-server-test`, `sync-server-web`, `sync-server-faults`, `sync-server-package`, `sync-server-bench`, `sync-server-deps`. Names do not prove availability/execution. Parent owns static/provenance/privacy/similarity and batch gates. Record arguments/source/lock/build/fixture hashes/original batch base, nonzero selected cases, expected/completed counts and unavailable reasons. Schema validity is not runtime proof. Zero cases, skipped targets, stale source or missing independent review cannot satisfy acceptance. This drafting unit performed no runtime verification.

## C11. Lane identity and mandatory semantic validation

Each matched, different or sealed case has a structured `lane` containing kind, client and server identities, plus `fixture_sha256`. Lane kind `reference` pairs actual Desktop/Mobile with the pinned reference server runtime; `candidate` pairs actual Desktop/Mobile with Meguri Sync; `retained_peer` pairs the retained Meguri binary with Meguri Sync; `policy` pairs the synthetic service/test driver with Meguri Sync. The client kind `service` names this driver, not an Anki client. All client/server identities contain exact version and build. Reference server identity includes its runtime SHA-256; Desktop identity additionally requires the pinned SDK/runtime SHA-256. Meguri Sync peer identity includes the tested executable SHA-256 and source commit/tree/lock hashes. The retained client requires its executable hash; the synthetic driver requires its runtime hash. Do not substitute a label such as latest for an exact build.

AnkiMobile identity requires version 26.10, exact app build and OS build, with a hashed `device_evidence` artifact from the actual device run. That artifact binds the device's observed installed build/OS and execution receipt to this case's fixture, peer and results. Extracting or hashing the closed Mobile app binary is not required and must not be claimed when unavailable. Desktop SDK/runtime hashing and Mobile device evidence are intentionally distinct. Mobile screenshots alone, without that execution/build binding, cannot satisfy the device evidence role.

Every P-family case uses an actual Desktop or Mobile client and reference/candidate lane. P-R01 specifically requires Desktop against the candidate; P-R02 requires Mobile against the candidate; P-R03 expands separately for both actual client kinds against the candidate. Other P families cannot be sealed by a retained client, service driver, or a P ID relabeled as policy. A reference-lane seal establishes the observed reference behavior for that exact target/runtime. A candidate-lane seal establishes only that target's interoperability with that exact Meguri Sync build; it cannot establish a new reference behavior fact. M-I01's retained results remain separately scoped. A family's unavailable target remains incomplete even when another target passes.

Matched/different/sealed evidence requires hashed synthetic-input, neutral-observation and MG-receipt artifacts. A seal additionally requires a hashed independent review of the current case, final source/peer/fixture identities and observation outcome. All hashes identify the actual retained artifacts; a syntactically valid digest or stale receipt is insufficient. The synthetic-input artifact identifies the fixture manifest named by `fixture_sha256`, whose entries cover every action's input fixture. Observation and MG artifacts bind case ID, lane/client/server identities, source/lock/executable identities where applicable, fixture identities, selected variant and recorded outcomes. Review must bind these same final inputs; an earlier reviewed source or a result from the other lane cannot be reused as this seal.

U3 owns implementation of semantic validation at the `tools/mg` harness boundary, in addition to JSON Schema validation. It must reject duplicate IDs; require each ID's prefix before `:` to equal its declared family; verify family/classification/lane/client binding; and ensure each required contract variant and target is represented exactly once in the expected selection. `expected_case_count` is the contract's required expansion for that lane/job selection, `selected_case_count` is the number of unique selected case IDs in `cases`, and `completed_case_count` counts only matched, different, failed or sealed execution outcomes. Unobserved and unavailable cases are not completed. Recompute all three counts and compare them to the declarations and actual MG selection/execution receipts. Credit requires nonzero selection, selection equal to the expected expansion, every required case completed successfully, and no hidden/skipped/unavailable variants. Report incomplete coverage truthfully; do not reduce the expected count to the cases that ran.

The same validator compares named expected/observed outcomes using the owning neutral spec's exact numeric/tuple/order/content semantics; wide integers use exact decimal strings where a JSON consumer would lose precision. Matched/sealed requires all expected assertions to match, and different requires an actual mismatch. Runtime failures stay failed; independent review cannot relabel a mismatch or execution failure as a seal without a new reviewed contract and new applicable evidence. Require unique observation names and resolve every artifact/fixture hash against its retained bytes. Check candidate peer source/tree/lock/executable binding to the manifest and MG build/package receipts, reference/desktop runtime hashes to their pinned-runtime receipts, and Mobile build/OS/device binding to its device receipt. An execution, review or source mismatch invalidates credit before a lane is reported passed. No schema-valid manifest by itself proves these checks were implemented or run; this U2 correction establishes no new runtime observations.

### Selected Desktop media execution

`sync-server-reference media` runs four SDK groups against an isolated reference account. The `media` candidate phase uses the same groups and the existing full-phase build receipt. It adds no compile phase. The full and normal selections retain eleven and ten cases.

The media groups share account history. Two clients exchange original SVG and WAV additions, replacements and deletions in both directions. Both then perform a no-change sync. The receiver downloads 26 files through requests for 25 and one file. A fresh client receives 1,005 current filenames through listing pages of 1,000, five and zero entries. Six earlier mutations remain reflected in the final USN of 1,011. Every receiver's file hashes must match the generated bytes. Public SDK completion must return successfully. Crafted requests receive no credit in this selection; Mobile, restart and concurrent publication remain separate evidence.
