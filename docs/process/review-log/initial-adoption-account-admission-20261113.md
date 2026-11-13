# Initial adoption account admission

Start: `32183b60`. Role: fixer. Session: `s-20261110T214025Z-20fa15fd`.
Pending batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

An account signal could arrive while an initial download held its original
callback. Account proof correctly waited for that work, but admission stayed
open. Returning the old callback could then install the collection first.

The existing test also joined proof before returning its callback. Diagnostic
export `cleanroom/20261113T095845Z-3634` recorded the ordered account signal,
then authority waiting with the callback still held. Only its verified helper
was stopped. That run remains incomplete failure evidence. The temporary
markers were reversed exactly.

Test-order export `cleanroom/20261113T100133Z-13958` retained every assertion
and both account/Close cases. Independent review `20261113T100307Z-15383`
cleared its source. Running it in `swift-test/20261113T100329Z-15839` exposed
three failed account-case assertions: collection/backup absence, unchanged
generation, and disabled settings. The Close case and retained-publication
checks passed. No failing assertion was removed.

## Repair and review

Export `cleanroom/20261113T100521Z-18193` closes initial admission at the
ordered account transition and joins original work before proof completion.
Core re-enumerates retained work before admission reopens. Same-account proof
can continue; failed proof stays blocked, and account loss uses Core cleanup.
Explicit transport Resume retains its authority check. Activation also rejects
unresolved proof or drain. Ordinary account refresh remains unchanged.

Added checks cover same-account continuation, failed-read recovery and sign-out.
The original callback, replacement, generation, settings and ownership checks
remain. No UI style or presentation change was made.

R1 `cleanroom/20261113T101444Z-36292` found no P0/P1 and one P2: the new error
path needed the same lifecycle fences as its successful continuation. Fix
`20261113T101938Z-51346` changed only that catch; tests were byte-identical.
R2 `20261113T102128Z-53703` cleared the finding, with no new P0/P1/P2.
Current errors retain their category; original result, ACK and drain remain.
The interrupted catch race has source-review coverage, not a new runtime test.

## Verification and limits

- `swift-test/20261113T101458Z-37829`: six methods passed, including both
  interruption cases, three reproof cases, pause/background, transient failure
  and retained-publication proof ordering.
- `ui-test/20261113T101739Z-49358`: one test passed, zero skips:
  reveal, ratings, Undo, waiting and saved state after relaunch. This precedes
  the catch-only correction; the review path and all tests are unchanged.
- `swift-test/20261113T102143Z-54166`: the same six methods passed on the final
  catch correction, no skips.
- R1 privacy `20261113T101550Z-39598`, static checks `20261113T101550Z-39906`
  and similarity `20261113T101551Z-39596` passed; similarity covered both files
  with no exemptions. Final staged checks are retained with this commit.

The broader failures remain in `retained-publication-proof-order-20261113.md`.
Isolation found the selected refusal case passes alone, while provider outcomes,
flush/ACK checks and the deleted-active check still fail separately. Their
receipts remain retained. A strict provider-fault witness patch is prepared
in build output, unapplied and unverified. It is deferred while the live review
and iCloud workflow takes priority.

The full pending gate and latest signed cross-device round trip remain due.
This checkpoint gives no Unit, milestone or release acceptance.
