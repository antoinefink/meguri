# Manual Anki server transfers

Spec: sync/anki-transfers
Requirements: R118, R119, R120, R121, R126, R1540, R1548, R2060, R2061, R2062, R2063, R2064
Anki: none (Meguri product policy; retained protocol facts are cited in F1)
Scenarios: none (focused core, local-server and native acceptance in M8)

This contract governs explicitly selected self-hosted server transfers. Routine sync uses CloudKit.
Collection Import and Export are directional; separately authorized media reconciliation changes both sides.
Collection-only is a valid selected scope, but does not complete the full include-media delivery requirement.
File import/export follows its package/native contracts and is never subject to this server cooldown.

## F1. Retained protocol facts and proof limits

`sync/login-endpoints#L1` and `#L2` define endpoint normalization and method paths, including optional prefixes.
L5 returns a host key only; L7 makes it opaque. No canonical account id or alias-equivalence proof is returned.
`sync/wire-format#W3`: meta.empty means no cards, not no collection, notes, history, decks or configuration.
Meta stamps and USNs are observations; they are not a transaction id, accepted-file hash or conditional-write token.
`sync/wire-format#W14` and `sync/one-way#O6.4`: exactly OK means upload acceptance; other responses are not success.
`sync/one-way#O7.2`: the official server may upgrade a collection schema when accepting it.
O7.4: download raises server col.usn and replaces col.mod; the downloader then sets local col.ls to that mod.
A later download's bytes therefore need not hash like the uploaded file, even with no user content change.
O6.1 permits another client's change between preview, server copy and upload; these requests are not one transaction.
O12 permits remote acceptance without a received/durable OK. Local cancellation does not prove remote rollback.
`sync/media-sync#Y3` pulls and pushes; Y6–Y11 define decisions, durable batches, acknowledgements and sanity checking.
These are retained compatibility facts, not permission to run requests automatically or reconcile without consent.
U109 one-way and U110/U111 media contracts are integrated. Their code, scenarios and acceptance remain pending;
M5 support is still required. U107 normal/hold-back contracts remain a deferred dependency, not an implied exchange.

## M1. Entry, credentials and isolation

Settings offers self-hosted endpoint configuration, Login, Import from Anki and Export to Anki in every build.
Validate the endpoint with L1/L2. Changing it clears active credentials and redirects before another request.
Keep historical operation/cooldown records. Store username and host key in device-only nonsynchronizable Keychain.
Never persist the password, log credentials, or put credentials in operation records, URLs or diagnostics.
Login returns to the selected direction's preview; it never transfers or schedules a future request.
Only explicit preview/preparation, Start and Resume actions may issue their disclosed server requests.
Launch, foreground, timers, background, review end, Sync Now and integrations never start Anki-server work.
This includes automatic status refresh and media jobs. Already authorized work may continue within lifecycle limits.
Never contact AnkiWeb in this self-hosted delivery or its tests.
Use existing bounded streaming transport, typed errors/progress and the adjustable 60-second inactivity timeout.
Background continuation may finish an already authorized operation under lifecycle limits; it cannot start another.

## M2. Preparation, preview and admission

Keep one active operation across both directions. Repeated clicks do not create a second job.
An unresolved possible upload retains this guard across logout, credential/endpoint/account changes and restart.
Preview direction, configured normalized endpoint, approved redirect target, login label, collection/media scopes,
local committed/pending work and available remote facts. Unknown counts are unknown, not zero.
Import replaces the local collection. Export replaces the server collection and activates the prepared upload locally.
Do not run incremental normal sync to obtain a preview or make a directional operation succeed.
If later reconciliation is needed, disclose its additional local/remote effects and obtain a new explicit decision.

Ask permission for Export preparation, including the download and F1 stamp changes. Make the O5 local backup
before downloading. Save the M3 server copy even for meta.empty=true. Then read meta and show its values with
the copy's identity, type and time before asking to upload. Use this observation for later checks.
Allow known stamp changes caused by the download. If a verified file shows other changes, refresh or stop.
Keep unverified contents/counts unknown. Explain F1's race: another client may write after the copy was made;
those later edits may be lost on upload. The protocol has no transaction covering both requests.

Bind final confirmation to operation identity, direction, endpoint/account, selected scopes, source generation,
local admission sequence, fresh remote mod/scm/usn and owned artifact identities where applicable.
Admission sequence is a local guard, not a server revision or exact SQLite snapshot sequence.
Recheck these facts and eligibility at admission and exclusive entry. Changed facts require a refreshed decision;
never silently reverse direction, merge, activate a stale file or infer authority from an equal timestamp.
Refuse collection transfer while a review session is active. Drain/cancel conflicting jobs through existing lifecycle.
The upload starter cannot dispatch destructive upload before final confirmation and required durable intent.

## M3. Collection transfer, backup and activation

Import streams to owned staging, validates/opens/upgrades/finalizes it, then forces the real local safety backup
and uses verified FullDownload replacement with col.ls=col.mod. Never activate unchecked bytes.
Reuse package/open/replace primitives and their storage ownership, size bounds and finalization.
An existing damaged collection uses the specified raw safety backup; no existing file means no local file to back up.
Before local activation, record the expected source generation and candidate identity through current R6.

Apply O6.1 to every Export: retain either a verified collection-only package or the downloaded raw file clearly
marked unverified. This prerequisite also applies to zero-card and newly configured accounts. If fetching or
publishing that backup fails, refuse upload. No defined reply proves that a server file does not exist; a failed
HTTP request, login or connection cannot substitute for a backup. Preserve published recovery files after errors,
and exclude server copies from retention. O6.2 produces the consistent, transformed, compacted, finalized candidate;
send its bytes. Before acknowledged acceptance, permit only disclosed preflight maintenance on the live database.
Save exact OK durably, then perform verified FullUpload using the retained candidate. F1 permits server schema
processing and download stamp changes, so acceptance supplies no byte-equivalence guarantee. If local activation
cannot finish, keep both versions and all backups, and distinguish remote acceptance from local recovery progress.
An error does not undo prior effects on either side; report the changes already made.

## M4. Durable phases, cancellation and recovery

Persist operation identity, captured endpoint/login identity, direction/scopes, generation/sequence guards,
artifact length/digest identities, backup references, known acknowledgements, actual active generation and stage outcomes.
Represent preview/preparation, confirmed transfer intent, transferring, remote accepted, activating locally,
collection complete, media awaiting consent/incomplete/complete, complete, cancelled, failed and needs decision.
These phase names describe semantics; they do not prescribe a new shared wire or sidecar serialization.
Write intent before upload may reach the server and before local activation. Persist known OK before activation.
Write final success and its cooldown deadline in one durable state transition; failure cannot record success.
Keep required artifacts and recovery evidence outside U32's unconditional temporary sweep namespaces.
At relaunch display state and explicit Resume/Recovery actions; issue no request automatically.
Resume only the same operation with matching identity and verified retained artifacts; reauthenticate if needed.
New local work, unrelated replacement or changed remote facts requires a new decision, not stale activation.

Cancel before Import activation keeps the local file and backups. During activation, recover through R6 and
report the generation in use. If upload bytes may have arrived, keep the candidate and needs-decision state,
and prevent overlap. This applies after cancellation, lost reply and process death. Without durable OK,
matching stamps or a stopped worker cannot prove acceptance or completion. Abandon keeps this guard and result.
A later upload needs supported proof/serialization or an evidence-based recovery decision establishing that
the old request has finished; agreement to risk is insufficient. Durable OK permits local activation recovery,
not another upload. Partial downloads may restart at byte zero. No range or resumable upload is specified.

## M5. Selected media, effects and resume

Offer collection-only or “Reconcile media on this device and server”. The collection direction does not select
media direction. Before applying media pages or sending changes, ask for a separate yes covering both sides.
This choice can be part of the preview. Bind it to operation, endpoint/account, media scope and resulting
generation. Check any expected replacement before starting. Ask again if unrelated identity or scope changes.
The decision covers the described categories for all files and batches; extra per-item questions and a
one-way media adapter are unnecessary.

Disclose that either collection direction can download server files and upload pending local files, including
files unrelated to this collection's references. Server versions can replace local pending edits; old local bytes
go to trash. Server deletions can trash clean local files; pending local deletions can instead be downloaded again.
Local deletions may reach the server. Name repair/case conflicts may rename files and delete server names, without
rewriting collection fields. Completed effects survive cancellation; neither side is automatically restored.
The selected scope is all supported media handled by Y3–Y15, not an exact mirror or a frozen per-file manifest.
Empty/over-100-MiB files are excluded. Show known exclusions/reasons and unknown counts honestly.
Local edits and other clients may change work during the run; consent covers the disclosed effect categories.
Changed categories/scope require another decision. No new dry-run engine is required for this category-level consent.

Keep the completed collection stage even if media is awaiting permission, declined, cancelled, interrupted or failed.
Present each stage's result and mark the operation partial until the media work selected for it finishes.
Cancelling media does not allow changing the recorded scope afterward to obtain success or start the cooldown.
List excluded files alongside any supported-media completion; do not describe excluded files as transferred.
For collection-only, leave media alone and issue no media requests. Resume is explicit and continues media for
the same operation without uploading or replacing the collection again. Use U110's journal/change stamps and
U111's USN, pending/awaiting state, durable batches and stamp-matched acknowledgements.
Keep unknown or partial effects after a lost answer; explicit resume uses Y3–Y14 to list and reconcile again.
Y11 may retry its sanity check within the authorized operation's bounded re-run and permitted effect scope.
Run one media job at most. Replacement cancels and drains it through lifecycle, and requests hold no media lock.
Continue study using awaiting placeholders and MediaArrived as files arrive. Launch, timers, review completion
and background events never initiate or resume this server media work.

## M6. Shared success-only cooldown and identity limits

Persist last success/deadline per normalized configured endpoint plus exact trimmed login username, preserving case.
This is the logical identity, not permission to duplicate the username in ordinary durable records. Operation and
cooldown records use a stable opaque or derived identity reference; username and display login remain Keychain-backed.
Never place the raw username, host key or password in those records. The reference must preserve the logical
identity across logout and credential rotation without changing its case-sensitive equality or alias limitations.
Share the 60-minute cooldown between Import and Export. The host key, direction, redirect target, generation,
login session and credential rotation do not form a new identity or erase historical deadlines.
Logout, switching endpoint away/back and restart preserve records. Use configured identity through redirects.
This prevents reset for that configured login; no canonical id proves that different aliases/case spellings or
different configured URL aliases are the same server account. Do not claim stronger alias enforcement.
Do not infer account equivalence from host keys, lowercase usernames or merge unrelated accounts speculatively.
The unresolved-operation overlap guard is independent of these cooldown keys and cannot be bypassed by switching them.

Record 60 minutes from whole selected-scope completion, only when the receipt and deadline are durable.
Selected collection-only can finish without media; selected media must finish before whole-operation success.
Failed, cancelled, partial or ambiguous operations consume no success cooldown. Resume of the same incomplete
operation is not a new transfer; the unresolved-operation guard remains applicable independently.
Show the next eligible instant. A backward clock change cannot shorten the retained deadline.
File operations and CloudKit remain exempt. Enforcement is installation-local, not an account-wide service.

## M7. Fresh cloud-publication decision after manual Import

FullDownload used for manual Import follows R6's durable replacement handshake, not its obsolete preserve row.
Allocate a fresh persistent publication generation bound to the captured CloudKit account/configuration/environment,
or explicitly unbound for local-only use. Set cloudPublicationPending=true even if the prior generation was clear.
Before rename, record old/new closed-file length/digests, prior complete decision and candidate fence durably.
After activation, keep the imported generation pending until U113/U114's separate explicit cloud/local decision.
Old queued cloud work, callbacks and retries cannot publish or be reassigned to the imported generation.
Proven pre-rename recovery restores the old generation's exact prior-clear or prior-pending decision; ambiguous
file/account identity keeps publication paused. Post-rename recovery activates the proven candidate with its fence.
Manual Export's FullUpload activation preserves any existing cloud decision; manual/media success never clears it.
Legacy manual reconciliation/sanity bookkeeping is separate; pendingScm/timestamp mismatch cannot clear a cloud fence.
An Anki login is not a CloudKit identity or authorization. Local review does not require resolving cloud publication.

## M8. Focused acceptance and complete delivery scope

Use the pinned local official-server rig and scripted faults; never AnkiWeb. Cover path-prefix login and redirects,
both direction confirmations, no hidden incremental merge, real local backup before mutation and server-copy restore.
Include zero-card nondefault decks/config/history, fresh-account pre-copy, failed-copy refusal and post-copy stamp preview.
Compare accepted-upload logical content after declared schema/stamp transforms; do not mask arbitrary differences.
Cover lost OK/kill, post-OK local failure, partial media, explicit media-only resume, change-stamp races, exclusions,
server-wins/trash/deletion/name repair, stale identities, overlap across credential changes, durable shared cooldown,
logout/key rotation/restart, deadline clock changes and Import's fresh generation/account-bound cloud fence.
With no explicitly authorized operation running, assert launch, foreground, timers, review end and integrations
make zero Anki requests, including status/media requests.
Native Mac/iPhone/iPad evidence must show Settings configuration/login/preview, direction/media decisions, progress,
cancel/recovery/resume and truthful final per-stage results. Local server routing into the VM stays controlled.
Server/oracle evidence proves this protocol boundary, not Apple delivery/account/quota/convergence.
Collection-only may ship as a separately logged partial delivery; it cannot claim the full selected-media requirement.
Keep remaining plan units, formats/options, compatibility suites and all milestone/final gates explicit.
