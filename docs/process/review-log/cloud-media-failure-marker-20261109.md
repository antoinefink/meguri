# Fixed media failure evidence

Source start: `2f77aff2`; integrated after `f819339d`.
Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
U113, U114 and U151 remain unaccepted.

Filtered export `34264` adds one DEBUG-only media tuple. Its fields are fixed
request, stage and category enums. It retains no file, record, account, path,
domain or error-description value. Policy status preserves this one tuple;
there is no history or extra log sink. Existing numeric errors, typed absence,
callback order, staging errors, cancellation and Release behavior remain.

Native `41559` passes all 48 selected checks. Independent review finds one P2:
two downloadsBlocked awaits can fail without a tuple after nextWork returns.
Both need Core-admission evidence and a focused rejection proof. Other reviewed
privacy, callback, staging and result behavior has no P0-P2 finding. Patch
privacy passes. No phone build or sync acceptance is claimed here.

Export `43366` records both download-block failures. Independent review closes
the P2 without another source finding. Native `48676` compiles and runs four
selected checks: three pass. The new no-work case passes; its download case
stops in setup because the original media batch is absent. A test-fixture
repair is pending. No failing result is accepted or retried unchanged.

Export `50749` adds the initial collection Saved acknowledgements and exact
revision checks. Native `52068` reaches the next fixture phase: newly created
manifest work is still pending alongside the blob. The original guard remains
in force. The fixture must follow the native collection-before-media sequence;
source diagnosis continues before a further test-only change.

Source review of `54907` has no P0-P2 finding. It keeps the original download
selection check. Native `56029` compiles: the
no-work case passes; the download fixture throws a tracking error in setup.
That enabled failure remains open.

A diagnostic device preview proceeds within this still-open batch. Its marker
production source is reviewed; the broader 48 selected checks passed in
`41559`. This preview neither accepts the new fixture nor waives any test or
gate. The actual device observation takes priority over more fixture work;
source diagnosis of the fixture continues separately.

## Focused completion

Export `59777` supplies standard fake-SDK Saved system fields while preserving
the actual handed records and batch IDs. Retained fetch uses the returned
manifest receipt. Independent review has no P0-P2 finding. Native `64993`
passes both parameter cases on the same production source as `48676`. The
broader initial-marker checks passed in `41559`; final combined focused
verification is pending after the two catch-path additions. Earlier failed
receipts remain.

Signed normal phone preparation `58165` passes. Device drives `60171`, `61614`
and `63208` complete all requested steps; their overall results still fail
identifier checks. One normal Sync Now plus a later observation advances the
sync timestamp and reports 2776 waiting media files and 2389 missing names.
There is no fresh media failure tuple. The retained taskCancelled diagnostic
is not evidence of a new media transfer failure.

The missing-name counter uses known remote Deleted state, distinct from
unprobed blobs. It does not prove matching bytes are absent under every other
local name, or that an import is the only remedy. No live-source loss is
inferred. Required completion implementation remains prepared separately;
unit, integration, physical identifier and full sync acceptance remain open.

Final combined native `66959` passes all 49 selected checks on the complete
source. Final similarity passes all seven changed source files. Privacy
checks pass. The diagnostic component and its fixture repairs are complete;
original failed receipts remain retained. The prepared completion changes and
full batch gate are not covered by these results and remain open.
