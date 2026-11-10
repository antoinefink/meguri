# CloudKit amendments under review (R2066), combined review 2026-11-10

Scope: the parts that `e7c36c00` (cloudkit-agent J4.1, J5.1 and five pointers) and `52842733` (P24/Q11 D1-D7) mark Under review in the Status lines of `conformance/cloudkit-agent`, `conformance/cloudkit-acceptance` and `sync/policy`.

- Reviewed at: `ef3aa399` (origin/master). Implementer view only: no `refs/`, `oracle/`, `local-data/` or Anki source.
- Inputs: three independent reviews (A, B, C below), each verdict `approvable_after_fixes`. This record checks each of their findings against the spec text and test sources.
- No spec, test or commit was changed.

Severity: major = blocks approval of that part; minor = fix in the same correction; nit = wording.

## J4.1 Order evidence for engine observations

Verdict: not approvable as written. Four majors.

Confirmed:

1. **Major: the pair link is undefined (A, B, C).**
   - J4.1 pairs a result and a state save "only by their recorded references: the result's related operation label and the durable operation that the state save references (J4)".
   - J4 says only "A state-save or apply observation references the actual durable operation it reports". That is the tracking commit, a different operation from the send named by the result's label. A6 (`sync/cloudkit-records` line 450) is per batch: "per-record results before ... the engine state that reflects them".
   - Nothing says the two references must be equal. Nothing names the reflected batch, says which state save counts when several match, or says what an unlinked result means.
   - Effects:
     - A label scheme that never links (send op7, commit op8) makes no pair, so A6 is never checked.
     - One-to-many linking lets the validator pair a late result with a later state save and show A6 as holding.
   - Fix:
     - Every state-save observation names the send operation labels whose per-record results it reflects, in the same namespace as the results' related operation label. Keep J4's durable-operation reference as a separate fact.
     - A pair is a per-record result of send S and the first state save of the same engine instance that names S.
     - A state save that names S before one of S's results is a reversed order.
     - A result whose send is never named by a state save in its own engine instance, as after a kill, needs no order.
     - A state save that names no send, or a send with an ambiguous label, is a failed evidence check wherever A6 is judged.
2. **Major: a result with no state save in its own instance is not covered (A).**
   - Every kill between a result and its state save produces such a result: Q7.3's kill bullet, Q9.2's kill every 45 minutes.
   - J4.1's first bullet says a pair across `agent_instance` values "shows no order".
   - If the relaunched engine's state save is taken to reflect the original results, every killed leg fails A6. Q7.6 judges A6 on "every leg's step log".
   - Fix: covered by the last-but-one clause of fix 1. Say it explicitly in J4.1's list.
3. **Major: "callback-order fact, such as a tick, under the same fact name" is an open class (B, C).**
   - The validator cannot list which facts are order facts. Two observations that both carry, for example, a count under one name would be compared.
   - The cross-check is optional and works in one direction only: an agent with no tick has its append order trusted.
   - Fix (either):
     - (a) Name one integer fact, assigned per engine instance inside the callback before it returns. State whether it is required, and that no other fact orders.
     - (b) State that no order fact is recorded, and require each engine observation to be durably appended before its callback returns. A6 already allows the callback to await persistence, so append order is the delivery order.
4. **Major: a reversed shown order has two classifications (B).**
   - Paragraph 1 says Q7.4/Q7.6 "judge A6 from the order shown here": a reversed order would be A6 `false`. Under Q7.4, `false` means "the contract is corrected", which is K23's fallback, a product change.
   - The buffering paragraph calls the same journal an agent that "does not show the order J4 requires", which is a failed evidence check.
   - The validator cannot tell the two apart, so an agent defect could force a contract correction.
   - Fix: state which one applies. Suggested: a reversed order sets A6 `false` only when fix 3(b) or a required order fact makes append order the delivery order by construction. Otherwise it is a failed evidence check.
5. **Minor: the failure output is not testable (B).**
   - "The failure names the pair and the order each form gives" gives no fixed failure reason and says neither who records it nor where.
   - "the checks that depend on it fail" defines no dependency.
   - Fix:
     - Name the reason.
     - The Q7.4 runner records it for a leg or ledger check. The agent records `failed` with that reason only for its own J5 recovery check.
     - List the dependent checks (A6 and the check that required the pair), or drop those words.
6. **Minor: engine-instance labels need not be unique within one `agent_instance` (C).**
   - `sync/policy` P4 `ConfigureAutomatic` re-creates an instance inside one process. A reused label merges two instances and defeats J4.1's one-instance rule.
   - Fix: a label is never reused within one journal.
7. **Nit, wording (A, C):**
   - "Neither form overrides the other: ... so neither is taken as the delivery order" reads as contradicting the append-order bullet. Fix: "When the two forms disagree or tie, neither is taken as the delivery order".
   - "has missing evidence (J5)": J5's stop rule covers recovery checks only. Fix: "(Q7.4; for a recovery check, J5)".

Rejected: none.

## J5.1 Process boundary at a relaunch

Verdict: not approvable as written. One major.

Confirmed:

1. **Major: "the original exchange's actual result" is undefined after a kill (B, C).**
   - J5.1 says durable ACK state counts "only together with the original exchange's actual result (J3.2)", and the relaunched process recomputes "Q5 `results`, recomputed as Q5 states".
   - Q5 `results` is "recomputed from the request log and tracking". The request log is the fake's (Q3.7, acceptance line 133). The signed lane has none, and J5.1's list of surviving evidence has none.
   - Nothing in J3.2 or J5 stops the relaunched app's engine from running before the recovery check. J5.1 itself says "a durable row alone does not show which process wrote it".
   - Failure case:
     1. Kill after send, before the results are journaled.
     2. The relaunched engine re-sends and writes the ACK row.
     3. The recomputed results read `succeeded`, so the recovered `done` passes.
   - "An original acknowledgement whose manual or automatic cause is unresolved" has no rule for what resolves it. "unresolved" appears only in J5.1.
   - Fix:
     - The original exchange's actual result is the journaled observations under the original `agent_instance`: the terminal `RecordExchange` result and the per-record results with their operation label.
     - Q5 `results`, on the signed lane, is recomputed from those observations and tracking. Tracking alone only cross-checks.
     - Without the original terminal result the exchange is indeterminate (J3.2: fails).
     - A cause is resolved only by a journaled original event that ties the ACK to the manual exchange's operation label or to an earlier automatic operation.
     - Alternative: no engine admission in the relaunched process until the interrupted command's recovery check ends. This changes launch behavior, so it needs the master agent's decision.
2. **Minor: J3.1's words have two meanings (A, B).**
   - J3.1: "joins the original app-hosted process". J5.1 uses "the original process" for the killed one, which "the host never joins".
   - Fix: "J3.1's 'original app-hosted process' is, after a relaunch, the process that ran `close`".
3. **Minor: scope (B).**
   - J5.1 applies only after "a kill, or after it terminated the app for an offline period". Neither J5.1 nor Q7.3 says what happens when the app ends without a kill or a planned termination.
   - Fix: J5.1 applies to any end of the app process that the coordinator relaunches after. Or state that any other end of the app fails the run.
4. **Minor: J7 has no cases for J4.1 or J5.1 (B).**
   - Fix: add rows for each of these:
     - a tick that agrees, disagrees or ties;
     - a pair across instances;
     - a buffered result lost at a kill;
     - after a relaunch, `close.joined` reporting only the relaunched process's work;
     - an ACK from the relaunched engine refused for the interrupted command.

Rejected:

- "What the interrupted command is after an offline-period termination between commands" (B): J5's last paragraph covers it ("If the journal ends after a terminal `done`, the next command is its successor").
- "'Joins' for the host is undefined" (B): that is in-force J3.1 text, outside this review. Raise it separately.
- Cases that C tried and that failed: a killed process counted as joined; order across instances; a waiver. J5.1 and J3.2 already block each.

## Pointer sentences

- J4 "J4.1 states how the journal shows that order.": approvable once J4.1 is fixed.
- J3.2 "After a relaunch, J5.1 states which process the host joins and which work that `close` joins.": approvable once J5.1 fix 2 lands.
- Q7.3 kill bullet, last sentence ("What survives such an end of the process as evidence, ..."): approvable with J5.1.
- Q7.5 `engine` row, "as `conformance/cloudkit-agent#J4.1` states it": approvable with J4.1.
- Q7.6 A6 Note: approvable with J4.1. It inherits J4.1 majors 1, 2 and 4, because A6 is judged on every leg's step log.

## Provenance of `e7c36c00`

Confirmed, minor (A, B, C).

- The commit body ("J3.1, J3.2, J4 and acceptance Q7.3, Q7.5 and Q7.6 point to them") and its Spec trailer name `conformance/cloudkit-agent#J3.1`. `git show e7c36c00` changes no J3.1 line.
- The Status line is correct, and so is the J3 brief ("J3.1, the smoke, is not changed").
- Fix: history is never rewritten. The next spec-writer commit for these parts states the correction in its body and leaves J3.1 out of its Spec trailer.

## D1: P24 K2 reset path, sentence "Whether a device whose keys have not arrived after a reset ..."

Verdict: approvable after fixes. The sentence agrees with Q7.5 and Q12.

Confirmed:

1. **Minor: in-force contract text still expects a signed reset (A, B, C).**
   - `sync/cloudkit-records` K2: "deletes nothing, until the signed runs record the exact error domain and code CloudKit gives for such a record and this contract names it".
   - `sync/cloudkit-recovery` V24 A18: "The signed runs record what CloudKit gives after a reset, ... so that K2 can name it".
   - Q7.5: "no signed leg triggers an encrypted data reset".
   - This conflict predates `52842733`; the amendment does not create it. Q7.5's "the contract wins" does not decide between contract files.
   - Fix, in the same correction batch: K2 and V24 say that no planned signed leg triggers a reset. K2 acts only on the `e`-absent sign until a run that does trigger one records a code.
2. **Minor: no named fake control produces the case (B).**
   - The sentence says the fake "gives both answers in test (a)". Q3.2 `survivors` and Q3's `reset_encrypted(acct[, survivors])` act on zones that survive.
   - A device's own `MGEpoch`, saved after the reset in a new zone, is not in a zone that survived the reset. No Q3 or Q3.5 control makes it read back with `e` absent until its keys arrive.
   - Fix: name the control, for example a `reset_encrypted` option or a Q3.5 injection for "keys not yet arrived" on an account's own new records.
3. **Nit:**
   - "its `survivors` variant": Q6 `encrypted_data_reset` has several `survivors` variants.
   - The Status line should also say that the sentence replaces the deleted clause ", and whether ... is known only from the signed runs (A18)".

## D2: Q11 label corrections (in force, not under review)

Not reviewed for approval. They interact with D3's second sentence, below.

## D3: Q11 opening, the three sentences from "For an item still in P24"

Verdict: not approvable as written. One major.

Confirmed:

1. **Major: closed rows can narrow the removed P24 text (C).**
   - The never-narrows rule applies only "For an item still in P24". Q10 then reruns "these tests", meaning the cells. Several closed cells leave out clauses that their removing commit deleted from P24:
     - X2 K10.2 (`98382e55`): "edits made before an identity read that returns another identity are marked and never sent".
     - X3 K13.1 (`70901b08`): "(and a save conflict's first read)" and "the entity is sent after".
     - X4 K17.4 (`36734582`): "changes no entity; a save meeting unknown item still drops its tombstone; a pass interrupted by a kill marks the delivered records again".
     - X5 K5.2 (`303ed37a`): "a landed earlier attempt's values win; no previous epoch on a first publication".
   - Fix: for a closed row, the test is every clause of the P24 text that its removing commit deleted, plus the cell's "with" references. Each closed cell names that commit. Or restate the full text in the cell.
2. **Minor: the cells do not record what the sentence says they record (A, C).**
   - The sentence says each closed cell records "the commit that removed the item from P24 and the evidence its review log cites, clean or dirty as each receipt records".
   - The K9.5 cell (removed by `f6236e81`) and the K17.4 cell (`36734582`) name no commit, and K17.4 gives no clean or dirty state.
   - The K10.2, K13.1 and K12.4 cells give only a log link. X2's cited 29-test receipt is dirty (its review log: "The cited 29-test receipt reports a dirty checkout").
   - Fix: add the commits and the clean or dirty state of each receipt, or reword to "or its review log records".

## D4: K5.2 `Active` row (X5), the List cell from "Removed from P24 in `303ed37a`"

Verdict: not approvable as written. One major.

Confirmed:

1. **Major: X5's state is not decided (A, B, C).**
   - The P24 preamble says "An item is closed by removing it here in that commit" (spec-writer). Q10 R2066 requires items "removed from P24 or O19 in a spec-writer commit".
   - `303ed37a` is `Role: integrator`.
   - The cell keeps "P24 test closed", adds a spec-writer closure review, and admits "They do not show K22.2 'previous epoch retention'", which its own test cell requires ("with K22.2 'previous epoch retention'").
   - D3's "with" rule covers only items still in P24, so X5 is neither open in P24 nor closed under Q10. In effect this decides the brief's D4 without saying so.
   - Fix: decide explicitly. Either:
     - return X5 to P24's Open list, and close it in a spec-writer commit once K22.2 is shown; or
     - state that X5 does not meet Q10's R2066 criterion until such a commit.
2. **Minor: 11 days against the removed 10 days (A, B, C).**
   - The removed text: "a zone the new binding saw 10 days before `done`". The Q11 test cell says the same.
   - `cloud_publication.rs` line 1346 sets `first_seen_ms == ORIGIN` and `done = ORIGIN + 11 * DAY + 123`. The 11-day value is an equally strong instance of the rule: earlier than `done`, under 30 days, and `first_seen_ms` moves to `done`.
   - Fix: say so in the cell ("any first-seen instant earlier than `done`; the test uses 11 days"), or change the test to 10 days.
   - The clause claim itself is borne out: line 1163ff asserts `landed.previous == None` on a first publication. The lost-reply test asserts that "earlier attempt's time/previous win".

## D5: OP5 and OP9 test cells, "written now against the approved `sync/cloudkit-records#K19.2` 'Joining'"

Verdict: approvable as written. No reviewer finding. This record found nothing: the cells restate that an inactive draft does not apply, and change no test clause.

## D6: OP1 (P3 `FetchDrained`) test cell, the Copy Debug Info clause

Verdict: approvable after fixes.

Confirmed:

1. **Minor: the producer and surface are unnamed (A, B, C).**
   - `sync/status-strings` gives `sync.action.copyDebugInfo` to "U153's diagnostics copy", and its row list has no A7 entry. V26.1 says only "logged, as an A7 failure for Copy Debug Info".
   - The clause says "from its producer or from the surface's action in a test host". It names neither, states no dependency on U153, and gives no lane.
   - A producer-only test passes even if the action does not call that producer, so the clause could narrow P24 (d).
   - Fix:
     - Name the producer as the function whose output `sync.action.copyDebugInfo` writes, and require a check that the action uses it.
     - Add an A7 row to that action's row list in `sync/status-strings`, or the text it appears under.
     - State that the (d) test needs U153's surface.
     - Name the lane: `CoreBridgeTests`, or an app-hosted test in the VM.

## D7: Q2 row for other `cloud_*.rs` and `CoreBridgeTests` files

Verdict: approvable after fixes.

Confirmed:

1. **Minor: precedence over P24 text that names a file is unstated (A, B, C).**
   - P24 names files:
     - P10.3 (Confirmed): "Test (U113 `cloud_policy`, A21 false)".
     - V17.6 (Open): "the P24 `cloud_recovery` fault tests".
     - P6 gate row (Open): "the P24 `cloud_recovery` tests".
   - The Q11 cells repeat these names. Under D3, a cell restating them "never narrows" them, while the new row lets a test "stay in the file it was written in".
   - Q11's in-force "it lives where Q2 puts the section it tests" still speaks of sections. Any new test written in another `cloud_*` file would also "stay" there.
   - A severity of major from one reviewer is not upheld. The ambiguity decides evidence location only, not behavior, and a one-sentence fix closes it.
   - Fix:
     - State whether a file named in P24 text binds.
     - Point Q11's location sentence to the new row.
     - Limit the row to tests already in those files at `52842733`, or allow new ones explicitly.

## Not part of this approval

This is an independent review record, not an approval. Approval of these parts and the Status-line changes are for Antoine or the master agent.

## Settled 2026-11-10 (spec writer)

Session `s-20261110T085429Z-165d83d3`, implementer view, at `origin/master` `a0aefad8`. Uncommitted. The master agent's direction: settle only clarifications that preserve the approved contract. Kept unchanged: no signed reset; strict proof of the actual original events and their order; `recovery_check` equal to `sync_now.exchange`; every account fence; the fresh close's cancellation/drain, join, `close.integrity`, Core close and `close.joined`. No waiver, signed pass, schema, receipt or `core/`/`app/` change. No Status line reads Approved. An independent check decides.

Changed files: `specs/conformance/cloudkit-agent.md` (J4.1 and J5.1 rewritten, J3.2 pointer and Status quote); `specs/conformance/cloudkit-acceptance.md` (Q7.3 pointer, Q11 opening, K5.2 `Active` cell); `specs/sync/policy.md` (P24 K2 item, Status line); `specs/conformance/interop-vocab.txt` (`engine_order_not_shown`); the J3 command 5 brief, `J3-mac-command5-recovery.md` (S2 and S3 notes).

### J4.1

| Finding | Settled | Where |
|---|---|---|
| 1 major, pair link | Applied. Each state save names the sends whose per-record results the saved state reflects. The agent derives the list from the saved state, never from journal order. This is separate from J4's durable-operation reference. A pair is a result of send S and the first state save, in append order, of the same engine instance that names S. A state save without the fact, or a label that names two sends, is a failed evidence check. | J4.1 "Pairs" |
| 2 major, no state save in own instance | Applied explicitly: such a result forms no pair and needs no shown order for A6. Another instance's state save never pairs with it. | J4.1 "Pairs", second bullet |
| 3 major, open class of order facts | Applied as option (b) without a new mechanism. Append order (`record_seq`) is the only shown order. No tick, counter or instant shows, confirms or removes an order. In-force J4 ("preserve callback order") and J7's app-hosted "actual adapter event order" check are what make append order the delivery order. The draft's tick cross-check is removed. The brief's C4 still holds: append order must show the result first. | J4.1 "Shown order" |
| 4 major, two classifications | Applied the review's suggested fix. A reversed shown order is an A6 observation (`false`, Q7.4) only for a build whose producer passed that J7 check at the run's source commit. Otherwise it is a failed evidence check. Either way the run fails; only the contract-correction route differs. | J4.1 "Reversed order" |
| 5 minor, failure output | Applied. The fixed reason is `engine_order_not_shown`, added to `interop-vocab.txt`. The Q7.4 runner records it for leg and ledger checks, naming the check and each `record_seq`. The agent records it only for its own J5 recovery check. The failing checks are the check that required the order and, where judged, A6's entry. | J4.1 "Failure" |
| 6 minor, label reuse | Applied: an engine-instance label is never reused within one journal, including for a P4 `ConfigureAutomatic` re-creation. Operation labels are also unique per journal. | J4.1 "Engine instances", "Pairs" |
| 7 nits | Applied. The sentence on forms that disagree is gone with the cross-check. The missing-evidence reference now reads "(acceptance Q7.4; for a recovery check, J5)". | J4.1 "No order" |
| J5.1 minor 4 (J4.1 cases) | Applied inside J4.1, since J7 itself is in force: a closing paragraph lists the protocol cases. | J4.1, last paragraph |

### J5.1

| Finding | Settled | Where |
|---|---|---|
| 1 major, original actual result | Applied as clarification. The original exchange's actual result is its terminal `RecordExchange` result, with accounting, journaled under the original `agent_instance`. Without it the exchange is indeterminate and recovery fails, as P12.7 already implies ("a crash ends the exchange without a result"). | J5.1 "The original exchange's actual result" |
| 1, Q5 request log (master's direction) | Applied. The signed lane has no fake request log. After a relaunch, the request-log evidence for the interrupted exchange is only the original process's complete journal records of that exchange: capture (S_c and selected-entity labels), per-record results, fetch and apply observations (own landed saves included), the state saves that name those sends, and the terminal result. Capture, selection and acked set lived in memory (P12.2, P12.7), so tracking cannot supply them. Checked: (1) P12.4 and P12.6, recomputed from those records, against the original terminal result and accounting; an entity counts as acknowledged only through an original K12.3 result or K13.2 landed save at or above S_c; (2) current tracking against the recomputation, as a cross-check only. The relaunched engine's observations are never that evidence and are not merged in. Missing records or any mismatch fails the recovery check. Nothing is filled from fresh accounting, durable rows, an original `done` line or a new exchange. This closes the review's failure case (relaunched engine re-sends, its ACK row makes `succeeded`) without the alternative. | J5.1 "Q5 `results` of an interrupted exchange", items 1 and 2 and the next paragraph |
| 1, "unresolved" cause | Applied. A cause is resolved only by an original journaled event that ties the acknowledgement to the manual exchange's operation label or to an earlier automatic operation. | J5.1 paragraph "Original records stay where they are" |
| 1, alternative: no engine admission before recovery ends | Not applied. It changes launch behavior and needs the master agent's decision. The applied fix does not depend on it. | Open (optional) |
| 2 minor, two meanings of J3.1 | Applied: after a relaunch, J3.1's "original app-hosted process" is the process that ran `close`. J3.1 itself is unchanged. | J5.1 close paragraph |
| 3 minor, scope | Applied: J5.1 covers every end of the app process that the coordinator relaunches after. An end that interrupts a command without a `recovery_check` fails under J5. | J5.1 first paragraph |
| 4 minor, J7 cases | Applied inside J4.1 and J5.1 (closing paragraphs), not as J7 rows, so no in-force section changes. | J4.1 and J5.1, last paragraphs |

### Pointer sentences

- J4 pointer, Q7.5 `engine` words, Q7.6 A6 Note: unchanged. They now point to the settled J4.1.
- J3.2 pointer: extended to "..., and which surviving records are an interrupted exchange's actual result and its Q5 `results` evidence." The Status quote is updated to match.
- Q7.3 kill bullet, last sentence: extended to name "which surviving records are the Q5 `results` evidence of an interrupted exchange". The Status quote is still its opening words. Q5 itself is not changed: it is in force, and the relaunch case belongs to J5.1.

### Provenance of `e7c36c00`

Recorded here, because history is never rewritten. The `e7c36c00` body says "J3.1, J3.2, J4 and acceptance Q7.3, Q7.5 and Q7.6 point to them", and its Spec trailer names `conformance/cloudkit-agent#J3.1`. That commit changed no J3.1 line. The next spec-writer commit for these parts must state this correction in its body and leave J3.1 out of its Spec trailer. J5.1 now cites J3.1's words; J3.1 is still unchanged.

### D1–D7 (still under review, not part of the approval of J4.1 and J5.1)

| Part | Applied | Left open, and why |
|---|---|---|
| D1 | Nit: "its `survivors` variant" became "its `survivors` variants". The `sync/policy` Status line now quotes the deleted clause the sentence replaces. | Minor 1: K2 and V24 A18 still expect a signed reset. These are approved texts in other files, so changing them goes beyond a wording fix. Minor 2: naming a fake control for "keys not yet arrived" is a new control. |
| D2 | Not reviewed for approval. | None. |
| D3 | Minor 2, wording: "with each receipt's clean or dirty state, as the cell or its review log records them". | Major 1: the test of a closed row needs a decision. Minor 2 remainder: the K9.5 cell still names no `f6236e81`. The K17.4 cell still names no `36734582` and no receipt state. Both cells are in force, so they are not edited here. |
| D4 | Minor 2: the K5.2 `Active` cell says the tests use 11 days where the removed text says 10. Any first-seen instant earlier than `done` and less than 30 days before it is an equal instance. | Major 1: X5's state (back to P24 Open, or not meeting Q10) needs a decision. |
| D5 | Nothing to apply. | None. Approvable as written, per the review. |
| D6 | None. | Minor 1: naming the producer, an A7 row in `sync/status-strings`, the U153 dependency and the lane are content additions. |
| D7 | None. | Minor 1: whether a file named in P24 binds is a precedence decision, as is limiting the row to existing tests. |

### Ready for approval, in the spec writer's view

- `conformance/cloudkit-agent#J4.1` as settled, with the J4 pointer, Q7.5's `engine` words and Q7.6's A6 Note. Every confirmed finding is applied. Nothing new is added beyond how the journal shows the order that in-force J4, A6, J7 and Q7.4 already require.
- `conformance/cloudkit-agent#J5.1` as settled, with the J3.2 pointer and the Q7.3 sentence. Every confirmed finding is applied, and so is the master's Q5 direction. The optional alternative stays open. It is not needed for the fix.
- The new text was not independently re-reviewed. It includes the classification rule of J4.1 "Reversed order", `engine_order_not_shown`, the reflected-sends fact, and the J5.1 Q5 paragraphs. These need the independent check before any Status line changes.
- D1 is approvable once its two minor findings are settled or accepted as follow-ups. D5 is approvable now. D3, D4, D6 and D7 stay under review.

## Independent check of the settled text (2026-11-10): HOLD

One read-only reviewer, implementer view, against cloudkit-agent J3-J7, acceptance Q1, Q3.7, Q5 and Q7.3-Q7.6, policy P12.2-P12.7 and cloudkit-records K12.3, K12.4, K13.2 and A6. Approvable: the J4, J3.2, Q7.3, Q7.5 and Q7.6 pointer sentences and both Status lines; they are approved only together with their targets. Not approvable yet, so they stay under review:

- J4.1: (1) pairs are per send while A6 is per batch, so a later batch pairs with an earlier save; name batches (send and batch label) instead. (2) The "sends a state save reflects" fact is new and defined only by example; make one derivation the only allowed one (the engine's pending list inside that state-update handling, net of adapter `UpdatePending` removals). (3) An empty list exempts a result from any order; limit the exemption to an engine instance that ended before any later save, and require at least one shown pair for A6 `holds`. (4) The reversed-order rule cites a J7 check no spec names and no runner input carries; name the evidence (a passing MG result for J7's adapter-event-order case at the same clean commit, referenced from `signed-evidence.json`) or require each engine observation to be durably appended before its callback returns. (5) Ledger wording does not fit Q7.4's values. (6) "The journal shows order only between records of one engine instance" is too absolute; limit it to two engine observations.
- J5.1: (1) each per-record result and own landed save needs its image seq (or whether it is at or above S_c) and the held flag; (2) define "while the exchange ran" as the original instance's records between the capture and the terminal result, in `record_seq` order; (3) state that every pair among those records must show the result first or the recovery check fails with `engine_order_not_shown`; also name batches as in J4.1 (1).

Preserved and confirmed: no signed reset, no waiver, no product choice, `recovery_check` = `sync_now.exchange`, J5's account fences and deadlines, the fresh close order, original callback namespaces, and Q5 never filled from fresh accounting, durable rows, a `done` line or a new exchange. This is a review record, not an approval.

## Second settling pass (2026-11-10, spec writer)

Session `s-20261110T085429Z-165d83d3`, implementer view, at `origin/master` `927a7d19`. Uncommitted. Input: the HOLD list above and the master agent's decisions. Kept unchanged: no fresh SDK accounting import, no signed reset, `recovery_check` = `sync_now.exchange`, every account fence and deadline, the fresh close (cancellation/drain, join, `close.integrity`, Core close, `close.joined`), original callback namespaces. No waiver, signed-pass claim or `core/`/`app/` change. Both Status lines still read Under review; an independent check decides.

Changed file: `specs/conformance/cloudkit-agent.md` (J4.1 and J5.1 only). Not changed: `cloudkit-acceptance.md`, `sync/policy.md`, `interop-vocab.txt` (`engine_order_not_shown` is already listed; no new interface name), and every pointer sentence.

### J4.1

| HOLD item | Applied | Where |
|---|---|---|
| (1) pairs per batch | A batch is the records of one sent-record-zone-changes event. Each per-record result carries its send's operation label and its batch label. A state save names batches (send and batch label). A pair is a result of batch B and the first state save of the same engine instance that names B; a save that names only other batches forms no pair with B's results. | J4.1 "Batches", "Pairs" first bullet |
| (2) one derivation | The only derivation: inside the state-update handling, the agent reads the engine's pending record-zone changes. The save names B when that list no longer holds at least one record of B, not counting a record that the adapter removed through `UpdatePending` after the core built B. Journal order, nearby records, instants and later state never supply the list. | J4.1 "Reflected batches" |
| (3) empty-list exemption, A6 `holds` | A result with no naming save needs no order only when no state save of its engine instance follows it in append order (for example, a kill). Any other unpaired result has a missing order and fails with `engine_order_not_shown`. A6 is `holds` only when its step logs show at least one pair, each with the result first. | J4.1 "Pairs" second bullet; "A6's entry" |
| (4) reversed-order evidence source | Master's choice: in the signed test evidence path, the agent durably appends each engine observation, with the actual facts of that callback, before the delegate callback returns. This applies to the agent's step journal only. It changes no production callback timing, locking or journal. Append order is delivery order by construction. The J7-check gate is gone: a reversed pair is A6 `false`. | J4.1 "Shown order", "Reversed order" |
| (5) ledger wording | A failed evidence check fills no ledger value. It is never `holds` or `false`, and Q7.6 allows no `unverified` for A6, so the run fails (Q7.4). The old wording, in which "A6's ledger entry" fails, is gone. | J4.1 "Failure", "A6's entry" |
| (6) scope of "shows order" | Now "shows the order of two engine observations only when both belong to one engine instance". | J4.1 "Engine instances" |
| (7) label nit | The HOLD list does not spell this out. Taken as making all label uniqueness rules use the same words: engine-instance, operation and batch labels are each never reused within one journal. This replaces "An operation label names one send only within one journal". | J4.1 "Batches" |
| missing or reversed required pair | `engine_order_not_shown` when a required pair is missing or reversed, when an unpaired result has a missing order, or when a failed evidence check applies. A reversed pair is also A6's `false`. | J4.1 "Failure" |
| buffering example | Now "one whose callback a kill ended before its append". | J4.1 "No order" third bullet |
| J7 cases | Changed to batches. Added: a later batch after a save that names only an earlier batch; an unpaired result followed by a later save; a reused batch label; a journal with no pair. | J4.1 last paragraph |

### J5.1

| HOLD item | Applied | Where |
|---|---|---|
| (1) image seq or at-or-above-S_c fact, held flag | Each per-record result and own landed save that acknowledges an image carries the image's seq, or whether the image is at or above S_c, and whether the image was held. An entity counts as acknowledged only when that fact puts the image at or above S_c. It is held back when the held flag says so (P12.4's first row). | J5.1 "Q5 `results`" evidence list; item 1 |
| (2) "while the exchange ran" | The original `agent_instance`'s records after its capture record and before its terminal-result record, in `record_seq` order. The clause "when the original process journaled it" is dropped: a missing capture already fails the check. | J5.1 "Q5 `results`" second sentence |
| (3) required pairs, batches | Every pair that a per-record result among those records requires must show the result first. Otherwise the recovery check fails with `engine_order_not_shown`. State saves and per-record results name batches. | J5.1 "Q5 `results`", end of first paragraph; "What the original process leaves" paragraph |
| (3) P12.4-row facts in the missing-records clause | The failure clause now also covers: a missing own landed save; the image seq (or at-or-above-S_c fact) or held flag of a counted acknowledgement; the facts that place a selected entity in its other P12.4 row, or an incoming entry in its row. | J5.1, paragraph after item 2 |
| J7 cases | Added: an acknowledgement without its image seq or held flag; a missing or reversed required pair. | J5.1 last paragraph |

### Pointer sentences

The HOLD list names no pointer nit. The independent check found the J4, J3.2, Q7.3, Q7.5 and Q7.6 pointers approvable together with their targets. They and both Status lines are unchanged.

### Choices made within the direction (for the independent check)

- J5.1 pairs: the state save that a result in the exchange window needs may come after the terminal-result record. It is taken from the original process's records, not only from the window. Otherwise an exchange whose last state save follows its terminal result would fail.
- A6's "at least one pair" is judged over all the step logs that the entry is judged from in that run (Q7.6). It is not judged per leg.

### Open: needs a decision, not applied

1. **Batches that no derivation names (likely to fail valid runs).** Under item (2)'s derivation, no state save names a batch while every record of that batch is still in, or back in, the engine's pending list when the state update is read. Two causes:
   - every result of the batch is an error the engine retries itself (K12.4 "Failed saves": for example `networkUnavailable` or `requestRateLimited`);
   - the adapter added every name back before that read (a Saved result that leaves the entity pending, or a `batchRequestFailed` or `limitExceeded` add-back, K12.3 and K12.4).

   Under item (3), such a result in an engine instance that saves state again fails with `engine_order_not_shown`. The `engine` leg's manual calls with the VM offline and while throttled (Q7.5) are likely to produce such results. Options:
   - (a) such a batch needs no pair, because its results leave nothing in the saved state to reflect;
   - (b) keep the failure;
   - (c) choose another derivation.

   Each one decides what A6 evidence counts, so it is not a wording fix.
2. **Delivery order "by construction".** The append-before-return rule gives delivery order only if one engine instance delivers its next callback after the previous one has returned. A6 says the callback "may await durable persistence before returning", but no contract text says that delivery is serial. If it is not, a statement or an assumption is needed. That is a contract addition, not made here.
3. **Item (7).** The interpretation above needs confirmation from the reviewer who raised it.

## Independent check of the second settling pass (2026-11-10): HOLD

One read-only reviewer, implementer view (plus the CloudKit SDK headers for event delivery). Resolved from the first HOLD: J4.1 batches, the reversed-order evidence (append before the callback returns, signed test path only, J7 gate removed), the shown-order scope; J5.1 image seq or at-or-above-S_c fact, held flag, the capture-to-terminal window and required pairs. Pointer sentences and both Status lines: approvable together with their targets. Still blocking:

- B1 (J4.1, inherited by J5.1; owner choice): batches no state save names make valid runs fail: all results are errors the engine keeps pending (K12.4), adapter add-backs after the sent handler, and, read literally, the `UpdatePending` exclusion also catches a cleanly Saved name, so an ordinary batch may never be named. Proposed fix: (a) count a name removed through `UpdatePending` only if the engine's list still held it at removal; (b) do count a record the list holds only because of an adapter add made after B's sent event returned; (c) a batch whose every result is an error the engine keeps pending needs no pair.
- B2 (J4.1 "A6's entry"): a reversed pair is both A6 `false` and a failed check with no value; say `engine_order_not_shown` other than a reversed pair fills no value, and a reversed pair is `false`.
- B3 (J4.1 "A6's entry"): "at least one pair" fails valid `--leg` runs without engine sends; require the pair in the `engine` leg's step logs only, a run without that leg fills no A6 value, other legs only add pairs.
- Minor, same pass: M1 cite `sync/cloudkit-recovery` V3.1 rule 2 and V10.2 after "by construction" (serial delivery is in force, so the remaining gap is not an assumption); M2 classify a reused engine-instance, operation or batch label as a failed evidence check; M3 J5.1 "other P12.4 outcomes (clean, dropped unsent, blocked, outstanding)". Risk to note: the live pending list may change before the state serialization, which J7's adapter-event-order test should exercise.

Nothing is approved; all named parts stay Under review (R2066).
