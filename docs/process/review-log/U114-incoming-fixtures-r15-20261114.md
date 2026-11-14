# U114 incoming fixtures, R15 — 2026-11-14

Starting commit: 37c0595c8b5dacdfc2cc445f5f893039db3c25f5.
Integration base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
This is a focused fixture repair, not U114 or batch acceptance.

The ten positive incoming tests used a baseline without accepted epoch
information. The approved policy keeps collection writes closed in that
state. An explicit positive helper now accepts a complete matching epoch
through the original SDK before the measured work. The default fixture keeps
its prior behavior. Setup asserts the real epoch result and verifies that it
adds no collection writes, cursor, inbox entries, apply runs or drain proof.

All existing limits and negative checks remain, including 502 entries in
operations of at most 500, replay, parent arrival, rollback, fatal failure,
cancellation and rejection of an old SDK instance. Production code is unchanged.

The filtered author run was 20261114T210138Z-29883. Independent review
20261114T210526Z-33573 found no P0, P1 or P2 issue. Focused core test
20261114T210557Z-34069 passed all ten selected tests. The exact reviewed patch
was saved and removed while the physical phone session needed an unchanged
checkout; the same patch was restored after that session ended.

Privacy run 20261114T215123Z-63204 passed. Similarity run
20261114T215128Z-63466 scanned the changed file with no findings. The
implementation report alone was read. Rust lint run 20261114T215133Z-63824
passed all three checks, including layering.

The earlier baseline failures remain in their original receipts. These ten
passing tests do not establish a passing full FFI suite. Other fixture groups,
required CloudKit lanes and the full batch gate remain open.
