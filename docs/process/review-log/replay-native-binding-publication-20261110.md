# Replay native binding publication — 2026-11-10

- Role: recorded implementer, filtered source-only session. Starting baseline: `9ff06124`; original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9` (provided by orchestrator; no git access).
- Input evidence supplied by orchestrator: actual VM 80949 built and executed, then failed at first-stage “Native replay host binding unavailable”, before AX presses or attachments. Independent source/spec review identified these D4/D12 implementation errors; no contract change or waiver is requested.
- Fixture correction: a question requires both the existing matching-current-update `shown` callback and `pair.onCommitted` for the current question page/update, in either order. The observable completion marker changes after actual native presentation completion. An answer records completion only after successful `showAnswer()` (matching shown hooks and host-context publication), followed by current pair/page/card/generation/side/shown/live-binding rechecks. Operations clear completion; transitions also clear shown. A checkpoint restores prior completion only when the same page/update and shown remain current. Stop clears both facts.
- Every ready/report evaluation requires a live guarded binding. The report reads it once and uses that same value for readiness and metadata encoding; it keeps no cached binding or fallback.
- Metadata correction only: replace `settled` with the current update's actual `shown` phase. Retain `behaviorAvailable`, `hostDOMKnown` (including no pending native presentation), committed-root document equality, exact root URL, current update/payload, native page identifier and side-specific trusted ownership IDs; also require payload card ID to match the update. No runtime, bridge, admission, rendering or settled signals change.
- Added focused CardRendering source coverage: unpresented/presentation-pending binding rejection; completed question presentation still awaiting shown; matching question binding; answer transition rejecting stale question readiness; successful shown answer binding while `settled` remains false; host-context republishing retaining that binding; close invalidating it. This uses existing real pair completion/shown seams. App fixture callback-order/observable publication and checkpoint behavior still require root's fixture execution.
- Verification limit: one source consistency check only, recorded in the handoff. No compilation, test execution, native AX proof, Unit pass, visual agreement or device result is claimed. Root owns independent review and all build/test/provenance/privacy/similarity verification. Existing twenty native presses, hide/restore checks, four stages, ninety captures, naming/ruby/geometry/keyboard/mobile checks, AX tree requirements and readiness budgets are unchanged. The separate package own-process AX-children/TCC issue is untouched. No forbidden sources, private data, raw logs, network, git or UI were accessed.

Root R1 review is clear. The lifecycle test passed in
`swift-test/20261110T114138Z-14094`. Its manual Core/Service variant completed;
the automatic variant failed before enqueue at invocation reporting.
`mac-ui/20261110T114404Z-19113` built and ran one VM case. Metadata publication
passed its former boundary; AXWindows failed with `-25211` (API disabled).
The native twenty-press proof remains incomplete. Consent is reserved by
`docs/process/one-time-setup.md#uitest-automation`; defer that step to final setup.
Injected wait tests passed all nine cases in
`ui-test/20261110T114915Z-34928`, with zero skips. Privacy and similarity passed.
