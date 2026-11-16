# Native capture repairs and replay observations — 2026-11-16

Production code: `fefef71ad` (unchanged). The four changes affect tests.
The full integration gate remains failed.

## Mac capture repairs

Both original Mac audit crashes stop inside XCUIElement.screenshot, at
CGS_REQUIRE_INIT, before image conversion, luminance sampling or native audits.
The driver resize case and four example scripts have the same crash.
Image decoding cannot repair that earlier failure.

| Source | Start | Author / independent review | Change |
|---|---|---|---|
| `a49bbf3d` | `0f1b8096a` | cleanroom 130145-70608 / 130308-72142, clear | Initialize NSApplication.shared before the Mac audit screenshot. |
| `102b4fc1` | `a49bbf3d` | cleanroom 134915-79833 / 135115-98857, clear | Initialize NSApplication.shared inside the Mac driver capture boundary. |

All run prefixes in these short references are `20261116T`. The driver commit
message names review 82495 by mistake; the actual independent review is 98857.
Both exported patches matched their reviewed seeds. Native window capture,
foreground/privacy guards, image sampling, audit variants, thresholds, assertions,
coordinates and deadlines remain unchanged. Production and mobile code are unchanged.

Privacy/static/similarity receipts pass respectively:

- Audit: 133906-13772, 133906-13773, 133906-13807.
- Driver: 135301-4706, 135301-4660, 135302-5200.

Each filtered similarity report covers one file with zero flags or exceptions.

| Native check | Clean source | Receipt | Result |
|---|---|---|---|
| Original sample audit, all configured variants | `a49bbf3d` | a11y/20261116T134047Z-19963 | 1 passed, 0 failed, 0 skipped |
| Original card-surface audit | `a49bbf3d` | ui-test/20261116T134259Z-23685 | 1 passed, 0 failed, 0 skipped |
| Driver resize and four example scripts | `102b4fc1` | mac-ui/20261116T135350Z-6169 | 5 suites passed, 0 failed, 0 skipped |

One inspected synthetic capture still had old crash dialogs over the app.
After all jobs ended, normal termination of only the VM user's notification
process cleared those dialogs. No saved permission, report file or app data changed.
The subsequent `ui-tree/20261116T140220Z-46454` passes both checks. Its inspected
sample-window capture is unobscured. This establishes capture readiness, not
production design agreement or live sync acceptance.

The original replay export has a distinct AXWindows -25211 failure. Read-only,
prompt-free `VMAccessibilitySetupTests/testCheckRunnerAccessibilityTrust` in
`ui-test/20261116T140614Z-53659` confirms false trust: 1 failed, no passes/skips.
Its temporary opt-in files were nonce-owned and removed afterward. No permission
was enabled or refreshed. The separate app-hosted AXChildren -25204 failure at
root/0/0 remains unresolved; false runner trust does not prove its cause.

## Bounded mobile observation

The original zero-link snapshot did not prove a missing scene: the fixture already
used UIWindow(windowScene:). On clean `102b4fc1`, iPhone receipt
`unit-sim/20261116T141510Z-68889` shows one WKWebView object, no native links,
and an attached, visible window in a foreground-active scene.

`47d50adb` starts at `102b4fc1`. Author cleanroom 142109-87016 and independent
review 142321-96971 add concrete UIKit subview traversal, respecting hidden views
and accessibilityElementsHidden. Object identity deduplication and native
accessibility-container reads remain. The commit's D4 citation is mistaken:
the reference is `render/display-transform#D4`, with `render/card-behaviors#B6`.
Privacy 142530-98938, static 142530-98851 and similarity 142531-99520 pass.
The filtered similarity report has one file, no flags or exceptions.
The next iPhone run, `unit-sim/20261116T142619Z-702`, still finds zero links.

`7b2cfc6c` starts at `47d50adb`. Author 143057-16786, review 143547-24624,
fixer 144145-35158 and final independent review 144724-50426 retain one existing
10 s publication budget. The fix rejects a hierarchy capture completed after
that deadline. Only the native link count is polled; bad identifiers, names,
assertions and activation results are never retried. Privacy 145318-61553,
static 145321-61908 and similarity 145321-61843 pass. The filtered similarity
report has one file, no flags or exceptions.

On clean `7b2cfc6c`, iPhone `unit-sim/20261116T145733Z-70590` and iPad
`unit-sim/20261116T150024Z-74273` each report 1 passed method, 1 failed method
and zero skips. DOM/localization passes; native publication fails at its deadline.
Both final snapshots contain 20 real UIKit objects and zero native links in a
valid attached, visible foreground scene. No native activation is reached.
The required 3/7/3/7 counts, exact identifiers and localized names, 20 actual
activations, command cardinality and suite limit remain unchanged.

Stop speculative window or assistive-setting changes here. Establish a valid
native accessibility measurement before another repair. These runs do not prove
a production rendering defect, a native replay pass or mobile suite acceptance.

## Ordinary review sync scope

Read-only source tracing finds the normal review, durable card/revlog capture,
send/ACK and incoming/reopen path connected. Initial Settings publication and
adoption also reach binding, account revalidation and fresh root checks.
The later K19 structural generation pipeline is separate. attention_known=false
blocks UpToDate status; it does not block ordinary sending.

`core-test/20261116T150641Z-87303` on clean `7b2cfc6c` runs
cloud_review_roundtrip and cloud_incoming_review: 7 passed, 0 failed/skipped.
That includes five substantive scenarios and two child-entry harness methods.
These controlled Rust checks cover saved reviews, incoming rows, reopen,
explicit sync after reopen and durable ACK bookkeeping. They are not a real
CloudKit or two-device round trip. Native publication, peer adoption, cold reopen
and review/media convergence remain the next practical proof.

All earlier failed receipts remain. No Unit, milestone, full-gate or release
acceptance is granted by this record.

Independent evidence review `cleanroom/20261116T153552Z-27553` is clear
against permitted source and the supplied receipt summaries. The filtered copy
cannot inspect raw receipts or the historical Docker inventory; the integrator
retains responsibility for those checks. No source change was requested.
