# Resource boundary checks

Implementation base: `8921d870`; repair base: `60c99f51`.
Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Implementer: `s-20261109T094715Z-c7c7c45e`, filtered export `92053`.
Fixer: `s-20261103T133750Z-f276f58e`, filtered export `4648`.
No Anki or private source was used. Production code is unchanged.

Four added Rust checks use actual retained passes and registered jobs:

- Full complete, cancel and finish registered before pause; suspend waits for their returns.
- Actual worker tails after map removal; teardown joins them and rejects an expired deadline.
- Account changes preserve the original typed error and exact request cleanup.
- Simultaneous stale authority preserves error order, zero SDK calls, no attempt charge and the first-read grant.

Independent reviews: `build/cloud-resource-matrix-independent-review-20261109.md`
and `build/cloud-resource-matrix-r2-review-20261109.md`.
The first run `20261109T183940Z-2027` passes three cases. The final case fails
because pausing an already stopping engine leaves its policy token unchanged.
The corrected fixture uses an actual account-status transition and asserts a
strict token increase. It restores the captured account before checking a new
SDK and retained grant. Assertions, limits and production behavior are unchanged.
Focused `20261109T184702Z-5856` passes that corrected case. The other three
case bodies remain unchanged apart from formatting. No P0–P2 review finding remains.

Other required coverage stays open in the lifecycle log's proof table.
These focused checks do not grant unit, integration or release acceptance.

Lint `20261109T184810Z-6358` passes all four checks; similarity
`20261109T184815Z-6709` passes both files without exceptions. Source privacy
check `20261109T184815Z-6630` passes source; the log is covered by `20261109T184839Z-7285`.
