# Replacement consent qualification

Starting commit: `de00fbd1f8e27f11223a88a8b183ab32b28083c1`.
Original app gate base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
No unit or release acceptance is claimed.

Filtered author `cleanroom/20261117T101440Z-71285` changes the common Runtime
qualifier. It preserves pending setup while publication or adoption targets a
different binding. The actual target still needs matching settled Clear R6.
Independent `102013Z-81138` clears the exact seed; no author bytes changed.
Production commit: `2512b6310`. Ordinary privacy passes.

Separate test author `102014Z-81407` adds native acceptance only at three
confirmed Bind seams. Independent `/root/adoption_empty_decks` clears patch
SHA-256 `42d1ca29c77a63ddd09e1e5df4bd7b475924a3dc80b851815f548b37130a8161`.
All other phases and protected test bodies stay unchanged. Commit `9829769d3`
contains only these fixture edits; ordinary privacy passes.

Actual `core-test/20261117T102617Z-5064` passes all five selected tests with
zero failures or skips on clean `2512b6310`. These are the new consent case
and the four restoration and provider-choice cases. The unchanged native
sequence now reaches its actual replacement scan.

The whole binary `102842Z-23892` has 134 passes, three failures, zero skips.
Failures remain in accepted original Active delivery, retained blocked work
after cancellation, and lost Active reply cancellation. Their original typed
IdentityUnverified errors remain recorded. No whole-binary pass is claimed.

A further independent source audit by `/root/incoming_contract_review` confirms
a public Rust race: caller A finishes the target check, caller B confirms and
accepts replacement setup, then A qualifies the predecessor using that new
consent. CoreBridge's normal maintenance lane serializes its calls; no ordinary
Swift overlap is established. The existing provider mutex covers individual
writes but not the check and write together. This required follow-on is under
repair; revision comparison alone is insufficient. Preserve original completion
ownership and keep provider I/O outside Runtime and tracking locks.

## Serialized qualification

Filtered author `cleanroom/20261117T102959Z-31268` uses the existing provider
publication mutex across the actual target check and qualification. Runtime
and tracking guards end before sidecar I/O. Setup uses the same mutex.
Independent `104004Z-61124` clears the unchanged seed, lock order and all three
complete files. Actual exported patch SHA-256:
`e0553f69a0228f05b485eaba80549d3673fceed523921758be49d59d52ed9eb5`.
The author's report-body digest is not the Git export digest.

The new synthetic Host regression holds the original qualifier, observes actual
setup contention, releases it and joins both workers before assertions. It then
requires pending successor consent, unchanged rows and ACK history, and actual
replacement Bind and scan. Actual `core-test/20261117T104709Z-94718` passes
all six selected consent and restoration methods with zero failures or skips
on clean `1dd82b55f`. This includes the new concurrency regression. No whole
binary, native-overlap or integration-gate acceptance is inferred.

Ordinary privacy `104513Z-87110` flags the provider file. The independent
complete-file public review admits only its exact SHA-256
`ae9aa3a95eb6df936adfef74504f648cab637030c0544f6a1376cd282ba5624a`.
Matcher behavior, token lists and privacy access stay unchanged.
