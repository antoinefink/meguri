# Retryable fetched asset across an actual host reopen

Role: implementer. Session: `s-20261025T134357Z-235af9a2`.
Source start supplied by orchestrator: `5875adc7`.
Original U113 start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
Integration batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Contract review supplied by orchestrator: spec-writer
`s-20261101T113954Z-77a58a2a` (independently reviewed consistent).

Requirements: `sync/policy#P24`, `sync/policy#P4`,
`sync/cloudkit-records#K13.1`, `sync/cloudkit-records#K22.2`,
`conformance/cloudkit-acceptance#Q11`.

Test first: added the nested Rust/FFI test
`five_failed_drain_retries_then_actual_reopen_first_drain_merges_and_service_acks_local_edit`.
Production implementation, exports, event variants and Swift are unchanged.
The existing test bodies and PublishedFixture implementation are unchanged.

The test publishes and acknowledges a Note through the Service. An engine update
changes one field and tracking waits for its returned sequence. The remote
conditional save changes a different field and carries a payload asset.
Real Service FetchZone pages travel between begin/will/zone/did/finish callbacks.
Policy output, rather than a manually requested retry, provides each retained
native execution/read pass. Only native asset file delivery is failed.

Assertions cover five charged failures, original inbox identity/version and
pending payload/base/mark/ACK preservation, no immediate or duplicate drain
action, an empty sixth selection, and assetUnavailable attention. The actual
host closes and opens its existing files with the same clock/random/provider
scope and retained Service. No keys, baseline, ACK, budget or tracking state
are seeded at reopen. A new policy-issued StartEngine registers the new SDK;
actual Service root pages establish that instance's root before its first
collection drain. That drain must issue the successful same-version retry.

Successful intake must merge physical fields through the normal incoming writer,
with the original entry id in the apply run and a matching engine/file WAL
receipt covering that run's actual operation sequence. The pending merged note
then goes through a real batch and conditional Service Saved callback; exact
payload digest/asset bytes/system fields and cleared pending state are checked.
A second actual reopen checks durable row/ACK and unrelated entity preservation.

The source author ran no compiler or runtime test. Root must build the code,
execute the workflow and check the final evidence. Source inspection alone
establishes no production failure or acceptance.
Formatting is also pending: the source-only rustfmt edit could not run because
this filtered environment has no `rustfmt` executable (exit 127).

This is only a core/FFI native-port proof candidate. It does not establish live
Apple SDK, Swift coordinator dispatch or UI acceptance. U113/U114, P24/Q11,
milestones and gates remain open; this packet accepts none of them.

## First actual execution

The seven existing asset-read FFI cases pass in
`core-test/20261101T130630Z-70356`. The new test compiled in
`core-test/20261101T131133Z-71957` and failed at the reopened StartEngine
expectation. Its five failed reads, cap, close and persisted reopen assertions
preceded that failure successfully. It has not verified the successful retry.

Reviewer native_conformance_lifecycle_audit identified a test setup omission.
Invoke `workflow.prepare_runtime()` before reading issued commands, as the app
does. Keep the first fetch untouched and verify that no retry was admitted.
The production path needs no change for this failure.

## Startup preparation fixture correction

Fixer session `s-20261025T162405Z-9dc656e4`; original U113 start
`1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`, batch base
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.
The helper now invokes `workflow.prepare_runtime()` at startup. It verifies
unchanged inbox contents, pending Note data and zero charged reads. Service logs
must contain no new collection fetch or read before the test's first drain.
The test still requires the issued startup handle and all later merge and save
checks. The existing callback reentry probes remain enabled.

One source consistency check only. Compile/test: UNRUN; root owns focused
verification, independent review and integration gates. No acceptance claimed.

Root result: `core-test/20261101T131902Z-75815` PASS1/1 after the setup fix.
The full five-failure/reopen/first-drain/merge/Service-ACK path now passes.
Independent review found no P0–P2 in the correction. The native Swift layer and
remaining required regression checks are still separate obligations.
