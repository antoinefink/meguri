# meguri-sync

- `endpoint_guard` (R1876) is the only copy of the sync endpoint rule: the `test-hooks` transport applies it and meguri-testkit re-exports it. It compiles only with `test-hooks`, because a build without `ankiweb` must contain no AnkiWeb name (byte scan, U105/U116); never move it out of that gate. Its test vectors are `tests/endpoint_guard.rs` (`required-features`).
