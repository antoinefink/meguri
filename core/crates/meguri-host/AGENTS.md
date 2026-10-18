# Composition root

- Construct and wire providers here; business operations stay in their owning crates. Callers supply HostConfig inputs, never Ports.
- DerivedHost's open step comes first. Media recovery must finish before Ready. Close the engine and shared media connection before joining derived workers; unwind every started provider on open failure.
- `wiring/pending.rs` names unavailable providers and their owner units. Never replace a safety backup with a successful no-op. Add every engine ServicePort exactly once, then remove its pending entry when its provider lands.
- Raw storage conformance stays raw until an engine-owned operation promotes it. Promotion closes the old writer before CoreHost opens; see `specs/engine/host.md` H4.
