# ZIP security update — review draft, 2026-11-01

Start: `9d88dddf78698eed3b463705ebe705fe1472d7e4`. Manifest author: filtered fixer, session `s-20261025T162405Z-9dc656e4`; export `cleanroom/20261101T192008Z-39556`. Independent source review confirmed exactly two requirement changes and unchanged feature flags. Root owns integration, commit attribution and acceptance.

[GHSA-m665-v7pm-7grj / CVE-2026-115905](https://github.com/zip-rs/zip2/security/advisories/GHSA-m665-v7pm-7grj) affects ZIP versions through 9.0.4; 9.0.5 is the first fixed release. The dependency reaches the shipped package importer/inspection runtime and the separate fuzz workspace. The inspected sources use ZipArchive; no advisory-style mixed streaming-extraction chain was established. This update does not claim Meguri was exploitable through that chain.

Both `core/Cargo.toml` and `core/fuzz/Cargo.toml` change only `~9.0.2` to `~9.0.5`, preserving `default-features = false` and `features = ["deflate"]`. `tools/mg core-update zip 9.0.5` refreshed the main lockfile; the MG fuzz build resolved its own lockfile. Both now select ZIP 9.0.5. No application code, server implementation or test assertion changed for this security update.

Fuzz-lock synchronization also adds cmov 0.5.4, ctutils 0.4.7, data-encoding 2.11.1 and hmac 0.13.0; digest gains its ctutils edge. Existing package/sync dependency edges now match their manifests. Pulldown-cmark moves from 0.13.4 to 0.13.5 to satisfy the workspace's existing exact 0.13.5 pin. Exact generated diff: `build/zip-security-fuzz-lock-20261101.patch`. No lockfile was constructed manually.

Verification:

- `core-update/20261101T192608Z-49464`: PASS, exact ZIP 9.0.5 update.
- `core-test/20261101T193641Z-63923`: PASS 545/545, no skips; includes the 106 package tests, shared CloudKit tests and media ZIP memory check. This is the shared run, not 545 ZIP-specific tests.
- `core-deny/20261101T194854Z-75930`: PASS, advisories clean for the main workspace.
- `core-fuzz/20261101T194900Z-76205`: PASS, package_reader only, 92,658 executions with a 60-second budget (61 seconds reported by libFuzzer), no findings, peak RSS 351 MB. Nightly sanitizer compilation preceded the timed run. Log: `build/zip-security-package-fuzz-20261101.log`.

These receipts record dirty-source inputs. The dependency update has not received its full batch gate or native product verification. No Unit trailer, milestone, signed CloudKit, device, trial or release acceptance is granted. Preserve frozen server scope and all existing safety assertions.

Independent lock review found no P0-P2 issues. Fuzz adds ctutils 0.4.7 while the main lock retains 0.4.6; this is a new compatible fuzz edge, not a shipped dependency change. Both ZIP entries have the same 9.0.5 checksum.
