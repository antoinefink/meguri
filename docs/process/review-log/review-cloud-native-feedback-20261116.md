# Normal review and cloud observations — 2026-11-16

Production code: `fefef71ad` (unchanged). Native checks use clean `379a8f979`.
This is practical development-app feedback, not signed-lane or milestone acceptance.
The full integration gate remains failed at its original base.

## Phone review, upload and cold retention

The owner unlocked the phone. Normal drive `20261116T165008Z-21753`
passes six steps with no failures, skips or hierarchy issues. The app has the
public four-card media fixture. No synthetic route, fixture reset or clock is used.

Normal preparation `run/20261116T165355Z-28787` verifies the installed app,
runner and selected process. The subsequent drives keep that process running.
`drive/20261116T165527Z-31294` reaches Good once, presents the next question,
and ends the review. Its capture shows the real PNG. The audio control is
visible; playback is not established. The run still fails: five captures report
the same missing native audio-link identifier, and its next navigation wait
incorrectly expects the global sync button on the deck page. No grade is replayed.

The continuation `drive/20261116T170053Z-34867` uses the observed BackButton,
returns to Decks and requests Sync Now. All eight steps pass with no issues.
Process identity/installation continuity is verified. Native diagnostics report
three collection records saved, no failed saves, and zero pending, held-back,
in-flight or waiting incoming work. The completed manual exchange succeeds with
selected=0; it does not prove that this command uploaded the review. At that observation the outstanding-work counters are zero. Process totals
do not identify individual records or tie the review to a particular upload.

`run/20261116T170649Z-40223` normally relaunches the app; its selected process
identifier differs from 28787. `drive/20261116T170844Z-44643` passes both
read-only steps with verified continuity in that fresh process. The inspected
home retains two available cards and the one-answer previous session.

## Mac review and incoming phone schedule

Antoine permits Meguri feedback directly on this Mac. Host runs use the existing
explicit host opt-in, without the project VM selection for that invocation.
Only the two continuation passes override the screen-use interval for this
feedback session. App-only captures, exclusive leases, native assertions and
test deadlines stay unchanged. These receipts never replace required VM gates.

The older-preview refusal in `drive/20261116T171020Z-45730` is retained.
The next run no longer meets that refusal; no claim is made about who closed it.
`drive/20261116T171556Z-48355` passes six host steps. Its real deck shows
2 new, 0 learning and 0 to review. The earlier retained Mac observation had
2 new and 1 learning; this is consistent with the phone review, not an exact
row-identity proof.

`drive/20261116T172157Z-54130` records exactly one Mac Good, presents the next
question, and ends the session. Its real counts become 1 new, 1 learning and
0 to review. Original native saved-record totals are three, with no failed
saves and no outstanding work. The run fails when Sync Now lies beyond the
parent-window frame used by the driver. The input guard refuses the tap.
No guard is weakened and no second grade is issued. A later geometry/read-only
check, `20261116T172639Z-60763`, errors at the first native screenshot because
its window does not exist. Its cause is unresolved; it is not a sync pass.

On the retained phone, `drive/20261116T172851Z-63173` observes five fetched
collection records delivered to the core, no refused callbacks and no outstanding
work. No manual fetch or grade occurs in this receiving process. Navigation
then fails because the script guessed `.home` instead of the actual `.decks`
tab identifier. The corrected continuation `20261116T173135Z-65942` passes all
six steps with verified continuity. Its inspected native deck values are
1 new, 1 learning and 0 to review, matching the Mac's changed schedule.

These observations are consistent with schedule transfer in both directions.
Per-record causality remains unproved. Exact card/revlog equality, duplicate-history checks,
fresh media-byte transfer and cold retention after incoming application remain
unproved by these observations. Existing images rendering is separate from
proof of a new media transfer. Unknown status remains conservative; no status
requirement is waived to make the display green.

## VM connection and remaining boundaries

Public fixture producer `fixtures/20261116T164013Z-94927` passes. The fresh
20-card, five-PNG VM collection opens normally. Connection preview drive
`20261116T164736Z-5193` passes twelve UI steps but reaches unavailable/Unknown.
The retained account-state element says not signed in, and the failure value is
previewRead:none. This is a current account/admission boundary before any actual
preview record read. It does not establish a server or transport defect.
No account setting changes or owner-only setup steps are performed.

The native audio identifier issue remains open. Read-only source review finds
that trusted DOM IDs reach rendered anchors. This review establishes no public
iOS native identifier mapping. Trusted DOM metadata stays separate from native
observations. All existing native replay
assertions, counts and twenty activations remain required.

All failed receipts remain. No Unit, complete gate, signed acceptance or release
is declared. During the separate coordinator's core-test phase, no new heavy
Rust/Apple build, core-lint or core-test starts here.

Independent evidence review is clear after two wording corrections.
