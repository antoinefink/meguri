# Answering in scenarios: the held study queue and the load balancer

Spec: conformance/answer-ops
Requirements: R1883, R1890, R1893
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/loadbalancer/, conformance/scenarios/sched/answer/

The `answer` op (`dsl.md` D7.2) and the `next_states` observation (`observations.md` O5) compute a card's next states at the scenario's instant. Anki applies its load balancer (`specs/sched/load-balancer.md`) only while it holds a study queue, so these two depend on whether a queue is held. This file says when that is, and how Meguri's executor mirrors it. Nothing here adds an op or an observation kind; the `queue` observation is `queue-ops.md` Q2 (U41).

## H1. Without a held queue

Before any `queue` read, and after anything that drops the queue (H3), neither executor has a load balancer: `next_states` and `answer` use plain fuzz (`sched/fuzz`) whatever `loadBalancerEnabled` says. This is the state every scenario starts in, and the state of every scenario that never reads `queue`.

## H2. Holding a queue

- **Oracle.** A `queue` read makes Anki build the current deck's study queue when it holds none, and keep it. When `loadBalancerEnabled` is on at that moment, the queue carries Anki's load balancer, built from the whole collection (`sched/load-balancer#W2`). While Anki holds that queue, `next_states` and `answer` use it, for any card, in any deck, also one the queue does not contain. A `queue` read while a queue is held reuses it.
- **Meguri.** At a `queue` read, when the executor holds no cache, it builds one as `sched/load-balancer#W2` says when `loadBalancerEnabled` is on, and holds it (with none when the key is off). While it holds a cache, `next_states` and `answer` offer it as the fuzzed-day chooser (`sched/fuzz#Z8`); `answer` updates it (`sched/load-balancer#W8`); `undo` of an answer made while it was held removes the card from it (W9); `redo` changes nothing in it. The queue itself (cards, counts) is not held: `queue-ops.md` Q2.2 still applies to what the `queue` read reports.

## H3. Dropping the queue

Both executors drop the held queue, and with it the load balancer, after:

- an op that changes card rows other than through a flag: `add_note`, `delete_notes`, `suspend`, `bury`, `forget`, `set_due`, `reposition`, `build_filtered`, an `edit_note` or note type change that adds or removes cards, and the undo or redo of any of these;
- an op that actually changes decks or presets: `legacy_update` of a deck or preset, `update_preset`, the deck ops; selecting another current deck also drops it. Selecting the already-current deck alone is not a portable proof of a fresh build; use Q2.3’s deck-switch or raw no-op write;
- a `db_exec` whose statement does not begin with `select` (`record-ops.md` W1);
- an op that fails inside its transaction, `reopen`, and a handoff;
- the next `queue` read after the clock has passed the cache's `next_day_at`.

Production sessions additionally follow `sched/queue-building#B10` for whole-collection, generation and incoming changes. That classifier needs observer payloads and session ownership; a memory-only flag alone cannot keep a production queue valid. The scenario-held cache described here does not implement U43’s mutable presentation queue.

They keep it across `answer`, the undo and redo of an answer, `set_flag`, the tag ops, `edit_note` that adds or removes no card, `set_config` and `remove_config`, `advance` and `set_clock` within the same day, and every observation.

## H4. Rules for scenarios

1. **Answer the head.** With a queue held, Anki answers a card only when it is the first card of the queue's main list or one of its intraday learning cards; any other card makes Anki fail the answer with an invalid-input error, a run error (`anki_error`) on the oracle. Meguri's executor does not check. Scenarios that hold a queue answer only the card the queue presents first, and keep the studied deck's queue simple (one card due).
2. **Config keys.** Anki keeps its queue across a `set_config` of `loadBalancerEnabled`, `fsrs` or any other key the queue build reads, while Meguri rebuilds (`sched/queue-building#B10`). After such a `set_config`, a scenario drops the queue with `db_exec: {sql: "UPDATE cards SET usn = usn WHERE 0"}` and reads `queue` again before observing anything the load balancer affects.
3. **One day.** A scenario does not move the clock across `next_day_at` while a queue is held. (Anki's next states would use the stale load balancer until something rebuilds its queue.)
4. **Read the queue only when none is held.** A `queue` read after an answer reports Anki's held queue minus the answered card, which Meguri's executor does not keep (Q2.2): load-balancer scenarios read `queue` only to start holding one.
5. **Undo.** As for every undo scenario, a `db_exec` that is not a `select` runs before the first `undo`, and it also drops the queue: scenarios that undo an answer read `queue` after that statement and before the answer.

## H5. Required evidence

The six load-balancer cases have historical reference output. Their newly labelled inputs need fresh reference recordings. No current comparison or working product adapter is proved by that earlier output.

U44 connects the answer provider and day chooser, then checks the complete cases on Linux and macOS. H2/H3 define the cache used by those cases. Activation requires successful product results on both systems. Existing request shapes, exact results and approved deviations remain the contract; these edits add no tolerance.

## H6. Position-case evidence

| Case | Requirements |
|---|---|
| `sched/data/positions` | P1; `conformance/dsl#D8.1`; `sched/card-data#K6`-K8 |
| `sched/answer/positions-normal` | H5; `sched/card-data#K6`-K8 |
| `sched/compat/enable-fsrs-live` | `conformance/dsl#D9.2`, D9.3 |

### P1. Required equalities

| Artifact | Commit | Path | Provenance |
|---|---|---|---|
| S0 | `b65827bdb6f21655730f5acf486739ae6d1dd846` | `conformance/scenarios/sched/data/positions.yaml` | archive |
| S1 | `089d266546fb95c2a6aef93ea98268019430ab56` | `conformance/scenarios/sched/data/positions.yaml` | tag metadata |
| G0 | `b65827bdb6f21655730f5acf486739ae6d1dd846` | `conformance/goldens/sched/data/positions.json` | archive |
| G1 | `1f7e1611807ea4b1a2d8906d79fde2c078ea42e8` | `conformance/goldens/sched/data/positions.json` | `Role: oracle-maintainer`; H5 |

- `S1.tags = ["needs:answer.filtered"]`
- `S1` without `tags` = `S0`
- `S1.clock = S0.clock`
- `S1.setup = S0.setup`
- `S1.steps = S0.steps`
- `G1.observations = G0.observations`

Comparison requirements: unchanged. Ownership: `conformance/dsl#D9.2`, D9.3. Engine assertion: `sched/answering#A12` (R53/R706), refusal writes = 0.
