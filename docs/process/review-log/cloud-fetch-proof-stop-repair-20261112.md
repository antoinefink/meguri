# Original fetch proof result — 2026-11-12

Role: fixer. Base: ebb3d354. Session: s-20261110T214025Z-20fa15fd.
Contracts: sync/policy P4, P15, P2.1; engine/jobs J2.1.1.

The original wait stores its first result and producer. A validated account-stop
receipt can identify its own cancellation. Prior provider, transport and SDK
failures stay errors through later close. Cleanup still runs. The native result
forwards sealed evidence; ordinary output and ACK delivery remain in place.
Caller and stop producers record their result before cancelling the provider.
No authority, protocol, timeout or five-case assertion changed.

Author: cleanroom52662. Type annotation:66221. Focused correction:71196.
Review64613: P1 private test helper, P2 cancellation ordering; both repaired.
Independent review73742: no P0–P2 in the corrections.
Core swift-test74117: PASS, 6 groups, 31 cases, no compiler warning.
App swift-test76743: FAIL, 8/9 groups pass. Original five-case account cases1/2
have no command error; expiry3 and replacement4 still fail. Case3 also reports
helperFailure=closed on StartEngine. Retain that evidence and every assertion.
Similarity78918: PASS. Private scan79236: FAIL on prose and a local test name;
text-only cleanup follows. No scan exception or guard change.

Required batch gate remains pending from base1416a520. No Unit, milestone,
signed lane, full-gate or release acceptance claimed.

Final text cleanup88309: two comments removed and one local test gate renamed;
reviewed as exact substitutions, with all other source unchanged.
Core swift-test94169: PASS, 6 groups/31 cases, no compiler warning.
Private93872 and similarity95875: PASS with unchanged guards.
App76743 remains failed evidence; no full-suite pass is claimed.
