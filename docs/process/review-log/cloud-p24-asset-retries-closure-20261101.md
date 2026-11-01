# P24 asset retry prerequisite — 2026-11-01

Closed: the K13.1 asset retries / K10.3 inbox retry count / P4 `RetryAssetEntries` item. This spec edit removes only that P24 bullet. No normative rule changes.

- Role: spec-writer; session `s-20261101T113954Z-77a58a2a`.
- Edit start: clean master `94f970405fdf4e24fd6c20b920bd0d74c077013e`.
- Original integration batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
- Authority: `sync/policy#P24`, `conformance/cloudkit-acceptance#Q11`, and its cited `sync/cloudkit-records#K22.2` asset case.
- Audit used Meguri specifications, source and sanitized receipts. No restricted source or raw guard report was opened. No tests were rerun.

## Clause evidence

The Rust tests below are in `core/crates/meguri-sync/tests/cloud_incoming_intents.rs` and pass in the retained 420-test run.

| Requirement | Test and checked facts |
|---|---|
| Same-`erev` retry applies | `ordinary_asset_same_version_reread_recovers_through_intent_writer_wal_and_ack`: invalid downloads leave rows and acknowledgements unchanged; a valid retry settles the same inbox id/version, applies through an intent and matching WAL receipt, and retains the acknowledgement after reopening. |
| Save-conflict first read applies and pending content is sent | `save_conflict_first_same_version_read_applies_and_pending_merge_is_sent_after`: a real conditional save conflict grants the first read; valid same-version bytes apply; the local merge remains pending and subsequently reaches the Service. `save_conflict_first_asset_read_is_granted_by_a_real_handed_result_and_is_separate` checks that this read is distinct from the five retry attempts. |
| A late entry has its own five attempts | `asset_retries_count_per_entry_including_a_late_entry_after_fifth_pass`: after one entry exhausts five passes, a second entry is still selected five times while the first stays exhausted. This is core selection evidence, not a native late-entry test. |
| Reopening resets even a partial count | `asset_count_three_at_close_gets_five_reads_on_next_actual_open`: three failed attempts, actual close/open, reset to zero, then five further attempts. |
| Once per drain, five per open, then first-drain recovery | `realServiceAssetInboxAutomaticallyRetriesOncePerDrainAndResetsOnReopen`, in `app/Packages/MeguriKit/Tests/AppModelTests/CloudServiceAssetRetryTests.swift:11`: five registered SDK collection fetches yield five actual drains, production coordinator commands and failed reads; a sixth drain and completed close yield no sixth read. The same paths reopen with a new SDK instance, reset count and unchanged pending data. The first collection drain executes the successful retry automatically. |
| No apply without the asset; merge, upload and persisted acknowledgement | The same Swift test checks unchanged local fields and acknowledgements through all failures; accepted same-version asset intake; a WAL-covered incoming merge preserving the local field and remote field; a real conditional Save using the remote system fields; the Service Saved receipt and matching clean tracking acknowledgement; and the merged content, unrelated Note and media after another open. A read alone is explicitly not an acknowledgement. |

The native test calls SDK fetch, not a retry or issued-command API. `ServiceAppBackend.execute` delegates to the production backend and only records observations. Its wire fault withholds one asset and fails five downloads for that exact Service version. The Service retains the actual remote record throughout. The FFI test below separately checks original inbox identity, native-file/generation WAL coverage and the final saved payload.

## Retained runs and source limits

- `build/mg/core-test/20261101T134745Z-91787/result.json`: clean `4fa09dad1b6eca37355ead52bf75a260b5d3aec9`, Docker, 420 passed, zero failed/skipped. Selection covers CloudKit tests in `meguri-sync` and `meguri-host`; it includes every Rust test named above.
- `build/mg/swift-test/20261101T144749Z-20763/result.json`: clean `18ac384afc5993e0b0ce619f17725e995f395eeb`, 2 passed. Both the automatic asset test and `realServiceReviewAndRegisteredMediaSurviveReopenAndFreshAdoption` pass. The result retains the synthetic fixture hash, published Service helper hash and core artifact stamp. Production source for this proof remains unchanged through `71901282`; subsequent differences concern test layout and documentation.
- `build/mg/core-test/20261101T131902Z-75815/result.json`: 1 passed, the FFI test `five_failed_drain_retries_then_actual_reopen_first_drain_merges_and_service_acks_local_edit`. It reports dirty inputs and is corroboration only, not a clean-commit receipt. It directly executes issued FFI commands; the later Swift test supplies automatic coordinator execution.

The rules agree across K13.1 and P4: retry budget belongs to each entry, resets on every open, and excludes the first save-conflict read; same-version retry results settle in place. Production `policy_runtime.rs` derives retryable work from each entry's count, and `policy/transition.rs` emits the action on a qualifying collection drain. Combined core selection tests and native drain execution cover the late-entry rule without claiming a separate native late-entry run.

## Wider failures remain open

`build/mg/swift-test/20261101T150338Z-27960/result.json` remains failed: 19 of 23 passed, four failed, with one package warning. The two named full Service workflows pass in its log as well. That result is not a passing suite receipt.

Pending work: verify normal close for original commands and required `UpdatePending` outputs. The failed receipt retains four cases needing fixes. Another six scheduler cases need a fixture that starts the policy correctly. All of this work remains required outside the present prerequisite.

This closes only the asset retry prerequisite. Resume V9.2, other P24 items, U113/U114, the original batch gate and milestone acceptance remain open. The native test uses a registered fake SDK with the real Service and production coordinator; it is not a signed CloudKit run or device evidence. No live SDK assumption, rendered UI result, human approval or trial is inferred.
