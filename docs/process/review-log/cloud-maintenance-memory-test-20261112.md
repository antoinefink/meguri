# SQLite memory test reader — 2026-11-12

Role: fixer. Session: `s-20261110T214025Z-20fa15fd`.
Start: `b3cb9363a51cb8d91692dced2ecce842e9fd3311`.
Integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Full Rust run `20261112T015202Z-84026` fails the existing memory worker.
Temporary diagnostic `cleanroom/20261112T015849Z-84743`, run as
`core-test/20261112T023122Z-93794`, disproves the initial VACUUM diagnosis.
VACUUM and its setting restoration succeed, with 1,379,480 SQLite bytes
allocated afterward. The failure is the following reader's `quick_check`
unwrap. Its normal 40 MiB cache budget exceeds this worker's global 16 MiB
heap cap. The diagnostic is exactly reverted and is not committed.

Filtered repair `cleanroom/20261112T023304Z-94158` gives only that test
reader the same 1 MiB cache budget as its writer. The original fixture,
file-size requirement, global cap, temporary-store checks, freelist check,
complete integrity check and subprocess isolation remain. New assertions
check the reader budget and the unchanged hard cap before and after the
integrity check. The reader lease ends before close. Production defaults
and APIs are unchanged.

Independent review `cleanroom/20261112T023525Z-94714` finds no P0–P2 issue
or weakened proof. Its MG source commit matches the start above; its exported
seed is identical. Review is static; integrator evidence is separate:

- `core-test/20261112T023547Z-95258`: all 27 integrity tests pass, including
  the original parent memory test.
- `core-lint/20261112T023638Z-95596`: four checks pass.
- `similarity/20261112T023639Z-95871`: the one source file passes, without
  an exception.

The original failures and cancelled gate `20261112T014846Z-84391` remain.
A complete integration gate at the original base is required after the
coherent recovery batch. This change accepts no unit or milestone.
