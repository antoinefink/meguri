# meguri-testkit

- The conformance runner implements `specs/conformance/`. `tests/conformance.rs` is T0 (one datatest-stable test per scenario file, `harness = false`); `meguri-cli conformance` is the same library for `tools/mg`.
- An area makes scenarios runnable by adding `src/exec/<area>.rs` and registering its setup sections, ops and observation kinds in `MeguriExecutor::new`. Unregistered means `pending`, never `pass`. Adapters open collections through `CoreHost::open` and take time only from `Session::clock` and randomness from `Session::random`.
- A new op or observation kind changes `scenario/model.rs` (`Step`, `ObservationKind`), the spec and schema, and the oracle driver in one change; f32 values go through `observe::f32_value`.
- Scenario YAML is parsed from granit-parser events with the core-schema rules in `scenario/yaml.rs`, not through serde-saphyr's typeless deserializer (it reads `yes`, `017`, `1_000`, aliases, merge keys and tags differently). Its hash must equal the oracle's `canon.py` byte for byte; `tests/scenario_format.rs` checks it against every committed golden.
- `wallclock.rs` is the only system-clock read, for report timing and budgets.
- Runner tests build temporary conformance trees from copies of committed scenarios and goldens; never write to `conformance/`.
