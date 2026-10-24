# Controlled native transfer rig

The integrator stages an ignored, mode-0600 `controlled-rig.json` in the test
VM through `tools/mg manual-transfer-check --platform mac|iphone|ipad`.
Use only the pinned, checkout-owned loopback sync rig and reserved synthetic
accounts A/C and B/D. Never use real credentials or a collection fixture launch.
Keep the manifest out of logs and attachments; cleanup removes it before the
forward and leases are released.

The manifest supplies `endpoint`, A's `username`/`password`, B's distinct
`exportUsername`/`exportPassword`, generated `deckID`, the three expected media
SHA256 values, `wireToken` and `nativeContext`. The controller also supplies its
fixed support-path seam. The helper generates these fields; do not hand-edit
account identities, receipts, policies, media files or clocks. Credentials enter
normal Settings controls and the login key stays in device-only Keychain.

Select exactly
`ManualTransferJourneyTests/testSameLoginReviewCycleThenCrossAccountMediaRecovery`.
Require one pass and zero failures/skips in both xcresult summary and case tree.
Missing hooks, manifest, VM route or independent peer proof cannot pass.

The fixed three-stage proof uses system time throughout:

1. A enters the exact URL and credentials and presses **Log In** directly.
   It never presses Save Endpoint. Login sends no collection or media transfer.
   A prepares/confirms Import with selected media. The observer holds the first
   real media payload; Cancel drains to a durable partial with no operation
   receipt and unchanged historical account state. Restart/history send zero
   requests. Explicit Resume requires fresh consent on the same operation and
   sends no collection again. Actual media is server4096 down/device6144 up;
   the zero-byte file is excluded.
2. Save an actual Easy answer, restart, and prove its persistence and A's stored
   login without passive requests. With no logout, relogin, endpoint edit, clock
   or policy edit, prepare a fresh backup/confirmation and Export back to A at
   that same URL. Its distinct operation completes before one real hour after
   Import. Fresh media consent is mandatory; unchanged same-owner media has
   exactly zero downloads/uploads. Server4096/device6144 bytes remain exact.
   This stage has no held payload or cancellation.
3. Log out and sign into B at the unchanged URL. Its separate Export preserves
   positive media coverage: server8192 down/device6144 up, an actual held request,
   Cancel/drain/durable partial, passive restart, fresh-consent same-operation
   Resume, and no collection resend. Preserve unknown prior effects and the
   zero-file exclusion.

New custom-server success receipts have `next_eligible_at == completed_at`:
there is no wait between explicit operations. Historical future receipts and
per-account maps remain readable and unchanged. A partial has no own receipt;
its historical map value is compared with the captured baseline, including a
nonnull legacy value. A whole success is proved by its own receipt.

The source reader retains strict schemas, frozen context/inode binding and
reserved-media bootstrap guards. The reducer retains Import, A Export and B
Export independently, rejecting missing, intervening, unfinished or wrong-account
A exports. No account/endpoint values leave the VM in proof summaries.

The data helper independently downloads completed A into `peer-a` and compares
its saved review and scheduling rows with the independent Easy replay. B retains
its separate peer download, all twelve logical-table comparisons against the
activated Export, replay comparison and exact app/server media bytes. UI text,
screenshots or B's success cannot substitute for A's peer proof. Root owns
compilation, focused checks, VM execution, independent review, full gates and
final human evidence; this source change claims none of them.
