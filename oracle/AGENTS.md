# oracle/ (AGPL-3.0-or-later quarantine)

Only spec writers and oracle maintainers work here. Implementers never read this tree (R1956); they run it through `tools/mg oracle ...` as a black box. Nothing outside `oracle/` imports, copies or ships its code (R1869, R1873).

- Add an area's ops and observations as a module under `driver/ops/` or `driver/observe/` with the `registry` decorators; `worker.py`, `session.py` and `harness.py` stay area-free. Write each vocabulary entry's meaning in `conformance/` (protocol.json now, the U4 specs later), never only here.
- Never put Anki's message text in a response or observation (it is localized, R1887). Errors carry a kind, the step and the exception class name; tracebacks go to stderr.
- Protobuf values go through `convert.message` and other f32 values through `numbers.f32_value`, or they print as widened f64 (11.262236595153809 instead of 11.262237).
- Time moves only through `Clock.set`/`advance` (epoch seconds in the libfaketime file, `FAKETIME_FMT=%s`, never the `@` form). Pass the frozen instant explicitly where Anki takes a timestamp argument (`answered_at_millis`). `time.monotonic`/`perf_counter` are real.
- `ANKI_TEST_MODE` turns Anki's fuzz off and shifts its clock: the entrypoint unsets it and the worker refuses to start with it.
- Workers need `/scratch` on tmpfs (the compose service has it). On the container overlay a scenario is ~10x slower.
- `canon.py` defines scenario_sha256 (RFC 8785 of the YAML 1.2 core-schema parse). The Rust runner (U4) must produce the same bytes; change both together.
- Any driver edit changes `driver_sha256` and the image tag. Goldens go stale only on scenario or lock changes; re-record with `tools/mg oracle record` when an edit changes observations, and check with `tools/mg oracle compare`.
- Pins move only through `tools/mg oracle bump <version>` (lock via `tools/mg oracle lock`). Adopting a bump is one separate change: staged `requirements.in`, `requirements.lock` and goldens together.
- Add-on oracles (another quarantined image): `FROM meguri-oracle:<tag>`, add modules, list them in `ORACLE_EXTRA_MODULES`.
- `tools/mg oracle test` runs `driver/tests/` in the container on the internal network (the isolation test fails anywhere else) plus `tools/tests/oracle_host_checks.sh`.
- `provenance/` holds per-section notes (Anki file + tag 26.09.3) for anything specified from Anki's source. `harvest/` is U4's.
