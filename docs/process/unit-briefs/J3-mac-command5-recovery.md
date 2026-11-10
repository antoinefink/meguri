# J3.2 Mac command 5 recovery and the close successor

2026-11-10. Spec-only clarification; no waiver, schema, receipt or code change. No pass is claimed.

Implementer view. Specs read at `origin/master` `5d15adf9`. The cloud specs (`specs/conformance/cloudkit-*.md` and `specs/sync/`) had no change from `afaf6cc7` to `2d11ce88` (`git diff afaf6cc7 2d11ce88 -- 'specs/conformance/cloudkit-*' specs/sync` is empty). The 2026-11-10 amendment for S2 and S3 (agent J4.1 and J5.1, with pointer sentences) is under review; the quotes below are of the approved text. Sources: `specs/conformance/cloudkit-agent.md` (cited as "agent") and `specs/conformance/cloudkit-acceptance.md` ("acceptance"). The packet `build/cloud-original-mac-ack-minimal-packet-20261110.md` and the review logs `cloud-selected-close-20261110.md` and `cloud-selected-restoration-20261110.md` show only what is proposed. They are not authority. Three independent readings agreed, and one independent review was applied. Every quote below was checked against the source.

## Question

Today the source refuses a Mac recovered exchange (command 5, `sync_now`) until there is proof of the original n1 ACK. The proposed implementation validates:

- the retained pre-scan original ACKed row, plus the original edit and exact exchange witnesses;
- the full original composite journal proof: ACK, tick, batch return and the later state save.

It does this separately from the empty fresh SDK ledger. May that fully witnessed recovered completion permit a typed historical `selectedCloseProof` for command 6, while the fresh runtime still performs the actual integrity, cancellation/drain and join checks?

## Answer

**Yes, with conditions.** A recovered command 5 `done` is a terminal `done`, so `close` is its normal successor. The contracts do not name `selectedCloseProof` or any close-state type, so its representation is a source design choice. It may only carry command 5's proven completion forward. It cannot supply any of close's own evidence.

No waiver and no product decision by Antoine is needed. The source step may proceed under the conditions below. Before any signed selected-leg claim depends on it, the spec writer must resolve one literal conflict (S2, which process close joins) and one ambiguity (S3, what "precede" means for journal order).

### Conditions

1. **The recovered terminal comes first.** Command 5's accepted command line must carry `recovery_check` equal to `sync_now.exchange`. If it is absent or different, recovery records `failed` and no close follows. Command 5 gets its own `done`, with `recovered_from` naming the original `started`, and only after a successful J5 `sync_now.exchange` check. That check records its observations under the command 5 identity and the new `agent_instance`. The close proof is limited to the facts that justified that `done`. It is rebuilt from the same durable row and journal evidence, never from the `done` line alone. Failed, missing or indeterminate recovery records `failed`, and no close follows.
2. **Recovery content is the full `sync_now` content.** The recovered `done` carries:
   - the original issued exchange and its actual `succeeded` accounting;
   - the Q5 `results` check on that accounting, recomputed from the request log and tracking: `succeeded` only when every selected entity has an ACK at or above S_c, with held-back work and media reported apart;
   - the revision-specific durable ACK of the same edited n1 revision that position 4 retained, with its original event/operation proof;
   - pending/paused/missing media, reported separately;
   - relaunch integrity and the same account/owner checks.

   None of these is enough alone: accounting, an ACKed row, selected count > 0, a note readback, a copied `done` line or a matching digest. The original accounting is checked, not passed through.
3. **Durable evidence and journal facts must agree.** The retained original row and the original edit/exchange witnesses must match the original composite journal facts. Phase from the journal alone is rejected. A fresh-runtime ACK of the current row is neither the original exchange nor an earlier acknowledgement. It cannot satisfy command 5 (inference from agent J3.2).
4. **The original journal order must meet J4.** Per-record result observations must precede the state-save observation that reflects them. Tick order inside one original instance namespace is acceptable only if append order also agrees for result versus state save. If the original journal appends the result after the state save, do not treat tick order alone as sufficient. Recovery then fails until the spec writer settles S3.
5. **Validate before successor work.** The proof is rebuilt and checked before close does any database or SDK work. If another fresh runtime starts before close, it rebuilds the proof from durable evidence the same way. An in-memory carry-over alone is not enough.
6. **Keep the two histories separate.** Original callbacks keep their recorded order, engine-instance label and command identity. Late callbacks under earlier commands stay where they are. Nothing is relabelled as fresh-instance work or as manual-exchange work. The fresh SDK ledger holds only its own admission and events. An automatic ACK is not attributed to the manual exchange. An ACK whose manual-versus-automatic cause is unresolved may satisfy the n1 revision only through the earlier-automatic-ACK path, with its original event/operation proof. It is never reported as the manual exchange's own acknowledgement.
7. **No replay and no new action.** Do not invoke `originalExchange`, the edit, publication or adoption again. Do not issue another exchange. No interruption point other than `sync_now.edit` / `sync_now.exchange` is granted. Recovery and close keep the product's existing account fences, deadlines and cancellation rules.
8. **Close stays fresh.** Command 6 produces its own `close.integrity` and `close.joined` in the fresh runtime. No historical value can satisfy them.
9. **The coordinator order is unchanged.** The coordinator checks the recovered command 5 `done` and validates the evidence behind it before it delivers the next command. Mac close is delivered only after iPhone positions 4–5, and only after the coordinator compares the actual Mac and iPhone observations. The compared Mac payload is the committed edit retained at position 4. Command 5 must prove the ACK of that same revision. (Requiring command 5 to restate the payload would be stricter and allowed, but the contract does not ask for it.)
10. **Keep the refusal until the proof exists.** Until every part above is implemented and evidenced, the Mac refusal stays (`implementation_missing` / failure without successor). Keep the existing refusal tests. A parser/DTO positive is not real-process or signed evidence.

## Governing text

| Spec id | Quote | Bearing |
|---|---|---|
| agent J3 table | "`recovery_check` \| Reference to the selected leg's expected recovery check if this command may be interrupted; otherwise absent" | C1 |
| agent J3 | "The coordinator delivers at most one command without a terminal result per device. It checks that result before delivering the next command." | C9 |
| agent J3 | "Unknown operations, malformed arguments and operations outside the selected leg fail before execution. The protocol does not grant additional recovery or destructive actions." | C7 |
| agent J5 | "When a command has `started` but no terminal record, the agent first opens the same core and tracking state. It executes the leg's `recovery_check` against the actual durable state and, where required, real CloudKit observations. It records those observations with the original command identity and the new `agent_instance`." | C1, C6 |
| agent J5 | "The check must distinguish the committed operation from one that never committed or has an indeterminate outcome. It verifies the leg's expected recovery, including required integrity checks after relaunch. A kill does not excuse a lost committed write (Q5). Neither a copied `done` line nor the presence of an expected digest alone proves that this app executed the operation." | C1, C2, C3 |
| agent J5 | "Only a successful recovery check permits a `done` record with `recovered_from` naming the original `started` record. It proves the planned interrupted outcome; it does not reissue a mutation to obtain that outcome. Missing recovery instructions, missing evidence, a failed check or an indeterminate outcome stops the run. Failed or indeterminate recovery records `failed`; the agent cannot continue to the next command." | C1, C7 |
| agent J5 | "If the journal ends after a terminal `done`, the next command is its successor. If it ends before any `started` for the next accepted command, that command may start normally. These are progress rules for this agent, not a new sync retry mechanism. Recovery and normal execution retain the product's existing account fences, deadlines and cancellation rules." | Close is the successor of a recovered `done`; C7 |
| agent J5 | "The coordinator kills and relaunches the actual app as Q7.3 specifies. The agent cannot survive in another process." | S2 |
| acceptance Q7.3 | "The agent runs inside that process and dies with it, so the host coordinator (`tools/mg cloudkit-signed`) relaunches the app-hosted test run on that device. The new agent reads the command file and its own step log, finds the last step it logged as done, and continues from the next one; a step logged as started and not done is the step the kill interrupted, and the agent checks its outcome (the leg's expected recovery) before it continues." | Successor; S2 |
| agent J3.2 (Bootstrap and fixed edit) | "Flush the update and inspect its committed row. Retain the actual operation provenance, revision, canonical digest and checks for both the changed field and preserved content/relations." | C2, C9 |
| agent J3.2 (Original exchanges) | "The Mac must report actual `succeeded` accounting and revision-specific durable acknowledgement of the edited note. An earlier automatic acknowledgement may satisfy that revision only with its original event/operation proof; do not attribute it to the manual exchange. Report actual pending/paused/missing media separately on both devices, without turning collection success into media success. Do not issue another exchange to obtain a requested outcome." | C2, C3, C6, C7 |
| agent J3.2 (Original exchanges) | "The coordinator compares the complete canonical payload with the Mac's actual committed payload, including the metadata that canonical projection represents; do not mask metadata differences." | C9 |
| agent J3.2 (Original exchanges) | "`close` retains J3.1's coherent final integrity and joined-shutdown requirements." | C8, S2 |
| agent J3.2 | "The coordinator first completes Mac positions 1–3, then iPhone positions 1–3, then Mac positions 4–5, then iPhone positions 4–5. It compares the actual source and peer observations before delivering either close. It delivers no successor after failure." | C9 |
| agent J3.2 (Fresh runtime) | "Re-establish account authority in J2 order and reconstruct only the mappings and phase supported by actual durable operation/row/progress evidence. Completed commands are not rerun, old sessions are not recreated, and journal phase alone does not prove that a referenced entity or operation exists. Missing or changed required progress fails before successor database/SDK work." | C1, C3, C5 |
| agent J3.2 (Fresh runtime) | "For intentional interruption in this slice, require `recovery_check` equal to `sync_now.edit` on Mac position 4, and `sync_now.exchange` on either `sync_now`. It is absent elsewhere; this slice grants no intentional interruption of bootstrap or close." | C1, C7 |
| agent J3.2 (Fresh runtime) | "`sync_now.exchange` must identify that original exchange's actual result and durable accounting, not run a new exchange. Both require relaunch integrity and the same account/owner checks. An uncommitted required edit, indeterminate exchange, missing recovery inspector or other unproved interrupted outcome fails without replay or successor. Only proven original completion permits `done` linked by `recovered_from`. A new runtime after a completed command must also retain these no-repeat guarantees." | C1, C2, C5, C7, C10 |
| agent J3.2 | "J4–J6 and Q5 apply throughout. Keep original callbacks and failure cancellation/ join ordering. Protocol/fake tests establish no signed execution." | C2, C4, C6, C10 |
| agent J3.1 | "Each command's `done` requires its corresponding actual observations below; a command cannot supply them as expected-success values." | C8 |
| agent J3.1 table | "`close.integrity` \| the same full local integrity/schema/reference checks at a coherent safe final snapshot after owned work is joined" and "`close.joined` \| actual coordinator cancellation/drain, owned work joined and Core closed, in that order; terminal journal remains durable/readable" | C8 |
| agent J3.1 | "`close` is an explicit final command. Stop and join the actual coordinator's owned work before the final integrity snapshot and Core close. Only then record joined shutdown. The host retains that terminal journal and joins the original app-hosted process; process exit or a lost connection alone is not clean close. Failure follows J4 cancellation/join rules and starts no successor." | C8, S2 |
| agent J3.1 (`sync_now`) | "neither selected-count greater than zero nor a successful note readback proves these facts. Earlier automatic acknowledgements may satisfy a current revision when their actual operation/event and durable state are retained; do not falsely attribute them to this manual exchange." | C2, C3, C6 |
| agent J3.1 | "Missing inspection or operation code is `implementation_missing`." | C10 |
| agent J4 | "Engine observations preserve callback order, the engine-instance label and the related operation label. Per-record result observations precede the state-save observation that reflects them. The journal must retain enough actual order to judge Q7.6's A6; a summary saying A6 holds cannot replace those events. A state-save or apply observation references the actual durable operation it reports. Background callbacks may follow their command's `done`; they retain that command identity and never add another terminal result. Automatic callbacks without a causal command use the latest started command as the observation boundary, without claiming it caused them." | C4, C6, S3 |
| agent J4 | "Run every check required by Q5 and the selected leg. The caller cannot select fewer checks." | C2 |
| agent J4 | "`done` contains the actual operation result and the checks required for that command by Q5 and the selected leg. … It does not accept an expected result supplied by the command as an observation. … A Sync Now result comes from the real coordinator and its tracking, not a command's requested result." | C2, C8 |
| agent J6 | "The coordinator validates header and command bytes, journal transitions, actual build bindings and the evidence behind operation outcomes." | C9 |
| acceptance Q5 (`results`) | "after every exchange \| the exchange's result follows P12.4 and P12.6 recomputed from the request log and tracking: `succeeded` only when every selected entity has an ack at or above S_c, held back and media reported apart" | C2 |
| acceptance Q5 (`integrity`) | "at quiescence, after every relaunch \| each collection opens, `PRAGMA integrity_check` returns `ok`, its `sqlite_master` equals U22's expected objects, and Meguri's reference check finds no card without its note or deck and no note without its note type" | C2 (relaunch integrity, separate from `close.integrity`) |
| acceptance Q7.5 (`engine`) | "per-record results arrive before the state that reflects them, from the event order in the step log (A6)" | C4, S3 |
| acceptance Q7.6 | "\| A6 \| `engine`, every leg's step log \|" | C4, S3 |
| agent J7 | "Supply only expected outcomes, build receipts or synthetic engine callbacks \| No signed pass or signed ledger observation" | C6, C10 |
| agent header | "This file adds no sync deadline, retry, account permission or acceptance exception." | No waiver |

## Stated vs inferred

Stated in the contracts:

- Only a successful recovery check permits a `recovered_from` `done`, and a terminal `done` makes the next command its successor (J5, Q7.3). The contracts do not treat recovered and live completion differently when they admit a successor.
- Command 5 may be recovered only when its line carries `recovery_check` equal to `sync_now.exchange` (J3, J3.2).
- What a Mac `sync_now` recovery must prove, including the Q5 `results` check, and that it never replays (J3.1, J3.2, J4, J5, Q5).
- A rebuilt phase must rest on durable evidence and be checked before successor database/SDK work (J3.2). A copied `done` line proves nothing alone (J5).
- Close's own integrity and join checks, and their order (J3.1, J3.2).
- The coordinator checks each terminal result and its evidence before the next command, and compares source and peer before either close (J3, J3.2, J6).

Inferred, not stated:

- A typed historical close proof is a valid form of "phase supported by actual durable operation/row/progress evidence", as long as it carries only the recovered command 5 completion.
- The packet's three-way agreement meets J3.2's "revision-specific durable acknowledgement … with its original event/operation proof": the retained ACKed row, the composite SavedRemoved/envelope/return/later-state-save journal join, and the exact original exchange readback. No single part does.
- Tick order inside one original instance namespace satisfies J4 only if it carries the actual callback order and append order also agrees for result versus state save (C4). The contract does not mention ticks; see S3.
- A fresh-runtime ACK of the current row cannot stand in for the original ACK (C3).
- An ACK with an unresolved timing label counts only through the earlier-automatic-ACK path (C6).
- The new SDK may be admitted after the proof is validated. That is stricter than J3.2's "before successor" rule, and allowed.
- J5's "where required, real CloudKit observations" does not require a new CloudKit call for `sync_now.exchange`, because J3.2 defines the required facts as the original exchange's result and durable accounting. J3.2 forbids a new exchange; it does not forbid a read-only observation. A read-only observation is allowed but not required.

## Conflicts and silences

No product decision by Antoine is needed. There is one literal conflict (S2) and one ambiguity (S3). Both need a spec-writer amendment before a signed claim depends on them.

- **S1. Close holding a source-ACK proof (silence).** No contract clause says `close` must hold a source-ACK proof. That gate comes from the implementation (`cloud-selected-close-20261110.md`: "It requires the prior source ACK or peer apply proof"). The contracts neither require nor forbid it. Keeping it is allowed under C1–C10. No decision is needed.
- **S2. Which process the host joins after a relaunch (literal conflict).** J3.2 imports J3.1's close rule, and J3.1 says the host "joins the original app-hosted process". After a kill, that process no longer exists (J5: "The agent cannot survive in another process"; Q7.3: the agent "dies with it"). Read literally, `close.joined` could never be met after any relaunch. The only reading that fits both is the relaunched app-hosted process that runs `close`, joining its own owned work. The source `joinedOriginalWork` check must mean the fresh runtime's own owned work. It must never claim a join of the killed process. The killed process's exit is not a clean close. This does not block the source step. Before a signed selected-leg claim depends on it, the spec writer should amend J3.1/J3.2 to confirm this reading.
  - *Amendment 2026-11-10, under review (R2066):* agent J5.1 "Process boundary at a relaunch", with one pointer sentence in J3.2's "Original exchanges and peer evidence" and one in acceptance Q7.3's kill bullet. J3.1, the smoke, is not changed: it supports no relaunch. J5.1 confirms the reading above. It applies after a kill or an offline-period termination only. The killed process leaves evidence, not work: durable collection, tracking and media state, the run directory, the complete journal records it appended (with their original namespace and command identity; unappended buffered observations are lost) and the host coordinator's retained copies. That evidence is input to recovery and phase reconstruction under J3.2's and J5's rules. The relaunched process recomputes every check of the recovered `done`, including Q5 `results`; an original `done` line or check outcome is input, never the outcome. J5.1 keeps C6 as written: an ACK with unresolved cause is never the manual exchange's own and may count only through J3.2's earlier-automatic rule with its original event/operation proof. It writes C3 into the spec: an ACK event that the relaunched engine delivers cannot satisfy the interrupted command, and a durable ACK row counts only together with the original exchange's result or the earlier ACK's original proof. Command 6 joins only the relaunched process's own work, in J3.1's order, and the host joins the process that ran `close`. `close.joined` never claims that the killed process was drained, joined or closed. A recovered `done` cannot supply `close.integrity` or `close.joined`. The host coordinator's checks before `close` are unchanged. J5.1 changes no account fence, deadline or cancellation rule, and no way service state is shown. Until the Status lines read Approved, treat this as the reading above, not as approved text.
- **S3. Journal order versus tick order (ambiguity).** J4 says result observations "precede" the state-save observation, and Q7.5 judges A6 "from the event order in the step log". Neither says whether that is append position or a recorded order field such as a tick. The packet notes that the runtime "buffers callback traces and appends them after summary facts". If that puts a result after its state save in append order, it is unclear whether tick order alone meets J4. Until the spec writer confirms tick order is acceptable, require both orders to agree (C4).
  - *Amendment 2026-11-10, under review (R2066):* agent J4.1 "Order evidence for engine observations", with a pointer sentence in J4 and pointer words in acceptance Q7.5's `engine` row and Q7.6's A6 row. "Precede" is one engine instance's callback delivery order. The journal shows a pair's order only inside one engine instance and `agent_instance`, and only for a result and a state save that their recorded references link. Append order (`record_seq`) is the shown order. When both observations also carry a callback-order fact such as a tick, the pair has a shown order only when the tick gives the same strict order; when the two differ, or the ticks are equal, no order is shown. The journal does not record when a tick was assigned or a record appended, so neither form overrides the other. So an agent that buffers callback observations and appends them out of delivery order does not show the order J4 requires. No order is shown across instances, without the link, or for a result the journal does not hold (for example one a kill lost from a buffer). `observed_at`, summary facts and durable rows fill no gap. A required order that is not shown is a failed evidence check, including for a recovery check that depends on it. J4.1 sets no ledger value; Q7.4 and Q7.6 judge A6 from the shown order, unchanged. This keeps C4 as written.

## What the fresh runtime must still do

1. Validate the J2/J3 bindings and the coordinator-retained journal prefix before opening the core or starting sync. Check that command 5 carries `recovery_check` equal to `sync_now.exchange`. Reopen the same run-owned collection and tracking with no install, publication or adoption. Re-establish account authority in J2 order.
2. Run the command 5 relaunch integrity check (Q5), the Q5 `results` check and the account/owner checks. Record the recovery observations under command 5 and the new `agent_instance`. Write the linked `done`, or `failed`.
3. Before close does any database or SDK work, rebuild or re-check the close precondition from durable evidence, not from the `done` line.
4. Run command 6:
   1. actual coordinator cancellation/drain;
   2. join of the owned work;
   3. a final full integrity/schema/reference snapshot after the join;
   4. Core close;
   5. only then, `close.joined`.

   Late callbacks are kept as observations under their own command identity. The terminal journal must stay durable and readable.
5. The host retains the terminal journal and joins the app-hosted process that ran close (S2). No step claims a join of the killed process.
6. On any failure, cancel and join through the existing coordinator path, and start no successor.
