# Signed CloudKit agent inputs

Status: contract reviewed; implementation and signed execution remain pending.
Component start: `cfd1f33d8c99cda713fbc18172ca0a9531559369`.
Spec writer: `s-20261102T011456Z-f4a4e630`.
Contract: `conformance/cloudkit-agent#J2` and J6/J7.

The contract now defines the executable byte boundary, exact build and
deployment receipts, protected expected-account input and frozen generated
fixture delivery. Signature, grants, provisioning and actual device delivery
remain separate checks. Missing verification code fails explicitly.

Independent reviewer: `native_bounds_review`; no P0–P2 findings.
Review covered receipt bindings, private input authority, initial installation,
relaunch preservation and the absence of circular input hashes. The fixture
input proves no study operation or whole signed test leg.

This spec change adds no unit acceptance, user approval or passing runtime
claim. Platform execution and the full signed-agent implementation remain open.
