# U162 — Offline package proof

Supplemental start: `7b35de76`. Original integration base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.

## Bounded diagnostics

Filtered exports `cleanroom/20261026T084508Z-93203` and
`cleanroom/20261026T085821Z-2515` change only the verifier and its regression.
Independent R1 and R2 are clear. Exact comparisons, bounds, collation refusal,
cleanup and successful outputs stay unchanged. Failures identify fixed schema
positions and reasons. Only mismatched keys gain UTF-8 SHA-256 fingerprints;
raw keys, values, SQL and exception text remain absent.

| Check | Receipt | Result |
|---|---|---|
| Focused Docker regression | `selftest/20261026T084917Z-96389` | Pass |
| Final regression, including key digests and redaction | `selftest/20261026T090114Z-4704` | Pass |
| Final two-file privacy scan | `private/20261026T090114Z-4936` | Pass |

## File proof remains open

Original `package-interop-check/20261026T002734Z-14327` fails before a client opens.
New runs `20261026T085131Z-98116` and `20261026T090245Z-6336` retain that failure.
The first difference is a config-table key at table 11, row 12.

Digest matches against public Meguri constants identify expected `newSpread`
and decoded `localOffset`. The generated baseline omits `localOffset`.
The native day query must add it at the pinned non-UTC clock (day-clock D5).
Fixture preparation needs correction before launch; no comparison is waived.
Further differences remain unproven. This is active offline file compatibility,
not frozen server work. No Unit trailer or full gate acceptance is claimed.

Final tooling similarity `20261026T090115Z-5133` passes for both files.

## Prepared baseline and fresh Mac proof

Start `f60a1cf9`; implementation checkpoint `8f034ed5`. F11.1 prepares a fresh
run-owned fixture before launch and preserves its generated parent. Docker checks
the single clock-config write. Native capture and package verification use this
same frozen copy; exact comparisons remain enabled.

Filtered implementation `cleanroom/20261026T092045Z-25654` and test-only fixer
`cleanroom/20261026T095059Z-54307` retain exports. Independent R1 found two test
setup defects: overlapping expected/output directories and duplicate decoding.
R2 closes both without changing production guards or assertions. Reports:
`/tmp/meguri-u162-prepared-fixture-review-r{1,2}.json`.

| Check | Receipt | Result |
|---|---|---|
| Native preparation/capture wiring | `selftest/20261026T093740Z-39201` | Pass, 14 checks |
| Original clock/host regression | `selftest/20261026T093848Z-40536` | Fail; test setup corrected above |
| Final clock/host regression | `selftest/20261026T095208Z-56016` | Pass, 2 checks |
| Six-file privacy | `private/20261026T093847Z-40340` | Pass |
| Six-file similarity | `similarity/20261026T093845Z-40040` | Pass |
| Final test privacy | `private/20261026T095232Z-56893` | Pass |
| Actual Mac native export | `ui-test/20261026T094106Z-41714` | Pass, no skips |
| Independent fresh package proof | `package-interop-check/20261026T094529Z-47013` | Pass |

The fresh package matches 12 tables, 2,425 rows and all 270 media files
(4,650,232 bytes), before client opening. Stamp edits and client-open edits are
zero; timing also matches. Historical failures above remain evidence. This closes
the bounded Mac proof failure, not U162. Mobile/native intake, remaining unit
requirements and the original-base integration gate remain outstanding. No Unit
trailer or release acceptance is claimed.

Final repaired-test similarity `similarity/20261026T095230Z-56684` passes.

## Mobile native Files selector repair

Start `51e07b93`. Retained synthetic failure `ui-test/20261026T011056Z-33684`
shows one focused native Files TextView; the test searched TextFields.
Filtered fixer `cleanroom/20261026T101505Z-87607` uses the exact observed local
root and editor identifiers, with uniqueness and existing state/actionability
checks. The modal TextField path and nonce, Cancel, Save, Replace and destination
assertions remain unchanged. Independent R1 is clear; privacy
`20261026T101851Z-91646` and similarity `20261026T101852Z-91828` pass.

Actual iPhone execution and iPad execution remain unrun for this repair.
Original-base gate `20261026T095453Z-62360` was cancelled after a separate active
review-durability failure; it is incomplete. See `C8-isolation.md`.
No mobile pass, Unit acceptance or gate acceptance is claimed.

On clean `0bc98afa`, UI lint `20261026T102715Z-96947` passes 128 checks with
zero findings; static checks `20261026T102714Z-96936` pass all three checks.
These source checks do not supply native mobile execution evidence.

## Fresh mobile results

On clean `d5a0db5a`, iPhone native Cancel/Save/Replace
`ui-test/20261026T123937Z-10988` passes one test, zero skips. Independent
`package-interop-check/20261026T124442Z-16972` passes against that run's frozen
prepared source: 12 tables, 2,425 rows, 270 media files, zero stamp edits and zero
client-open edits. Timing matches. The actual destination file is retained.

iPad `ui-test/20261026T124445Z-17228` fails one test, zero skips. New Folder was
tapped; the following naming-state query failed with an AX waiter interruption.
No native destination or package pass is claimed. Its earlier picker attachment
does not show the failed naming state. Diagnose that state before repair.

The checkpoint audit also identifies an unproved chain: media-bearing file
import, a new review, retained answers after relaunch, then re-export. Existing
native review-import tests use a collection-only package; the media-export test
launches a fixture directly. These separate passes do not establish the combined
workflow. This gap, iPad export and the original-base gate remain open.

## Bounded iPad naming-state diagnostic

Start `9b265083`. Filtered fixer `cleanroom/20261026T130424Z-38175` adds one
iPad-only generated-screen attachment before AX queries, followed by incremental
exact native-root/editor facts. Editor data is limited to two elements, 256
value characters and 2,000 description characters. Independent R1 is clear;
privacy `20261026T130829Z-41745` and similarity `20261026T130831Z-42020` pass.
All guards, interactions, assertions and ten-second limits are unchanged.
Runtime is pending. Extra observation can affect timing; a passing diagnostic
run alone must not be described as a repair.

## Native editor admission repair

Diagnostic `ui-test/20261026T131026Z-43150` still fails; it does not prove export.
Its bounded attachments prove one exact local root and one native TextView:
enabled, hittable, focused with selected text, and no software keyboard.
Alerts and sheets both count zero. The software-keyboard guard excluded this
valid native naming state. The single generated picker screen was inspected.

Start `61b5c2ab`. Filtered fixer `cleanroom/20261026T131816Z-49901` removes that
proxy guard. Exact owner, identifier, uniqueness and actionability checks remain.
Return now requires this exact editor to disappear plus the existing actual-child
or navigation proof. All actions, nonce checks and limits remain unchanged.
Independent R1 is clear; privacy `20261026T132217Z-54957` and similarity
`20261026T132217Z-54958` pass. iPad and shared iPhone execution are still pending.

## Cancellation readiness

`ui-test/20261026T132410Z-57679` fails before folder naming. In the cancellation
phase, the Save button's AX hit-point query fails. That control is not used to
cancel. The folder-editor repair therefore remains unverified by this run.

Filtered fixer `cleanroom/20261026T134059Z-73426` makes root readiness require
the actionable overflow control, the actual next cancellation action when
Cancel and Back are absent. It removes Save only from that helper's readiness
and diagnostic list. Real Cancel, Back and overflow actionability, navigation
bounds and final Cancel checks remain; real Save/Replace assertions elsewhere
are unchanged. Independent R1 is clear. Privacy `20261026T135222Z-86956` and
similarity `20261026T135219Z-86690` results are retained; native checks are pending.

## Native export proof on all three platforms

On clean `85969338`, iPad `ui-test/20261026T141305Z-4872` and iPhone
`ui-test/20261026T141724Z-9854` each pass the actual native Cancel/Save/Replace
test with zero skips. Return still requires the same editor to disappear.
Independent iPad proof `package-interop-check/20261026T141724Z-9729` and final
iPhone proof `package-interop-check/20261026T142152Z-14415` both pass against
their own frozen pre-launch source copies. Each proves 12 tables, 2,425 rows,
270 media files, matching timing, zero stamp edits and zero client-open edits.

The earlier Mac native/export proof remains retained above. This closes the
bounded three-platform export gap. Historical failed receipts remain failures.
It does not establish media-bearing file import/review/relaunch, full U162,
the original-base integration gate or milestone acceptance. P16 implementation
is now in progress; the public media-input refusal remains until verification.
