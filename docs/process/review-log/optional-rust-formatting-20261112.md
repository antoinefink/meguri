# Optional Rust formatting

Start: `6e5d92dd498e1f6b3b8ad57db2fdfb2f872af972`. The pending integration batch still starts at `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Antoine authorized removing source-cosmetic lint and requested a new Herdr subagent. That agent exported the filtered author patch from `cleanroom/20261112T155625Z-4502` and independently reviewed it without P0–P2 findings. The integrator also checked the applied patch. Clippy, layering, build and compiled-name scan implementation remains byte-identical to the starting version.

`core-lint` now runs three checks. Rust formatting is optional through `core-format`; it is neither run nor counted as a successful mandatory check. The tests retain illegal-clock, randomness, dependency and compiled-name failures and add acceptance for valid unformatted code. Native-interface, accessibility, privacy, provenance, similarity and dependency checks remain enforced. No app or core source was restyled.

Verification:

- `static-checks/20261112T160012Z-9966`: pass, three checks.
- `selftest/20261112T160007Z-8351`: interrupted with exit 137 during the Docker restart. Four completed TAP cases passed, including the new formatting acceptance case, dependency-edge rejection and illegal-clock rejection. The overall run remains an error.
- `selftest/20261112T160544Z-19584`: pass, the two remaining randomness and compiled-name cases. This run limits Cargo compilation to one process through an ignored Bats setup file; assertions and runtime limits are unchanged.
- The first sandbox invocation, `selftest/20261112T155937Z-7370`, could not reach Docker and ran no tests. Verification uses host access to the same MG job.

Code review found no P0–P2 findings. The completed checks are listed above; the interrupted job remains an error.

No unit, integration gate, milestone or release acceptance is claimed by this change.
