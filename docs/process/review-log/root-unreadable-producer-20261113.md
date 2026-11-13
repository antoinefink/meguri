# Root readability: R3 handoff

Fixer uses the implementer role. Canonical start:
`f059b874ac54567d73b79d0aedf559d5b812c22f`.
Root session: `s-20261110T214025Z-20fa15fd`.
Pending batch begins at `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
The filtered checkout's synthetic HEAD cannot verify canonical equivalence.
The supplied prepared patch is retained; this pass addresses R2's two P2s
and the reported privacy wording. Root applies the export and verifies it.

Contracts: `sync/policy#P3`, `#P14.4`;
`sync/cloudkit-records#K10.3`, `#K13.1`;
`sync/cloudkit-recovery#V10.2`, `#V26.1`, `#V16`.
See `cloud-status-k10-producer-map-20261112.md` for outstanding producers.

## Storage and production behavior

Publication V4 introduces `cloud_root_readability` for ordinary bindings.
Its new, unshipped representation now uses non-null TEXT `epoch_knowledge`:
`unknown`, `complete`, or `incomplete`. This describes the latest accepted
Epoch observation, independently of historical qualified crt. `status` is
`unknown`, `readable`, or `unreadable`; only unreadable rows hold a nonempty
BLOB Active version. SQL constraints and open validation reject bad values,
owners, missing columns/tables, inconsistent markers and unsupported versions.
Readable status requires complete Epoch knowledge. V1–V3 install an empty
ledger without deriving proof from legacy state. Runtime remains V7.

Active bytes/version remain in the existing adapter store. Readable matching
Active with unknown Epoch knowledge preserves both that knowledge and any
mark. Bad Active sets its actual version even with unknown Epoch knowledge.
Observed bad, missing, mismatched, unsupported or publishing Epoch records
incomplete knowledge. Only readable matching Active plus latest complete
Epoch clears the mark. A competing Active belongs to epoch-switch isolation;
its body supplies no readability claim for the bound epoch.

Ordinary SDK Active/Epoch intake commits readability with adapter/inbox/crt
state. Non-SDK Epoch intake uses its existing transaction. Issued reads keep
read ownership, existence, root confirmation and absence authority separate.
Unreadable Found Active retains its Service version without confirmation;
missing Epoch records incomplete knowledge with the existing loss obligation.
Root ACK/isolation, validated adoption, actual publication Epoch ACK and
publication switch evaluate the same ledger in their existing transactions.
Adoption grants no SDK confirmation. Explicit Stop ends this warning.
Removed binding work takes its ledger row with it. Account changes, Pause
and ordinary engine activity preserve the warning.
Policy projects the ordinary binding's mark under State then tracking access.
Other K10 work is unfinished, so `attention_known=false` remains intentional.
Native conversion retains routed typed controls missing encrypted fields;
malformed routing/types and prohibited assets still refuse. No ABI/UI change.

## Reviews and bounded repairs

R1: two P2s, no P0/P1. Non-SDK Active decoded before acceptance, and non-SDK
Epoch lacked physical revalidation. R2 repaired both using `with_scan_file`:
file/sidecar reads precede State; retained scan/caller ownership is repeated
before tracking access. Invalid Active and its version/mark commit together.
Public-open coverage retains the source writer/WAL and compares DB/WAL/SHM
bytes and filenames. Private probe copies alone receive writable modes.
Read-only fixtures do not prove permission enforcement for privileged users.
Two successor fixtures now retire, release, then register. Production SDK
lifecycle behavior is unchanged.

Independent R2: two P2s, no P0/P1; R1's two non-SDK defects closed in source.
First, absent Epoch proof was conflated with observed incomplete proof.
R3 introduces the explicit knowledge above. Migration tests continue through
Active-only delivery and retained Active confirmation, bad Active, subsequent
incomplete/complete Epoch bodies and a later bad Epoch despite qualified crt.
SQL tests refuse NULL and unproven readable status; validation also rejects
bad rows written with CHECK enforcement disabled.
Second, ordinary SDK intake repeated cached validity without physical proof.
R3 pins the original ScanGuard for Active/Epoch observations, calls
`with_scan_file`, then repeats original SDK, adapter and control guards under
State before the existing transaction. Entity-only/no-state handling, payload
asset admission, callbacks, root confirmation and scheduling retain their
existing paths. No recursive State or tracking lock was introduced.
Host coverage refuses ordinary SDK Active and Epoch setting/clearing after
file replacement or real engine-generated publication replacement. Epoch
replacement happens at the existing inbox hook after scan ownership capture.
Assertions preserve mark, qualified crt, inbox, serialization and root ACK.
Existing stale account/SDK/read and non-SDK cases remain enabled.

## Receipts supplied by Root

These are executed Root receipts, not checks run by this fixer.

- R1 `core-test/20261113T123244Z-83925`: 61 selected, 58 passed, 3 failed.
  Two failures were the successor fixture lifecycle omissions repaired in R2.
- The third was
  `epoch_crt_gate_committed_media_trash_settles_after_mismatch_with_real_intent_and_wal`:
  `last_seq` changed from `Seq(11)` to `Seq(12)` during drain (then line 843).
  Its exact assertion and enabled test remain; the cause is unresolved.
- `core-test/20261113T123510Z-85718`: clean canonical, isolated case, PASS 1.
- `core-test/20261113T123542Z-86137`: R1 patch, isolated case, PASS 1.
- `core-test/20261113T123659Z-86528`: clean canonical, combined selection,
  PASS 51. None of these passes establishes the cause of the failed run.
- R1 `swift-test/20261113T123253Z-84260`: PASS 13 native methods, including
  payload conversion and retained no-state/error regressions.
- Clean `f059b874`, `swift-test/20261113T122823Z-79940`: the two retained
  native ownership methods remain red; separate from the root feature.
- R2 `core-test/20261113T125331Z-2750`: PASS 65 selected, including the
  unchanged media sequence assertion. This does not resolve R1's failure.
- R2 `swift-test/20261113T125336Z-3053`: PASS 13.
- R2 `core-lint/20261113T125435Z-4741`: PASS 3.
- R2 `static-checks/20261113T125439Z-5034`: PASS 3.
- R2 `similarity/20261113T125456Z-5560`: PASS, 26 source files,
  2 excluded documents, zero exemptions.
- `private/20261113T125457Z-5710`: 28 files scanned; only the Active-decoder
  comment and this log flagged. R3 rewrites that full comment and replaces
  the long log with this handoff; guards and scan exceptions are unchanged.

Prior source inspection identified queued writer work as a possible sequence
producer, but no receipt identifies the operation owning the observed advance.
That possibility supplies neither attribution nor a repair. No additional
media investigation, instrumentation or assertion change belongs to R3.

## Verification still owned by Root

The filtered R3 pass checked the source and whitespace. Root owns runtime
verification; earlier receipts alone do not verify R3. Focused commands:

```sh
tools/mg core-test -- -E 'package(meguri-sync) & (test(root_unreadable) | test(epoch_crt))'
tools/mg core-test -- --test cloud_publication_phase -E 'package(meguri-host) & (test(root_unreadable::) | test(epoch_crt_knowledge::) | test(epoch_crt_gate::))'
tools/mg core-test -- --test cloud_adoption_guards -E 'package(meguri-host)'
tools/mg core-test -- -E 'package(meguri-sync) & (test(open::tests::) | test(conflict_expiry) | test(workflow_store_tests))'
tools/mg swift-test --package MeguriCore -- --filter NativeCloudPayloadAssetTests
```

Root must obtain fresh focused checks, affected bridge/build evidence,
independent review and provenance/privacy/similarity checks, followed by the
required coherent batch gate from the recorded original base. Media sequence
failure and native ownership failures remain open. No Unit/P24/milestone
acceptance is claimed. Synthetic controls and automated fixtures do not prove
live peer compatibility, CloudKit reset/decryption/timing, signed device or
manual reference behavior. Full gate and final-session evidence remain due.

## Root checkpoint

Independent runs: R1 `cleanroom/20261113T123217Z-83385`, R2
`20261113T125241Z-2166`, R3 `20261113T130525Z-25971`.
R3 found no P0/P1. Its remaining P2 is deferred: known collection-zone loss
can clear this warning using retained complete knowledge. The binding stays
isolated with sending blocked; completeness stays unknown. A follow-up must
invalidate readability on that loss and test its owned Active follow-up.
This checkpoint does not claim complete root conformance.

R3 `core-test/20261113T130615Z-27612` passes all 68 selected cases;
`swift-test/20261113T130618Z-27899` passes all 13 native methods.
The earlier sequence failure remains unexplained, with its assertion intact.
`core-lint/20261113T130745Z-33509` rejected one nested-if style rule.
Filtered `cleanroom/20261113T131034Z-36982` changed only that let-chain.
Root verified all other file hashes and the exact reverse transformation.
`core-lint/20261113T131244Z-43117` passes 3/3. The behavioral checks cover
the unchanged logic; final source/document checks remain separate receipts.

Final static `static-checks/20261113T131425Z-46554` passes 3/3.
Similarity `20261113T130746Z-33764` covers the 26 R3 source files;
`20261113T131422Z-46218` covers the one final style delta. Both pass
with zero exemptions. The final staged privacy receipt is in commit metadata.
