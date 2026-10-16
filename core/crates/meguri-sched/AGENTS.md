# meguri-sched

- FSRS values come only from the `fsrs` crate (`sched/fsrs-transitions` T1): never fill, clip or re-implement its parameters or formulas, and never call a math-library function (`powf`, `expf`, `logf`) on the path from its output to a stored interval. That is what keeps Meguri equal to Anki on each platform (KTD24); `tools/mg fp-probe` checks it.
- Fuzz (`states/fuzz.rs`) is `sched/fuzz` operation for operation: each `f32` step in the spec's order, and the factor from `rand` 0.9.4's `StdRng`. Reordering or widening one changes intervals on a rounding boundary.
- `timing/` is the day math only (`sched/day-clock` D2 to D4, D7, D11); the cache, the writes and the events are meguri-engine's `sched`. Day numbers are `u32` because legacy timing stores a negative day as its unsigned wrap (D4): never clamp or saturate it.
