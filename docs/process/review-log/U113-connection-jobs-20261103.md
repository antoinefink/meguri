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


## Debug failure evidence

Component start:25f0b383. Export19258 adds a DEBUG-only typed classifier.
Seeded export22661 retains one safe value for the existing error accessibility
metadata and caps unified-log output at 64 entries. Independent review finds
one P2: deferred configuration failure names the outer exchange action.
Export27795 changes that argument to the held action. Recheck finds no P0-P2.

The classifier discards error and action payloads. It uses fixed enum tags and
a bounded SDK code, with no raw error, description, account or record values.
The sink runs outside the mutex. Release code and sync decisions are unchanged.
This is diagnosis support, not a repair of the live transfer or another change
to the approved Resume contract.

Focused checks: Swift28882 passes 12 tests; Swift30717 passes 18 tests.
Private28419 and Similarity30026 pass all five source paths.
The signed live check and batch gate remain pending. No unit is accepted.


## Returned failures and status

Component start:4a96f2f0. Export38471 adds result and status evidence.
Swift40700: 18 pass, two fail. Export43525 changes six type expressions,
with assertions unchanged. Swift45005: 33 pass, two fail. Both runs stay failed.

Independent review approves withdrawing that optional classification feature.
Export46289 restores the original behavior for those errors and withdraws only
the two new feature tests. All original tests, assertions and limits remain.
The failed exports and receipts stay retained and failed. Account resolver
errors can still report other; codes come from typed Core status or the
established SDK wrapper. No sync decision changes.

Independent source review clears the reduced candidate. Swift48708 passes all
33 selected tests; Swift50172 passes 18 adoption/recovery tests. Private48434
and Similarity49644 pass the three source paths.

Signed Mac Debug build33447 passes at 4a96f2f0. A real Sync Now reaches media
work, followed by temporary unavailable and awaiting-evidence states. No catch
event is captured. Phone preparation34943, Connect36999 and recheck38132 pass
transport and original-process checks; page work remains active. Recheck46795
fails before running its script with XCTest future timeout 1000. It supplies
no app outcome or continuity proof. The live round trip and batch gate remain
open; no unit is accepted.

## R3P2 regression

Export73717 extends the existing held-reply suspend test. Independent review
finds no P0-P2. Core76840 runs alone and fails only the exact-error assertion;
cleanup, rows, generation, closed admission and retry/close checks pass.
The valid-reply case remains. No production repair is applied yet.
The nextest reservation now names this exact library test.

Export79970 retains the original adoption owner for abort rebind. Independent
review finds no P0-P2. Public admission remains closed; retained evidence,
current generation, fresh file and settled publication checks remain required.
Cleanup errors still propagate. Format82718 adjusts the test layout only.
Core83372 passes all 11 connection-job tests. Core84349 passes all 23 host
adoption-guard tests. Private83661 and Similarity83740 pass. Lint and the batch
gate remain pending; no unit acceptance is added.
Core-lint84743 passes all four checks. R3P2 is closed; final enqueue and the
batch gate remain open.
