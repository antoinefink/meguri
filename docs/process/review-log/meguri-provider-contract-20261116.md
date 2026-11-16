# Own Meguri provider: review and implementation intake

This records a delegated contract decision and planned implementation. It grants no technical acceptance.

## Contract review

Antoine explicitly requested two services on 2026-11-16: iCloud Sync and Meguri Sync using Meguri's own server. The scope does not authorize AnkiWeb, legacy Anki transfers or Anki code reuse.

Root reported independent contract review with one required P2 and no other P0–P2 findings. The P2 concerned media excluded by retained sync limits. MP5 now preserves and reports excluded files, accounts for every promised file, and prevents all-media bootstrap completion or automatic normal sync while any file remains pending or excluded. Empty and over-100-MiB files receive no silent exemption. Local study and guarded recovery remain available.

Root then reported the independent follow-up CLEAR, with the sole P2 closed and no P0–P2 findings. Root adopted MP1–MP5 as the delegated routine implementation contract under Antoine's explicit request. This is not owner approval of every detailed clause, an implementation pass, native design acceptance, or a change to existing CloudKit record/recovery wire contracts. Root retains the independent review records; this summary does not invent reviewer identities or receipt paths.

Recorded spec-writer session, as supplied by root: `s-20261116T213712Z-7e669259`; ledger-only commit `4f9f92394`. This preparation did not inspect Git. Before committing, root completes exact-content review, privacy review and static checks. No implementation unit is complete here.

## Integration contents

`specs/sync/meguri-provider.md` carries the reviewed MP1–MP5 text. Only its metadata and section-heading level differ from the reviewed proposal. The status means reviewed for implementation under the delegated decision; it does not replace any existing R2066 approval or Under review scope.

The authority patch changes only the iCloud-only scope in `AGENTS.md`, `review-first-delivery.md` and native-interface-contract N12.4. It retains legacy hidden routes, tests and freeze limits. The retained normal, hold-back, media, manual-transfer and one-way specs gain narrow caller cross-references. `sync/policy` gains a scope cross-reference outside its state-machine rules; its existing Status line remains untouched. No CloudKit wire, key, record, recovery or failure rule changes.

Before new UI lands, update only the affected `screen-inventory.yaml` entries (`settings.accountSync`, `sync.status`, necessary setup/choice routes) and `sync/status-strings` keys. Keep native controls, iCloud identifiers, keyboard ownership and review presentation. Regenerate `screens.md` through its existing job. No preview, native design agreement or UI acceptance is supplied by this document.

## Implementation mapping

The U-numbers below belong to the original application plan unless explicitly labelled server-plan. These are existing responsibilities and reuse boundaries, not new completed units. MP-A–MP-E are planning labels only; they must not appear as completed `Unit:` trailers.

| Order | Follow-on work | Existing application-plan mapping |
|---|---|---|
| MP-A | Device-local provider choice, secure account state, durable admission and handoff | U151 lifecycle/coordinator; U160 Settings; U161 status; U24 generation/jobs; U37 FFI services |
| MP-B | Explicit bootstrap and replacement with real backups, retained uncertainty and required media phase | U124a replace/package primitives; U126/U127 backups; U162 import/export; U109 safety mechanisms only |
| MP-C | Required media, durable owner/resume, complete promised-file inventory and blocked exclusions | U31 media service; U110/U111 retained media mechanisms; U151/U153 lifecycle/status |
| MP-D | Admitted normal-provider core/FFI/Swift caller after complete bootstrap; automatic sync and review hold-back | U105/U106 transport/messages; U107 normal/hold-back mechanisms; U24/U25/U40/U43 jobs/undo/day/review; U37/U151 bridge/coordinator |
| MP-E | Native service/account/status/recovery and final collection-plus-media workflow | U151/U153/U160/U161/U166 lifecycle/status/Settings/connection; U175/U178/U179 real review, answers and undo |

The original plan's unit table supplies these mappings at lines 3998–4026, 4062–4073, 4095–4111 and its Frozen Unit Index at 4197–4203. Historical paths there are responsibility mappings; implementers use current public interfaces. Reusing frozen U105–U111 mechanisms for this narrowly authorized provider does not accept those legacy units, reactivate their old delivery scope, or unfreeze U116/AnkiWeb. U113/U114 remain the existing CloudKit policy/adapter boundary, whose behavior and approval scopes must be preserved.

## Server dependencies and ownership

The separate server plan is `docs/plans/2026-11-15-1028-feat-meguri-sync-server-plan.md`; its U1–U11 IDs are local to that plan. Accepted foundation and U5–U8 evidence remain in `meguri-sync-foundation.md` and `meguri-sync-U5.md` through `meguri-sync-U8.md`. Public U8 lines 102–109 record gate `20261116T173757Z-67818`: all 29 selected stages passed with no blocking, incomplete, queued or quarantined checks. This does not establish application interoperability or full server delivery.

Root reports isolated U10 release-image verification at 4/4 PASS, not integrated on master yet. This is a dependency report, not reproduced evidence or master packaging acceptance. U11 M-I01 is not dispatched. Its test-only retained-client work uses existing public client APIs and belongs to the server coordinator. App/core/Swift/FFI provider integration belongs to the app coordinator; do not duplicate that peer harness or infer its completion. Today's inspected Desktop-only interoperability lane is not already a Meguri peer lane.

Keep server R32's original no-app-change acceptance boundary. This follow-on has separate authority and acceptance. U9 backup/restore and production deployment remain deferred. No real endpoint, account or credential is needed in committed code or docs. Use synthetic, job-owned fixtures only.

## Follow-on acceptance

Read `verification-status.md` before the batch. Required active repairs precede unrelated implementation. Keep the failed app gate and original integration base **`1416a520542a8c5daa38a23bfe398da704c3d6c9`**. Record the actual coherent batch start and each unit start separately; accepted server bases do not replace the app base.

Use focused checks, independent source review, separate implementation/fix commits and per-unit review/fix logs. Preserve provenance, complete-content privacy review, exact hashes and similarity checks. Root completes required checks before committing this contract; this preparation supplies no pass or scanner admission.

| Work | Required focused evidence, through existing MG jobs or a narrowly registered owned-fixture lane |
|---|---|
| MP-A | Migration preserves iCloud state/fences. Restart, corrupt metadata, credential-store failure, sign-out and stale callbacks cannot admit two writers. Core and Swift owner checks; properly signed real Keychain evidence. |
| MP-B | Actual own-server full transfer in both directions, including zero-card metadata, required backups, stale previews, disk-full, interrupted activation and remote-accepted/local-failed outcomes. Lost upload acknowledgement preserves exclusion. Collection completion alone is a retained phase, not bootstrap success. |
| MP-C | Actual bidirectional media add/change/delete and no-change preserve bytes across restart. Test case collisions, stale owners, cancelled batches and unsupported/excluded files. Complete source/package inventory must prevent false all-media success; verify received playback and cold retention. |
| MP-D | Start from completed collection-and-media bootstrap; automatic sync is on under its consent. Prove exact card/revlog identity, scheduling, held undo, rollover, offline retention, no-change and one-review incremental behavior. Pause affects both phases. Diagnose the retained delayed-meta/schema failure if it affects this authorized path; keep original failed evidence. |
| MP-E | Exact native build routes one service through Settings, toolbar and review commands. Sign-in transfers nothing. Verify switches both ways, credential removal and required collection-plus-media import/export/bootstrap, with separately reported phase failures. Run affected Xcode, snapshots, accessibility, UI and lint checks. |

Full upload retains the copy/upload race: a server copy and refreshed metadata supply no conditional-write guarantee. Explicit replacement consent, backups and uncertain-upload exclusion remain mandatory. Never infer a new server capability.

Run the required full app gate at the coherent batch boundary from the original base. Focused passes do not clear the failed gate. Keep all milestone and final device requirements. Host iOS simulator iteration is owner-authorized when the VM is inconvenient, but supplies only its actual local evidence, not formal-lane acceptance. No jobs or simulator work ran in this spec preparation.
