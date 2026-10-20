# Card-data function observation

Spec: conformance/extractor-ops
Requirements: R617, R618, R1070, R1886
Anki: 26.09.3
Scenarios: conformance/scenarios/search/extractors/

The observation kind `card_extractors`: the values of the card-data SQL functions of `specs/search/card-extractors.md` (stored memory-state fields, retrievability, relative retrievability, original position) for given arguments. It observes the functions themselves, before any search term, queue order or browser column uses them. `conformance/schema/scenario.schema.json` and `observation.schema.json` hold its shapes.

## E1. Request

`{card_extractors: {inputs: [<input>, ...]}}` in an `observe` list, at most once per observe step. `inputs` holds 1 to 1,000 inputs. Anything else is `invalid_scenario`; the observation has no rejection kinds.

An input is an object with exactly the members `id`, `data`, `due`, `ivl`, `today` and `now`:

- `id`: a non-empty string, unique within the request. It only labels the input and its row.
- `data`, `due`, `ivl`, `today`, `now`: SQL values (E2), the arguments of `search/card-extractors` X1 and X2.

## E2. SQL values

| Scenario value | SQLite value |
|---|---|
| an integer (within ±2^53, `dsl.md` D2) | INTEGER |
| `{int: "<decimal>"}`: an optional `-` and 1 to 19 digits, within the signed 64-bit range | INTEGER, for values beyond ±2^53 |
| a number with a fraction or an exponent | REAL |
| a string | TEXT holding the string's UTF-8 bytes |
| `null` | NULL |
| `{text_hex: "<hex>"}` | TEXT holding exactly these bytes, which need not be valid UTF-8 (`CAST(x'…' AS TEXT)` makes such a value) |
| `{blob_hex: "<hex>"}` | BLOB holding these bytes |

Hex is an even number of the digits `0-9`, `a-f`, `A-F`, possibly none. A boolean, an array, an object of another shape, an `int` text out of range or malformed hex is `invalid_scenario`.

## E3. Value

`{"rows": [<row>, ...]}`, one row per input, in input order. A row is `{id, s, d, dr, r, rr, pos}`:

| Member | Value |
|---|---|
| `id` | the input's `id` |
| `s`, `d`, `dr` | the memory-state field function (`search/card-extractors` X4) with the key `s`, `d` or `dr`, on `data` |
| `r` | retrievability (X5) of `data`, `due`, `ivl`, `today`, `now` |
| `rr` | relative retrievability (X6) of the same arguments |
| `pos` | original position (X7) of `data` |

- A REAL result is written as the 32-bit float it widens (`observations.md` O6): every REAL these functions return is a widened f32. +∞ and −∞ are the strings `"inf"` and `"-inf"`. NULL is `null`. `pos` is an integer.
- Each function is evaluated on its own: a NULL in one member says nothing about the others.

## E4. Comparison

- `r` and `rr` compare within a relative 1e-5 (`observations.md` O7, R1886); every other member, and `"inf"`, `"-inf"` and `null` anywhere, compare exactly.
- The tolerance accounts for system f32 power calculations (KTD24). For some inputs, macOS Meguri and the Linux golden differ by a few ULPs because Apple and glibc math results differ in their low bits. In the FSRS branch of RR, subtracting values near 1 can amplify that error: retrievability or desired retention near 1 may yield a relative difference greater than 1e-5. Those inputs belong to the exact same-platform tests in `search/card-extractors` X9, not these cross-platform scenarios.
- The `fp-libm-boundary` overlays (`float-determinism.md` F7, F8) do not apply to this kind.

## E5. Executors

- **A pure observation.** It never reads or changes the scenario's collection, so it may appear in any observe step, with or without setup, and its value does not depend on the scenario's clock or zone (`dsl.md` D8).
- **Oracle.** Anki's own card-data SQL functions, on the connection of one scratch collection per worker process (never the scenario's), one call per function and input, with the input's values bound with the storage classes of E2. Anki's retrievability and relative retrievability functions also take the next day boundary as an argument, which they never read; the driver passes 0. Results are read back as SQLite returns them. A REAL that is not exactly a widened f32 is a run error.
- **Meguri.** Its four functions of `search/card-extractors` X2 (`meguri_card_f32` with the keys `s`, `d`, `dr`, `meguri_retrievability`, `meguri_relative_retrievability`, `meguri_original_position`), on a connection where `meguri-search` registered them, with the same bound values.
- **Status.** Stable: the oracle driver implements the kind, and every scenario under `conformance/scenarios/search/extractors/` has its golden. Meguri's executor implements it in its search adapter with U82a.
