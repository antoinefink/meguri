# Spec follow-ups after implementation review: review log, 2026-11-21

Branch `unit/spec-followups-1121` (PR #37), spec-writer session `s-20261121T040841Z-2dc4eb63`. The lines marked "Changed 2026-11-21 (implementation review)", or carrying that marker inline, settle what the implementers of U81, U82, U84, U88a and U34 reported after their units merged. They were decided by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs.

## Decisions

1. `conformance/browser-ops` B6, `browser/columns` C15, `browser/sorting` S11: they now record what U84 built. A `fld:` sort waits for U261 (`unsupported_observation`).
2. A search never writes, as the coordinator decided. New `sched/day-clock` D8.1, with matching lines in `search/query-model` Q8, `browser/result-sets` H9, `conformance/browser-ops` B1 and B3, `conformance/search-ops` S1, `search/state-and-flags` S7, `search/card-extractors` X1, `search/history` H2 and D14.2. Q8 records two differences from Anki in spec sentences, since no scenario observes them:
   - Anki's search can store `rollover` and `localOffset` when it computes its day afresh.
   - After the clock moves back, Anki's search keeps its cached day.
   A search inside an op or read that already took its day from the day clock keeps that day.
3. `text/checksum-and-sort-field` C5: the duplicate checksum is C4 steps 2 and 3 on the comparison text, as `notes/field-check` K4 says and the golden `notes/fields-check/entity-markup` shows.
4. `search/text-matching`:
   - T7.1: a note whose note type is missing matches no plain, `nc:`, `sc:`, `-sc:`, `re:` or `w:` term when an excluded field exists anywhere. This follows Anki, not the implementer's choice.
   - T7.2: a sort index past the last field is not excluded.
   - T16: `{pattern}` is the value before NFC.
   T7.2 and T16 keep the implementer's choices.
5. `notetypes/structure-changes` S1.2 and S3 give the full order of refusals, with the origin range checked after the planning `Stale`. The coordinator settled PR #28's question this way: a draft is checked against the base it was loaded from, and anything that differs because the live note type changed is `Stale`.
6. Plan section 9 records:
   - the merged units: #25, #26, #30, #23, #34, #28 and #31;
   - PR #32's builder follow-ups, assigned to U260 (also noted in `search/join-and-replace` J3.6);
   - the code changes these decisions need;
   - the queued `--search-data` tooling follow-up.

## Independent review

Reviewer: an independent Claude subagent (Opus). It received the diff and the implementers' questions, not the writer's conclusions. To check facts, it read the neighbouring specs, the code on master (after PR #31 merged), the PR #28 thread, the goldens, and Anki at tag 26.09.3.

- P1: none.
- P2, all fixed:
  - Q8's "every search" was wider than D8.1 and the code: deck options' preset search takes its day from the day clock. Q8 and D8.1 now cover search requests only. A search inside an op or read that already has its day keeps that day.
  - The clock-moved-back difference from Anki and from the study screen was not recorded. It is now in Q8. `rollover` is written when it is "missing or unreadable".
  - The plan's U34 fixer bullet left the sort-field check after the note type read. It now moves that check up with the repeated-origin check, and it adds a test.
- P3, all fixed:
  - Old sentences were left beside the changes (S1.2 "Before them all", the Q8 table, H9, X1, H2). Each is now edited in place with the marker.
  - B3, search-ops S1 and D14.2 now say that `browser_rows` and `search` run no day query.
  - T7.1 now covers `-sc:` when no field is excluded.
  - The plan's U81 bullet names `sc:`, and a doubled "and" is removed.
  - This log was missing.

Confirmed by the reviewer:
- B6, C15 and S11 match the executor and `area.yaml` (34 active scenarios).
- Anki's day read writes `rollover` and `localOffset` and never unburies. The executor's `day_without_writes` matches D8.1, and no scenario reads timing keys after a search without an earlier `timing`.
- C5 matches K4, Anki and the golden.
- T7.1 (a), (b) and (c) are right against Anki and U81's code.
- The S1.2 order matches `check_live` and `finish` on master.
- The PR numbers are right.
- No Anki path or identifier appears in `specs/`.
- The reviewer checked "no scenario reads timing keys after a search without an earlier `timing`" against the scenarios that mention `rollover` or `localOffset`, not against every scenario.

Second pass on commit a63c842be: no P1 or P2 left. The fixes match the findings, nothing new conflicts, and the log above is a fair account. Both optional P3 notes were then taken: S7 of `search/state-and-flags` is limited to search requests, and the first row of the Q8 table names the exception for a context built inside an op.

CodeRabbit (PR #37) found four Minor points, all answered on their threads and fixed:
- B1's `fld:` line now says the sort waits for U261.
- D6.1 names D8.1's exception.
- T7.2: a note type with no included field matches nothing, whatever its sort index (as in Anki).
- S16 says the stale-origin test changes with the code (plan section 9).

## Code and test changes required (for the coordinator to assign)

- U34 fixer, in `core/crates/meguri-engine/src/notetypes/structure/plan.rs` `plan_draft`:
  - Move the sort-field position check before the note type read.
  - Move the origin range checks after the digest comparison.
  - Tests:
    - a stale base with an out-of-range origin gives `Stale`, writes nothing and takes no backup;
    - a fresh out-of-range origin still gives `InvalidArgument`;
    - a removed note type with a bad sort field gives `InvalidArgument`.
- U81 follow-up, in `core/crates/meguri-search/src/plan/text.rs`: when an excluded field exists, the plain and `-sc:` clauses and the `re:`/`w:` clause must not match a note whose `mid` names no note type. Today they use `n.mid NOT IN (...)`.
- U85: the search context's day comes from the day calculation alone.
- U260: PR #32's builder changes.

CodeRabbit, second pass (three Minor points, all answered on their threads):
- `conformance/search-ops` S1 now separates the two sides: Meguri writes nothing, and the oracle may write the two timing keys.
- Q8 says why its two differences have no `deviations.yaml` entry: no scenario observes them. This is not changed.
- T7.1 puts the query-wide `search_failed` refusals of T7.2 and T7.3 first.

## U260 readings in `search/filter-tokens` (added to this PR)

The coordinator added the six readings that the U260 implementer (PR #38) made where filter-tokens says nothing. Each decision keeps the implementer's reading, with three exceptions.

Kept:
- T3: card type names that are not offered, and a `card:` term reads back only for an existing card type.
- T5: a field term names no field only when its name has no wildcard, in any mode.
- T7: a negation holding No Flag, Any Flag or Untagged becomes advanced.
- T7: `tag:re:` in J3.2's form reads back as a Tag value; field names fold letter case and keep their stored spelling.
- T7.1: a Field Contains text holding other white space stays offered and comes back advanced; so does a field name holding such a character; Leech beside a Card State token.

Changed:
- T5: `-10:45` stays text, as the negation of its literal. The implementer made it an advanced token, which finds every note (`fields.md` F3).
- New T3.2 and T4: the written-text item keeps F7's shapes unchanged in the run text, in brackets unless it is a single term. A negated operator word runs and fails with E3's error. The implementer had no run text for it, and wrote `preset:` values in their broken canonical form.
- T6: whether a token keeps its written text depends on the item read back (`has\-cd:x`).

Independent review: a separate Claude subagent (Opus), given the commit, the questions and PR #38's code.
- P1, fixed: with no stated way to place a kept token among other filters, its meaning was at risk, and T4 contradicted itself. The new T3.2 says how that kept form is written and bracketed, and T4 writes all of the query through J4.
- P2, fixed:
  - The plan lacked the `has\-cd:x` code change.
  - A query with no run text had no rule for its consumers. There is now always a run text.
  - T7.1 lacked the field-name exception.
- P3, fixed:
  - The flag row now comes before row 2, and its unreachable case is removed.
  - A group of one item, a group of `tag:leech` items, and the quotes of a T5 literal are now stated.
  - The stored spelling when note types differ is stated.
  - Wording fixes.
  - The plan now lists tests.
- P3, noted:
  - `my_var:3` with no such field becomes an advanced token that finds nothing.

Re-review of 8a6cec468: the P1 fix holds and every query now has a run text. One P2: T3.2's brackets grew at each reading back; it now brackets only what is not one item. Six P3 points, fixed: T3.2's refusal kinds and J3.6; pointers in J3 and J8; the stored-spelling order (lowest note type id); the scope of T5's quote rule; this log's "finds every note"; one sentence split.
  - Refused card types get no filter.
