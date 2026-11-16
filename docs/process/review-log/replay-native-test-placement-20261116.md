# Native replay proof placement — 2026-11-16

Starting commit: `e1f90bf10` (clean master).
Keep integration base `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
This is repair work in progress. Native replay proof remains failed.
No Unit, milestone, full gate or live sync acceptance is claimed.

## Placement and coverage

The package Mac test ran without an AppKit application or published native
window. A VM-only candidate correctly refused the host package lane before
creating a window. Independent review `cleanroom/20261116T103846Z-45614`
rejected applying that candidate there: a required host job would remain red.

Commit `de9317f8` moves the complete Mac native method into the existing
app-hosted Snapshot VM target. The package retains DOM/localization and mobile
native cases. Mac Unit alone does not run Snapshot and cannot verify this proof.
The existing broader out-of-process Mac replay UI suite is unchanged.

The moved proof retains the public AX API, exact unique window and DOM identity,
attribute error/type checks, original retained element and fresh CFEqual check,
page/window/origin and card/generation/side binding, authored q0 collision and
name, four stages across two cards, 20 exact native presses, leading-zero
commands, exactly-once counts and empty diagnostics. All original timing and
tree bounds remain. There is no DOM activation fallback.

The synthetic fixture guards VM creation/publication, owns a real styled window,
and detaches its pages and handlers on exit. The app host owns AppKit startup;
the fixture does not activate the application or request a key window.

## Independent source review and repairs

Author `cleanroom/20261116T110953Z-74909` prepared the relocation. Review
`20261116T111643Z-869` found a shared-process lifecycle race: AppVisibility and
NativeReviewStartup can change application visibility or policy across awaits.
Suite-local serialization cannot exclude them.

Fixer `20261116T112447Z-29042` added a shared asynchronous lifecycle guard
covering the replay proof, both visibility cases and each complete startup
exercise, including deferred restoration. Its regression covers failure,
cancellation, queued and handoff cancellation, restoration and later admission.
A wrapper exercises both cancellation choices without parameterized selection.
Independent review `20261116T113411Z-59998` found no P0–P2 findings. A subsequent
mechanical helper-label change passed independent review
`20261116T114214Z-72913`; the privacy guard was unchanged.
The default package WebTestHarness remains byte-identical to its baseline.

Native compilation exposed two new helper defects before executing tests.
`55c65f73` supplies an explicit continuation type; `cfba1892` replaces an
end-of-scope defer with an immediate cleanup block. Independent reviews
`20261116T114954Z-94551` and `20261116T115520Z-18536` found no required findings.
The failed build receipts `mac-hosted-test/20261116T114458Z-76739` and
`20261116T115123Z-97134` remain failures, not executed tests.

Run `mac-hosted-test/20261116T115838Z-23997` compiled but crashed. Only its exact
crash attachment was inspected, without screenshots or unrelated attachments.
Its stack showed the own-process public AX read entering AppKit/WebKit callbacks
from a detached cooperative executor. `6b962941d` keeps that read and press on
the owning UI actor. `55503e5a0` explicitly annotates the nested asynchronous
observer after the compiler rejected its inferred isolation. Independent
reviews `20261116T121117Z-45353` and `20261116T122143Z-74274` were clear.
The intermediate build `20261116T121547Z-59743` failed before test execution.

Run `mac-hosted-test/20261116T122353Z-78506` failed with a scheme-handler
cleanup exception and reached the test time limit. The public SDK documents
that assigning a second handler for the same scheme throws, including nil.
Outcome before cleanup: unknown. Commit `74687efa9` removes only those two
calls; other cleanup stays intact. Handler ownership remains with its
configuration. Independent review
`20261116T123257Z-94851` was clear.

## Executed checks and limits

| Check | Clean source and receipt | Result |
|---|---|---|
| Host package DOM/localization | `55c65f73`, `swift-test/20261116T115558Z-20345` | Pass: one test, three platform cases; zero skips. No native Mac proof. |
| Shared lifecycle exclusion/restoration | `cfba1892`, `mac-hosted-test/20261116T120300Z-31378` | Pass: one selected test, zero skips. |
| Visible application | `cfba1892`, `20261116T120526Z-33703` | Pass: one selected test, zero skips. |
| Hide/unhide | `cfba1892`, `20261116T120908Z-38146` | Pass: one selected test, zero skips. |
| Review startup lifecycle | `cfba1892`, `20261116T121049Z-43542` | Pass: one selected test covering six scenarios, zero skips. |
| Accepted answer with held preparation, pause/close | `cfba1892`, `20261116T121338Z-54032` | Pass: one selected test covering both scenarios, zero skips. |
| Complete moved native replay proof | `74687efa9`, `20261116T123521Z-3449` | Fail: one executed test, zero passes/skips. |

The startup checks use the real renderer/model with a synthetic Core client;
they do not establish a real collection or peer round trip. Later changes affect
only the native replay reader/fixture. Broader affected suites remain unverified.

The latest replay result reports AXChildren error -25204 with a nil value and
process trust false. It no longer reports the earlier cleanup exception.
These observations do not establish that permission is the cause or that the
entire actor correction is verified. Localize the precise owned read before
changing it; another unchanged run would not establish the missing fact.
The original mobile native failures remain open; added passive diagnostics have
not yet been exercised in those lanes.

Privacy, static and similarity checks passed for each applied source correction.
For the final fixture correction: `private/20261116T123351Z-97519`,
`static-checks/20261116T123348Z-96898`, and
`similarity/20261116T123348Z-96899`. The filtered similarity view reports one
file scanned, zero flags, zero allowlisted and zero out of scope. Only that
filtered view was read. The earlier rejected helper labels remain documented
in their failed privacy receipt; no privacy exception was added.

All required failures remain in the verification register. Focused passes do
not clear the failed integration gate or the five review lifecycle failures.

Independent evidence review `cleanroom/20261116T124119Z-22719` found no
required findings. Exported document content matches the seed. The review makes
no additional verification claim.
