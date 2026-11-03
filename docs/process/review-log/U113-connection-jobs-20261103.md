# U113 connection jobs

Status: WIP; no unit acceptance.
Unit start:1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Component start:e20b54649c2a35a22b4c347baf1b567e54a0c41e.
Batch base:1416a520542a8c5daa38a23bfe398da704c3d6c9.
Fixer:s-20261103T133750Z-f276f58e.

The existing registered pool now owns the two potentially long connection
reply calls. Results remain typed and tied to the original Host. Swift retains
native payload leases through job setup, failure drain and terminal result
read. The two moved calls retain their previous ordering. Runtime cancellation
is checked at the first effect, then preserves mandatory completion and cleanup.

## Review history

R1 export33445 receives three findings: an all-job pre-flush wait violates
participant ordering; suspended close loses cleanup access; and moved Swift
calls can overtake each other. The next candidate addresses these findings.

R2's interrupted export is recovered without reading the raw model log.
The restricted guard/export continuation48313 passes and matches all recovered
permitted file bytes and modes. Independent review finds that close still waits
for unrelated jobs before flushing, retained-store open can create or migrate,
and a Swift test waits without a bound.

R3 export51924 passes its clean-room boundary checks. It scopes the early drain
to resource-retaining jobs, validates the original retained store without
creation or migration, and bounds the FIFO test with release/join cleanup.
Full independent review finds no proven P0/P1. A second independent audit clears
the retained-store refusal path. Source review is not execution evidence.

R3P2 stays open: a malformed reply claimed before suspend may fail during its
required abort/rebind because admission has closed. Rebinding asks for public
collection evidence, so its error may mask the reply error. Follow-up needs a
post-claim suspend test and a checked repair using retained evidence. The
round-three limit allows focused verification now; failed checks need fixes.

The integrator applies51924 after the R11 test commit and runs core-format62314.
Core-test62797 stops at compilation: the abort closure promises a unit result
but returns the boolean from `end_adoption`. No test executes and this is not
passing evidence. A narrow compiler correction is required before execution.
Focused Rust and Swift verification, regenerated bindings, privacy/similarity
checks and the required batch gate remain pending.

Core-test71962 compiles but the proposed selector matches no tests; its failed
receipt is retained. The corrected module selector in core-test72920 runs all
11 connection-job tests, all passing. Core-test78194 then finds a missing
`ReadRows` trait import in two new suspend-test calls. That build stops before
the broader lifecycle selection runs. Private scan passes the final staged
source. Preserve this work as WIP while correcting that test compilation issue.

Narrow fixer pass, rooted at actual baseline ef38a1b7: the seeded R3 candidate
now discards the successful `end_adoption` boolean and returns unit, propagating
errors as before. The regions identified by scan64302 have fresh wording;
private test/local names were shortened without changing assertions or public
names. One source consistency pass completed. No builds or tests ran here;
the integrator owns verification. R3P2 remains open.

Export79264 adds only the compiler-requested `ReadRows` import. Independent
review confirms that exact one-line delta. Core-test80417 then passes all 32
affected lifecycle, retained-store, payload and cursor tests. Swift80906 builds
matching device, Mac and simulator libraries and bindings, then passes all 11
selected bridge and interval-registry tests. Similarity84500 passes the complete
component from e20b5464. These checks supplement the 11 passing connection-job
tests; neither earlier failed receipt is reclassified.

The remaining AppModel integration checks, lint and batch gate remain pending.
The recorded P2 and live CloudKit round trip also remain open. No unit trailer
or milestone acceptance is added.

Swift91047 passes the retained 17-test AppModel adoption/recovery selection
against the new bridge. Core-lint93817 reports one test-hook collapsible-if
diagnostic; export95638 changes only that conditional. Independent review
confirms the same short-circuit order and lock lifetime. Core-lint97183 passes
all four checks, including layering. Core-test97539 passes the exact contention
regression that exercises the edited hook. The earlier failed lint stays
recorded. No limit, assertion or runtime guard is weakened.

## Live checks after integration

Personal Mac Release build9523 passes, source bf940b3e. Executable SHA-256:
443a3a1dc10a366b96afae0b205f71ee0c5999c9d19f9fa7dfada3cbe0dd4435.
The exact signed development app opens its retained collection. A real Sync Now
attempt fails; a foreground comparison later returns Could Not Finish Yet.
Media remains pending. Neither observation is round-trip acceptance.


A restricted codesign probe warns that the entitlement blob is invalid.
Independent host verification of the unchanged hash passes both strict checks
and both entitlement-display forms, returning the valid signed capabilities.
The warning is diagnostic-context dependent. No signing change is justified;
tools/AGENTS.md records the trap.


Normal team-signed Mac Debug build13681 passes. A bounded debugger read gives
no usable error type. The debugger is detached and its process joined; the
Release signature is unchanged. The live failure and batch gate remain open.


## Native epoch witness

Component start:1da8c363. Filtered export21835 adds two test paths.
Independent source review finds no P0-P2 issue; no runtime guard is changed.
Swift23516 passes the native archive witness: independent read/page encodes and
a restored object retain identical bytes for the synthetic record. This does
not establish equality across separate live server reads. Swift25046 passes
three adoption tests, including unchanged epoch-page success, changed epoch
refusal and the existing corrupt-page case. Exact saved-row and review checks
remain enabled. Private23189 and Similarity24586 pass both test paths.

The phone recheck20449 passes transport and original-process continuity; its
screen still shows Unknown and Try Again. No collection was adopted. Restricted
probe18556 could not reach the device service; the host-context check supplies
the observation. No failed receipt is reclassified. The native witness does
not establish the cause of this live failure or supply a completed round trip.
