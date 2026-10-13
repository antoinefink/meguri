# Card data JSON

Spec: model/card-data-json
Requirements: R1460, R1461
Anki: 26.09.3
Scenarios: conformance/scenarios/model/card-data/

The text format of the `cards.data` column: how Meguri reads it into card values and writes it back. What the values mean to the scheduler (when `s` and `d` change, when `pos` is saved) belongs to the scheduling specs; this file is only the format. Number rules are `model/json-numbers`.

## C1. Column

- `cards.data` holds TEXT: a JSON object. A new card gets `{}`.
- A stored value of another SQLite storage class (INTEGER, REAL, BLOB) reads as if the text were empty (C3). The column is `NOT NULL`.

## C2. Keys

| Key | Type (`model/json-numbers` N1) | Value |
|---|---|---|
| `pos` | `u32` | the card's position while it was new, saved when it left the new queue |
| `s` | `f32` | FSRS stability |
| `d` | `f32` | FSRS difficulty |
| `dr` | `f32` | desired retention the card was last scheduled with |
| `decay` | `f32` | FSRS decay the card was last scheduled with |
| `lrt` | `i64` | last review time, epoch seconds |
| `cd` | `string` | custom scheduling data: a string that itself holds JSON text (normally an object) |

## C3. Reading

1. The text is parsed as JSON (RFC 8259; whitespace allowed). Text that is empty, is not valid JSON, or is valid JSON other than an object or an array, reads as no keys at all: every known key absent and no unknown keys.
2. An object with the same known key twice reads as no keys at all (as in rule 1). Repeated unknown keys do not matter for the known keys.
3. Each known key is present when its value fits its type and absent otherwise (a wrong JSON type, a number out of range, a fraction for an integer type, `null`):
   - `pos`: an integer from 0 to 4294967295 (`3.0`, `-1` and `4294967296` do not fit);
   - `lrt`: an integer in the `i64` range (`1.5` and `9223372036854775808` do not fit);
   - `s`, `d`, `dr`, `decay`: any number, read as `f32` (`model/json-numbers` N2), so `5` reads as 5.0 and `1e40` as +∞; a string such as `"1.5"` does not fit;
   - `cd`: any string. Its content is not checked.
4. `s` and `d` are one memory state: when either is absent, both are absent (`{"s":1.5}` reads as neither).
5. `cd` that is the empty string or exactly `{}` counts as absent.
6. An array is read by position: element 1 is `pos`, then `s`, `d`, `dr`, `decay`, `lrt`, `cd`, each by rule 3, with `null` meaning absent. An array with more than 7 elements reads as no keys at all. An array has no unknown keys. (`[1,2,3]` reads as `pos` 1, `s` 2.0, `d` 3.0.)
7. Unknown keys (Meguri, R1461): every member of an object whose key is not one of the seven is kept, in order, with its value as exact text (`model/json-numbers` N7). When an unknown key repeats, only its last occurrence is kept, at that occurrence's place. Anki drops unknown keys; this is the deviation `card-data-unknown-keys` in `conformance/deviations.yaml`.

## C4. Writing

Every time a card row is written, its `data` text is built again from the card's values (Anki does the same, so rounding and key order are applied even to values no operation touched):

1. Members, in this order, each only when present: `pos`, `s`, `d`, `dr`, `decay`, `lrt`, `cd`; then the unknown keys of C3 rule 7, in their kept order with their kept text.
2. `s` is rounded to 4 decimal places, `d` to 3, `dr` to 2, `decay` to 3. The rounding is done in `f32` arithmetic, each step rounded to `f32`: multiply by 10^places (10, 100, 1000 or 10000, all exact in `f32`), round to the nearest integer with halves away from zero, divide by the same power of ten. The result is written per `model/json-numbers` N4, so a non-finite value is written as `null`, and `-0.00004` becomes `-0.0`.
3. `pos` and `lrt` are written as integers, `cd` as a JSON string (N8).
4. No whitespace. With no member at all the text is `{}`, never the empty string.

Meguri never sets `cd` itself (R591); it only carries it.

The custom-data limits Anki applies when an add-on's scheduler code sets `cd` (an object whose keys are at most 8 bytes, at most 100 bytes of text) are checked only on that path, which Meguri does not have. Reading and rewriting never check them.

## C5. Examples

Anki's output after rewriting a card whose `data` held the input (oracle). Meguri writes the same, except that the unknown keys (`zz`) stay in Meguri's output at the end.

| Input | Written back |
|---|---|
| `{"s":123.45678,"d":1.234567,"dr":0.987654,"decay":0.123456}` | `{"s":123.4568,"d":1.235,"dr":0.99,"decay":0.123}` |
| `{"zz":1,"cd":"{}","lrt":5,"decay":0.212,"dr":0.212,"d":0.212,"s":0.212,"pos":0}` | `{"pos":0,"s":0.212,"d":0.212,"dr":0.21,"decay":0.212,"lrt":5}` (Meguri: `…,"lrt":5,"zz":1}`) |
| `{"pos":3,"s":1.0,"d":5,"dr":0.9,"decay":0.1542,"lrt":1791882030,"cd":"{\"x\":1}","zz":7}` | `{"pos":3,"s":1.0,"d":5.0,"dr":0.9,"decay":0.154,"lrt":1791882030,"cd":"{\"x\":1}"}` (Meguri: `…,"zz":7}`) |
| `{"s":2.5,"d":0.0015,"dr":0.125,"decay":0.0005}` | `{"s":2.5,"d":0.002,"dr":0.13,"decay":0.001}` |
| `{"s":0.00015,"d":0.0025,"dr":0.005,"decay":1.0005}` | `{"s":0.0002,"d":0.003,"dr":0.01,"decay":1.0}` |
| `{"s":0.00005,"d":0.0005}` | `{"s":0.0001,"d":0.001}` |
| `{"s":1e-7,"d":1e10}` | `{"s":0.0,"d":10000000000.0}` |
| `{"s":12345678.9,"d":1}` | `{"s":12345679.0,"d":1.0}` |
| `{"s":1e40,"d":1}` | `{"s":null,"d":1.0}` |
| `{"s":-0.00004,"d":-0.0004}` | `{"s":-0.0,"d":-0.0}` |
| `{"s": 1 , "d" : 2}` | `{"s":1.0,"d":2.0}` |
| `{"dr":1,"decay":1}` | `{"dr":1.0,"decay":1.0}` |
| `{"s":1.5}`, `{"d":1.5}`, `{"s":"1.5","d":2}` | `{}` |
| `{"pos":-1,"lrt":1.5,"dr":null}`, `{"pos":4294967296}`, `{"pos":3.0}`, `{"pos":true}` | `{}` |
| `{"lrt":-5,"pos":0}` | `{"pos":0,"lrt":-5}` |
| `{"lrt":9223372036854775807}` | unchanged |
| `{"lrt":9223372036854775808}` | `{}` |
| `{"cd":5}`, `{"cd":""}`, `{"cd":"{}"}` | `{}` |
| `{"cd":"[1]"}`, `{"cd":"{\"a\": 1}"}` | unchanged |
| `{"s":1,"s":2,"d":3}`, `{"cd":"a","cd":"b"}` | `{}` |
| `{"zz":1,"zz":2,"s":1,"d":2}` | `{"s":1.0,"d":2.0}` (Meguri: `{"s":1.0,"d":2.0,"zz":2}`) |
| `garbage`, empty text, `1`, `"x"`, `null` | `{}` |
| `[1,2]` | `{"pos":1}` (`d` missing, so `s` is dropped too) |
| `[1,2,3]` | `{"pos":1,"s":2.0,"d":3.0}` |
| `[null,2,3]` | `{"s":2.0,"d":3.0}` |
| `[1,2,3,4,5,6,"{\"a\":1}"]` | `{"pos":1,"s":2.0,"d":3.0,"dr":4.0,"decay":5.0,"lrt":6,"cd":"{\"a\":1}"}` |
| `[1,2,3,4,5,6,"x",8]` | `{}` |

The scenarios under `conformance/scenarios/model/card-data/` hold these inputs and a few more (`rewrite`, `lenient`, and `unknown-keys` for C3 rule 7); their goldens are the authority. `unknown-keys` also covers a nested unknown value with inner whitespace (`{"zz":{"b":1,"a":[1, 2]},"pos":1}` becomes `{"pos":1,"zz":{"b":1,"a":[1, 2]}}` in Meguri) and an unknown key whose value is `null`, which Meguri keeps (`{"s":1.5,"zz":null}` becomes `{"zz":null}`).
