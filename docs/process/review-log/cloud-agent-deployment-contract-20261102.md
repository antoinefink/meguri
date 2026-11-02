# Signed-agent build and delivery contract

Status: reviewed; producer and runtime work remain open.
Start: `c5389d77`.
Spec writer: `s-20261102T034645Z-9f83814c`.
Contract: `conformance/cloudkit-agent#J2.1` and J2.2.

The contract defines CFBundleVersion, complete CloudAgent build inputs,
the loaded agent bundle, intended VM and simulator routing, actual copied
products and installation evidence. Header and receipt references have no
circular hash dependency. Evidence copies retain the original receipt bytes.

The approved lane stays the Mac app and reserved iPhone/iPad simulators in the
test VM. The coordinator creates per-run routing and reuses the approved
test account scope. User setup stays with Q7.2.

Keep the approved scope fixed. If retained input checks fail, stop new agent
admission and join owned work. This does not require interception of every
internal SDK network start. Core identity and SDK guards remain required.

Independent reviewer: `native_bounds_review`, no P0–P2 findings.
Validation: staged privacy scan and diff whitespace check, recorded with the commit.
No build, deployment, signed test, Unit or milestone acceptance follows.
