# Composition root

- Construct and wire providers here; business operations stay in their owning crates. Callers supply HostConfig inputs, never Ports.
- DerivedHost's open step comes first. Its file binding stays asynchronous; tests inspecting its path must use its wait_idle barrier. Media recovery must finish before Ready. Close the engine and shared media connection before joining derived workers; unwind every started provider on open failure.
- `wiring/pending.rs` names unavailable providers and their owner units. Never replace a safety backup with a successful no-op. Add every engine ServicePort exactly once, then remove its pending entry when its provider lands.
- Raw storage conformance stays raw until an engine-owned operation promotes it. Promotion closes the old writer before CoreHost opens; see `specs/engine/host.md` H4.

- HostConfig backup overrides exist only with `test-hooks`. A counted harness witness proves call order, never a production backup. Production forced safety backups use the package provider; RF2 retains every published file. U126/U127 scheduling, T7 and retention remain unaccepted. U37 must connect the FFI runtime to the host-owned engine and preserve its process-wide dispatcher/job identity; the Rust parity harness does not prove exported CoreBridge integration.
