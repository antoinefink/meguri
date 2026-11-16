# Meguri sync providers

Spec: sync/meguri-provider
Requirements: Antoine's explicit 2026-11-16 two-service decision; existing-unit mapping in docs/process/review-log/meguri-provider-contract-20261116.md#implementation-mapping
Anki: none (original Meguri provider policy)
Scenarios: none (focused and cross-layer acceptance in docs/process/review-log/meguri-provider-contract-20261116.md#follow-on-acceptance)
Status: Reviewed for implementation 2026-11-16 (delegated decision; MP1–MP5).
Review: docs/process/review-log/meguri-provider-contract-20261116.md#contract-review

Antoine explicitly requested iCloud Sync and Meguri Sync using Meguri's own server on 2026-11-16. Root adopted MP1–MP5 after independent contract review and closure of the media-exclusion finding. This is delegated routine implementation authority, not technical acceptance or owner approval of every detailed clause. It changes no existing CloudKit record/recovery wire contract and does not authorize legacy Anki transfers or AnkiWeb work.

## MP1. Provider, account and collection are separate

There are two services: **iCloud Sync** and **Meguri Sync**. Local-only use remains supported without either account. One provider can own synchronization of the installed collection on this device. An account can be configured without owning the collection.

Store selected provider, pause/automatic settings and connection references in device-local state, outside the transferable collection. Import, export and bootstrap always include collection and media; these flows offer no media-scope or media-off choice. Store the active provider binding and handoff intent durably through existing guarded metadata seams. The binding names the provider, opaque account reference, collection/publication generation and admission revision. Never treat a UI picker value as admission authority. Present-but-invalid metadata blocks network work; absence alone permits migration defaults.

An upgrade preserves the existing iCloud choice, pause state, binding, pending work and publication fence. Devices with no established choice remain local-only. Never infer an account or upload permission from collection stamps. A backup restore, package import, collection replacement or account change invalidates incompatible provider admission before work resumes.

## MP2. Native Settings and credentials

Keep Account & Sync in its existing native Settings location. Its first section uses a standard SwiftUI Picker labelled “Sync Service”, offering iCloud Sync and Meguri Sync. A separate Stop Using action supplies local-only use. Selection opens setup or a switch preview; the displayed active service changes only when its handoff commits.

Keep account, connection and status under the selected service, separate from Collection import/export. Meguri setup uses labelled Server Address, Username and SecureField Password controls, then Sign In. Sign In authenticates only. Test Connection, if offered, is beside the address and performs only its disclosed authentication/metadata check. Opening Settings, editing a field or merely selecting an unconfigured provider starts no Meguri request.

Reuse endpoint normalization; require HTTPS for the native Meguri provider. Plain HTTP remains only a job-owned synthetic test fixture exception, not a relaxation of the generic transport contract. No built-in real endpoint or credential. Do not follow login redirects or forward secrets to a new authority automatically. A changed redirect authority requires explicit connection review and new credentials; keep configured identity distinct from the accepted transport target.

Canonicalize own-server usernames by `servers/meguri-sync-web#W2`; preserve password bytes. Keep username and opaque hostkey in a **separate provider Keychain namespace**, device-only and nonsynchronizable. Do not adopt the hidden manual-transfer slot. Clear the password after the attempt. Endpoint edits invalidate credentials and redirects before another request. A locked/failed Keychain is an actionable storage error, never permission for a plaintext fallback. Persist only opaque account references in operation/history state. Login proves no stronger alias or account equivalence than the protocol returns.

Sign Out/Remove Connection stops admission, drains work and deletes local credentials before reporting completion. It keeps the collection, media, backups and unresolved-operation evidence. It deletes no server data and claims no server-side hostkey revocation; password/reset administration remains the existing browser workflow. No automatic fallback to iCloud follows authentication failure or removal.

Use native Forms, Sections, Buttons, sheets and destructive roles. Retain the existing toolbar/status location and keyboard ownership. Add no custom chrome or restyling. The status identifies the active service; account validity, collection exchange and media progress remain distinct. Use “Last exchange completed”, pending/held work and partial-media state, rather than claiming all devices match.

## MP3. Durable exclusive handoff

Switching is a local handoff, not a migration of remote stores and not a service-to-service bridge. Explain that other devices must choose the intended service themselves. Changes held only by the old remote service are not silently copied. Preserve both services' remote data.

1. Refuse handoff during an active review, editor commit, collection replacement or unsettled recovery/publication operation. The user can continue local review and retry later. Show pending collection/media work and any held changes before confirmation.
2. Record intent naming old binding, proposed service/account, current collection/publication identity and prior decisions. Block new work from both providers. Cancel and join old collection/media jobs and all admitted callbacks through their actual owner boundaries.
3. Preserve completed effects and pending state. For iCloud use its stop/isolation mechanism (`cloudkit-recovery#V16`) without purging tracking, conflict copies, remote epochs or publication fences. Require provider/generation guards on **every** later callback, batch, apply and media install, including old SDK callbacks.
4. Commit the old provider inactive and destination **setup required** in one recoverable metadata transition. Start no destination writer until old admission is closed and drain is proved. Failed durable publication leaves network admission closed; retain the journal for recovery.
5. On restart reconcile the intent before starting either provider. Resume a proven committed selection; otherwise stay stopped with an explicit recovery action. Never choose whichever credential or service responds first.

An upload that may have reached the server retains its existing global exclusion across pause, sign-out, provider changes and restart. Stopping a worker or forgetting a key does not settle remote acceptance. Connection removal may forget credentials but cannot release this exclusion. Local review can continue; a new network writer cannot.

Returning to either provider requires explicit reconnection and current collection-choice validation. Do not restore an old “clean” baseline after work under another service. Reuse iCloud V13–V15 choices and fences on return; never silently republish the Meguri version to an old iCloud epoch. Preserve existing iCloud encryption/key, account, container/environment, unknown-field and recovery protections.

## MP4. First connection and full replacement

For a new Meguri account binding, endpoint/account change, provider return, imported/replaced collection or lost baseline, require explicit bootstrap. **Do not run normal sync to discover whether unrelated collections can merge.** Equal stamps, zero cards and `meta.empty` do not prove an empty store or common lineage.

| Observed state | Required action |
|---|---|
| No local collection | Offer Download from Meguri Sync. Stage, validate and activate through existing lifecycle/replacement rules. There is no local file to back up. |
| Any local collection, including zero cards | Show “Use This Device's Collection…” and “Use Server Collection…” plus Cancel. Explain which collection is replaced and that media reconciliation is bidirectional. Neither destructive action is the default. |
| Remote facts unavailable, unsupported or corrupt | Show the typed failure. Keep local data and both providers stopped for setup; never assume remote absence. |
| A baseline is proved for this exact active binding and generation | Admit normal synchronization under MP5; a protocol full-sync requirement returns to the explicit choice. |

Reuse `sync/anki-transfers#M2–M5`, `sync/one-way#O4–O7` and `engine/collection-replace#R6` as the **safety mechanics**, without exposing the old Anki UI or inheriting its automatic-work prohibition for this new provider. Reuse only the narrow admission, drain, backup, replacement and media seams; add no general synchronization framework. Legacy collection-only allowances do not apply to these newly authorized flows.

For download, use owned staging and verified finalization, force the local safety backup before activation, then record intent, atomic replacement and actual resulting generation. Failed backup prevents activation. For upload, take the local safety backup, download and retain the server collection copy even for zero cards, refresh facts after that download's stamp changes, and require final destructive confirmation. Bind confirmation to actual preview/operation, account/endpoint, source generation and artifacts. Changed facts require a refreshed decision.

The protocol does not lock the interval between server copy and upload. Explicitly warn that another client's later edits can be replaced, and instruct the user to stop sync on other clients during replacement. Retained backup and fresh metadata are **not** conditional-write guarantees. Never auto-upload a full collection after a mismatch. If strict cross-client exclusion is required, that is a genuine missing server capability; do not invent a lease or silently claim this flow supplies it.

Persist intent before dispatch and exact `OK` before local upload activation. A lost acknowledgement stays “Server acceptance unknown”; preserve candidate, backups and exclusion until existing evidence-based recovery settles it. Report remote acceptance separately from failed local activation. Never clear an iCloud publication fence because a Meguri operation succeeded.

## MP5. Routine sync, reviews and media

Only a completed collection-and-media bootstrap establishes the provider's ready binding. Its explicit confirmation discloses subsequent automatic synchronization of both. Persist completion against the actual binding and resulting generation, then enable Meguri automatic sync without another setup step. Until then, Sync Now continues the explicit setup/recovery flow and performs no speculative normal exchange. Sign-in alone establishes no baseline and transfers nothing.

After successful bootstrap, Sync Now uses the retained **normal** job through a new provider-specific core/FFI adapter. Meguri automatic sync is ON after that explicit consent. Coalesce flushed local changes and foreground/reachability events; review completion can request a round. Use existing background/quit budgets; do not promise continuous execution on iOS. Existing Pause behavior closes network admission for collection and media, keeps pending work and makes Sync Now unavailable while paused. Resume reopens the same valid binding; it does not require another media choice. Any later Automatic Sync setting retains existing semantics: off stops automatic scheduling, while Sync Now still includes media. Preserve iCloud's existing scheduling defaults and semantics.

Normal work retains `sync/hold-back` barriers, durable answer writes, latest-answer undo hold-back, staged incoming changes and conflict rules. No review answer waits for the network. No replacement runs during a review. Recheck generation, schema admission and provider authority before mutation/finish; a late structural change requires an explicit full-sync decision. Authentication failure stops that binding and asks for sign-in; failed/cancelled/lost-ack rounds retain pending work and do not claim convergence.

Media is required for every import, export and bootstrap. Bootstrap discloses reconciliation of both sides separately from the chosen collection direction, including possible replacements, deletions and retained unreferenced files (`anki-transfers#M5`). Use the retained media reconciler with its own admitted owner and fresh baseline epoch after a provider/account/collection change. Do not reuse CloudKit cursors, USNs, keys, tombstones, pending downloads or “clean” marks. Rebinding retains physical live files and scans them; it grants no consent by itself.

**Unsupported/excluded media:** preserve and explicitly report every file the reused media path cannot transfer, including empty files and files over its 100 MiB limit. These files cannot count as completed all-media bootstrap or enable automatic normal sync. File import/export must account for every promised media file from the actual source/package inventory; a syncable-only journal inventory is insufficient. Classify each promised file as transferred, pending or unsupported/excluded. Any pending or unsupported/excluded file prevents whole-operation completion. Keep an actionable media-blocked phase with Show Affected Files and Retry; local study remains available. Retry retains existing binding, artifact and resume guards and cannot clear the block merely by omitting a file. Never automatically delete or convert excluded files, weaken limits, or infer a new server capability.

Serialize provider collection rounds and media phases initially. Reuse existing media-job exclusion, exact-name/hash checks, trash-before-replace, bounded streams and durable batch acknowledgement. Report collection and media progress/failure separately. “Collection transferred; media incomplete” describes retained effects, not successful completion of a smaller scope. Local study remains available while media retries, showing missing media honestly. Bootstrap remains incomplete until media finishes; automatic normal work does not bypass its retained admission. Routine media pauses with the service and resumes pending work without changing the promised scope. Resume validates the same binding and artifacts; changed ownership requires a new setup decision.
