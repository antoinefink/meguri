# Normal-app peer trial, 2026-11-14, R16

This is a practical test of the signed development app, not a J2 selected-leg
receipt or unit, P24, milestone or release acceptance. The normal Mac preview
was built from 4f8d497d and the phone from 37c0595c. The later commit changes
only Rust test fixtures and their review record. Main remained clean at
37c0595c throughout the phone session so its launch proof stayed valid.

## Collection and review observations

The phone initially had four old reviews and no new cards. Through native
Settings, the offered preview showed four cards and zero reviews in iCloud.
The test used the offered iCloud-to-device replacement. It did not publish the
old phone collection. Replace was submitted once in drive
20261114T213846Z-54471. All three actions passed; the whole job failed with 48
hierarchy findings. The actual screen showed Preparing, with Cancel disabled.

Drive 20261114T214118Z-55950 then passed both actions with zero hierarchy
findings. The replacement had completed: the normal Decks screen showed the
public Media Review Check deck with four new cards. Drive
20261114T214253Z-57199 passed all three actions with zero findings and showed
the same counts and an enabled Study button.

Drive 20261114T214438Z-58710 executed all 15 actions. It showed the question,
revealed the answer, captured the generated test image, recorded one Easy
answer, ended the review, and showed three new cards and one recorded answer.
The whole job failed with five findings because the rendered Play audio link
has no accessibility identifier. Keep those failures; successful actions do
not make the job pass.

Normal-app Sync Now was then offered and used. Drive
20261114T214644Z-59958 passed all three actions with zero findings. The
retained exchange reported success and a last-synced time. The overall status
still said Unknown; this does not prove all attention inputs are complete.

Read-only inspection of the Mac's normal collection before this phone answer
showed four cards, all new, and no reviews. After foreground activation, the
same collection had three new cards, one scheduled review card, and one review.
A further read-only check confirmed exactly one Easy rating. Aggregate and hash
receipts remain under the ignored build directory. The database was never
edited by these observations.

This proves that the offered cloud collection can be adopted on the physical
phone and that this phone's saved test answer reaches the normal Mac
collection. It does not yet prove a Mac-originated review arriving on the phone,
or a complete bidirectional conflict matrix. Normal run
20261114T214824Z-60682 then reopened the same phone build. Drive
20261114T214920Z-60680 passed all four actions with zero findings: three new
cards and one recorded answer survived the cold launch.

## Media and visual limits

The generated image rendered on the phone after adoption. The phone already
had media before replacement, so this image alone does not prove a fresh
asset download or byte equality. Audio output was not measured. These remain
open. Existing native count-row hierarchy findings also remain unresolved;
no checker exemption, layout override or passing result was introduced.

## Prepared repair

Status-to-Connect loses its native destination on the compact phone layout;
the direct Settings path opens it. The prepared seven-file fix defers the next
presentation until the status sheet's native onDismiss callback. Independent
review 20261114T214139Z-56303 found no P0, P1 or P2 source defect. The patch
remained isolated while the phone session ran. Compilation, model tests and
rendered native verification are still required before accepting that repair.
