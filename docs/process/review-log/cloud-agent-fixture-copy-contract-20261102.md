# Fixture copies for the device agent

Status: contract reviewed; client installation and signed execution remain open.
Component start: `055922fed8a6600b0c1880d288e5ddd1fa5e486e`.
Spec writer: `s-20261102T034645Z-9f83814c`.
Contract: `conformance/cloudkit-agent#J2.3`.

Independent reviewer: `native_bounds_review`, no P0–P2 findings.
The contract assigns fixed local roles to original receipt bytes, generation
evidence and parent/prepared copies. Host artifact paths remain metadata.
For direct small input, one parent copy supplies equal parent/install maps.
Generation comparison uses the request/manifest/observed projection; MG's
target/kind fields select the entry but are not part of that projection.

The header and frozen-input field sets and version remain unchanged. Installation
still requires a durable start, an absent run-owned destination and verified
bytes before Core opens. Relaunch preserves changed live data. An incomplete
installation requires actual recovery; this change grants no repeated copy,
new user step or signed acceptance. No code or guard changed.
