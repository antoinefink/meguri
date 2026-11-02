# Signed-agent account admission

Status: component verified; signed execution remains open.
Start: `b20a3fa599cc2e23fbed83142d04cb99b148d09b`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Implementer: `s-20261101T235351Z-264d9cd0`.
Contract: `conformance/cloudkit-agent#J2.2`, J5.

The runtime retains exact header bytes and the approved private input.
Both native account-reading paths use the same guarded factory. The guard
checks retained input before and after fresh lookup, then checks exact identity
bytes before returning an available result. No current read chooses its own
expected account. The app-hosted delivery compares exact header bytes too.

Independent reviewer: `native_bounds_review`, no required findings.
The first candidate refused native operation starts. Under J2.2 in
`786e3d09`, the final candidate uses frozen expected-account data.
Core still enforces identity, binding and SDK guards. Observed input failure
stays terminal, even after the file is restored. Original callbacks drain and
join; a successor cannot perform another operation.

Focused `swift-test/20261102T041944Z-99848` passed all 48 tests, no skips.
The four Swift files passed `similarity/20261102T042049Z-2077`; privacy
`private/20261102T041945Z-382` passed. The actual CloudAgent Mac target compiled
in `build-app/20261102T042320Z-2551`; its iOS simulator target compiled in
`build-app/20261102T042643Z-3862`. Both had zero errors and warnings.

Product verification, generated-base verification, integrity inspection and
durable operation recovery remain required. Generated protocol tests and
compilation do not prove SDK execution, Core account acceptance or convergence.
No Unit trailer, full gate, VM execution or signed acceptance is claimed.
