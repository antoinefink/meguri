# Review: class (b) rewrites in two sync actor scenarios (PR #90), 2026-11-21

- Branch `unit/class-b-sync`, author spec-writer session `s-20261121T182409Z-4e4281b1`.
- Reviewer: a separate Opus agent. It did not write the change and ran no jobs. Its only access to Anki at the pinned tag was a file search; it quotes no Anki text.
- Context: `similarity-test-data-dispositions-20261121.md` (class b), two of the three files the hand-off called expensive.

## Findings

- Same behaviour: yes. The review-log table has nine columns, in the order id, cid, usn, ease, ivl, lastIvl, factor, time, type (`ddl18.sql`). Each SELECT (`many-objects`) and each VALUES tuple (`records-up`) gives nine values in that order.
- Goldens: only `scenario_sha256` and `driver_sha256` changed. The new driver hash is the one master's other goldens already carry.
- Recording: the old and new `exchanges.jsonl` were compared field by field. Only these differ:
  - the `anki-sync` session ids (21 of 21);
  - the two note guids;
  - three collection-body `sha256` values.

  This is what `conformance/AGENTS.md` says a re-record changes, and S9 calls for one because `recording.json` holds the scenario hash. No consumer depends on the old values: `meguri-sync/tests/wire.rs` checks each exchange against itself, and `gate.bats` names only the records-down path.
- Meguri actor seeds: the new scenario hash changes only Meguri's random values (guids, session ids, file bytes). The goldens record guids as `#random`, and no observation covers session ids or file hashes. This fits the author's T1 runs, which give the same inherited `sync_error` before and after the change.
- New overlap: none. The tag has no positional insert into the review-log table.
- Comments: none is stale.
- Nits, both fixed: the PR body said 22 exchanges where there are 21, and the dispositions log still listed both files as open. The log's per-file table is updated in #88.

Verdict: approve.
