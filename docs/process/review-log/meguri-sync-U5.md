# Full collection transfer

This records the reviewed collection body and its integration checkpoint.
The full-transfer candidate lane passes all eleven selected current Desktop
backend cases. Acceptance awaits the coherent batch gate. Incremental sync,
media, release packaging, GUI and AnkiMobile remain separate.

Original batch/source base: `aa8565719af435051de7d5f068c0c817286b568e`.
Canonical integration starts from the accepted foundation and subsequent
metadata-only approval/provenance commits. No core or application behavior changed.

## Implemented body

Native host-key login, metadata, raw-zstd framing, compatible schema-18 files,
full upload, durable generation publication, pinned committed downloads and
bounded temporary/connection/stream resources are implemented independently.
Full transfer preserves whole-file contents and supported opaque values rather
than regenerating unrelated rows. Collection metadata remains separate from
account identity and credentials. Small note-derived-value and pinned collation
adapters have neutral vector coverage; normal-sync routes remain absent.

The standalone graph has 149 resolved packages, 20 production direct edges and
two development edges. The public model crate has no enabled features and only
its four intended production dependencies. New server code is MIT; reused model
files retain MPL-2.0 terms. The dependency assessment records purposes, features,
maintenance and license evidence. Scoped deny/about passed at
`/private/tmp/meguri-sync-U5-20261115/build/mg/sync-server-deps/20261115T194209Z-83856/result.json`.
These process and graph checks are not exclusive ownership or release evidence.

## Authorship and source review

Fresh filtered author/fixer sessions are recorded separately in the provenance
ledger: `s-20261115T185239Z-b90fa5f6`, `s-20261115T192058Z-e0c6e077`,
`s-20261115T200826Z-a722bd2e` and `s-20261115T203006Z-968b7d3c`.
Dependency-document updates also carry the separate wrapper sessions.
All original session commits were imported through normal merges. No source
commit was reconstructed from the initial compile-failure export.

The combined independent body review found four concrete issues: foreign-key
actions on collection connections, permanent admission closure after transient
metadata overload, generation-retirement/binding-validation races, and upload
sidecar reservations. A misleading parser comment was also removed.
The repaired implementation disables and verifies collection foreign keys while
preserving account constraints, distinguishes uncertain publication, serializes
authorized retirement with binding checks, and validates uploads through a
hardened immutable read-only connection that refuses auxiliary files.

A small followup explicitly moved the entire publication guard into blocking
work so uncertainty and cancellation cannot lose its cleanup action. An earlier
in-progress `blocking_send` observation was retracted: the frozen source already
used `try_send`. Its independent targeted rejection remains retained.

One combined source/simplification review and bounded delta reviews were used;
proof tests were preserved. Final seven-file review found no actionable issue:
`/private/tmp/meguri-sync-u5-body-delta-review-20261115/build/mg/cleanroom/20261115T204649Z-62883/codex.out`.
All 38 final hashes and the complete filtered output inventory were unchanged
by that reviewer. Reviewers inspected source and ran no Docker tests; the
orchestrator executed and inspected runtime verification separately.

## Verification and retained failures

The initial account baseline passed 95 tests. The first transfer compilation
failed on an unavailable SQLite configuration API; it was repaired without
claiming those tests had executed. No preimplementation red was observed for
the original transfer source. The resulting body then passed 116 tests.

Two defensive regressions actually failed against the unchanged original body
for foreign-row preservation and admission recovery:
`/private/tmp/meguri-sync-u5-body-repair-verify-20261115/build/mg/sync-server-test/20261115T202054Z-33122/result.json`.
The repaired body and guard followup passed 124 tests, with zero skips/retries.
The tiny guard followup has no observed pre-fix red claim.

Independent parent verification of all 38 exact file identities before and
after execution:

- Tests, 124/124:
  `/private/tmp/meguri-sync-u5-guard-repair-20261115/build/mg/sync-server-test/20261115T204734Z-63671/result.json`.
- Default and all-feature correctness/safety lint:
  `/private/tmp/meguri-sync-u5-guard-repair-20261115/build/mg/sync-server-lint/20261115T204818Z-64210/result.json`.

The first similarity scan flagged only the required schema declarations.
All 28 SQL statements exactly match the neutral schema contract. Antoine
approved structural coverage only for `server/src/collection/schema.sql`
lines 1–28, recorded as `E-20261115T200432Z-c0f9`. Other channels, paths and
thresholds remain checked. The approved frozen scan passed 32 supported files,
with one schema allowlist entry. Updated canonical checks remain required.

The first privacy scan reported 14 public-source vocabulary matches, naming
only paths and lines. No private token spellings or collection were read.
Independent source reviews classify the full original body and repaired delta
as public code/process/synthetic material. Any maintenance entry is limited to
its exact path and final full-file SHA; matching logic is unchanged.

## Actual Desktop full-transfer result

Committed candidate source a38664a4983c49fb2fabdb2f8e6004809b3c45e8 passed
all eleven selected cases, with zero failures or skips. The actual pinned
semantic validator and local retained-artifact validator passed. Compatibility
credit is true, client and pinned-validator exits are zero, and cleanup completed.
This is a debug executable with no enabled Cargo features, not a release build.

- Build: `build/mg/sync-server-candidate-build/20261115T230839Z-84330/result.json`.
- Interoperability: `build/mg/sync-server-interop/20261115T230850Z-84973/result.json`.
- Source archive SHA256: `2942e97ef5f8c9b2bbb3773759546c09f78ce67df2dd2cd15d1305dab419ebcb`.
- Cargo lock SHA256: `e6ff1a10840e84fac2a83621a1c4d4dbe21cf6915215ff59deb4d4532b3b8036`.
- Executable SHA256: `428fac17b44f96cee409dce0a0600236260d6c034611cea92fdb8497deee6ea6`.

Earlier failures stay unchanged and earn no retroactive compatibility credit.
The 214028Z-3179 invocation rejected an unsupported Compose flag before cases.
The 215904Z-22957 build compiled but failed its read-only source-mount check.
The 221633Z-38698 exchange matched eleven cases but failed static-action
validation on an optional recorder fixture hash. The 223611Z-52762 exchange
matched eleven cases but lacked per-case action-fixture artifact links. Those
bytes existed globally; missing links caused the failure. Reviewed repairs
preserve strict source/lock/executable binding, ordered actions, per-case
manifest links and content-addressed artifact checks. A proposed test duplicating
the command array was not landed; existing behavioral fixtures and actual Docker
executions cover these repairs. Scoped similarity excludes quarantined oracle
source. This result does not grant incremental, media, GUI or Mobile acceptance.

## Remaining integration

The candidate wrapper and artifact-retention repairs are independently reviewed.
The actual result above records their executed full-transfer scope. The coherent
batch gate remains pending; isolated full-transfer success does not replace it.

The user subsequently requested the setup code and setup URL in first-time
initialization output, accessible through Docker logs. That narrow intentional
bootstrap disclosure and matching browser hint are pending shared startup/web
integration; unrelated credentials remain private. It is outside this frozen
body review and does not authorize logging setup codes on routine restarts.

No PR, push, deployment, real account setup or private collection transfer is
part of this checkpoint. Final local integration preserves individual commits.


## Accepted shared batch verification

The coherent collection, incremental, media and web batch is accepted at clean source `e54b80acb91675e3527a8445596682753f313612`.
Its original batch base remains `aa8565719af435051de7d5f068c0c817286b568e`.
Earlier pending statements above describe historical checkpoints.

The complete [gate receipt](/private/tmp/meguri-sync-collection-media-gate-20261116/build/mg/gate/20261116T173757Z-67818/receipt.json) is passing and merge-ready.
All 29 selected stages passed, with no blocking, incomplete, queued or quarantined checks.
The wrapper reports a warning for 30 untriggered jobs and the pre-existing disabled legacy Anki server harness.
That disabled entry predates this batch; no test was disabled to obtain this result.

The exact source passed 188 server tests with zero skips, nine frontend proofs and the actual browser journey.
One fresh compiled debug server passed all selected Desktop full-transfer (11), incremental (10) and media (4) groups.
Client and semantic-validator exits are zero; the results grant compatibility credit only to those finite selections.
Source, executable, runtime and retained-artifact identities remain in their nested receipts.
The native/Docker conformance, live-reference, dependency, provenance and similarity checks also passed.

The first-initialization setup-code output and matching web hint are integrated and tested.
Docker entrypoint/log delivery, final packaged-artifact validation, the retained Meguri peer and actual AnkiMobile checks remain later work.
Backup/restore and production deployment operations are explicitly deferred by the owner's 2026-11-16 scope decision.
No final application, Mobile, restore or production-deployment acceptance is claimed here.

Earlier failures and cancelled receipts remain unchanged.
The previously observed maintenance statistics mismatch did not recur in this full gate.
Its cause remains unproved; this passing result establishes current verification without claiming a repair.
