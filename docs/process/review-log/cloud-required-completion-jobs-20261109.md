# Required SDK completion on registered jobs

Implementation start: `f819339d`. Swift source base: `dfdf463c`.
Integrated on `8b0f207e`. Original batch base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.
U113, U114 and U151 remain unaccepted.

## Contract and source review

Approved contract: `6732af21`, engine/jobs J2.1.1 and engine/suspend S1/S4.
Filtered Rust `40364` and Swift `43911` add Ended reporting on the existing
pool. Required completion ignores queued cancellation without bypassing
original authority or marking a first effect early. It joins the early Host
and Runtime drains. Ordinary jobs, slots, threads and paused admission remain.

A separate completion wrapper retains exact normal returns and leaves no-body
or panic results empty. Swift drains accepted work after listener failure,
then uses actual typed results before setup errors and retains terminal failure
when the cell is empty. Stored reads use no later owner.

Independent source reviews clear production behavior. One P2 requires a real
active-producer freeze proof. Combined export `47844` adds that proof with a
held original SDK send, awaited Ended output and actual Core suspend. Review
confirms the producer is active when freeze begins; it is not manually ended.
The final export retains all 16 source files, removes only three flagged prose
fragments and passes privacy. Prior Rust and Swift production reviews stand.

## Verification

Rust formatting passes. Focused Rust policy, media, host and suspend checks are
running. Native checks follow Rust on an idle build lane. No compiled result,
Unit trailer, batch-gate pass or release acceptance is claimed yet.

The resource terminal calls and generic Started report remain separate audit
work. Required Ended semantics do not grant those calls a new owner exemption.

Core `69561` fails compilation before tests. The completion selector borrows
the engine through self, then moves self into its worker closure. A narrow
owned-original-engine correction is pending; no test is counted as passing.

Export `70251` retains an owned clone of the same Engine handle. Independent
review confirms shared identity, generation and registry. Core `71960` passes
all 248 selected checks. Native `74919` builds; one new order test expects
nonempty commands from quiet Ended while automatic sync is off. Source review
confirms Commands([]) is the exact original result. A strict synchronous
baseline and rearmed Started proof will correct that expectation, preserving
every drain, cancellation and ordering check. The native failure is retained.

Filtered export `85843` makes only that fixture correction. Independent Swift
review finds no P0–P2 issues. Native `88001` passes all five selected tests,
including both listener cases of the corrected order proof and the ordinary
maintenance and connection failure-precedence checks. The other passing
checks from `74919` remain applicable: production source is unchanged.
Core lint `86723` passes all four checks. Privacy `90219` passes both files;
similarity `89847` passes all 21 in-scope files, with three excluded files and
no allowlist use. The full original batch gate remains pending.

Native `29868` rechecks required completion alongside the later reader and
generic Started changes: all 49 selected checks pass, including the real active
SDK producer freeze proof and both listener cases. This supplies a passing
combined native run after the earlier test correction. No unit or batch-gate
acceptance is inferred from these focused checks.
