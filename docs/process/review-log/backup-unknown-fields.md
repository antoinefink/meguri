# Backup metadata validation

Start: `a83b872401a304c395d120c68e6861c9e4a3f968`.
Fixer session: `s-20261024T174115Z-511e3ad6`.
Contract: `package/backups`, R6.

Gate `core-test/20261024T183852Z-89868` failed the unchanged host lifecycle test
for forced empty backups and unknown metadata. Backup publication was durable,
but recording its receipt rejected an additional member in the active identity.

Filtered export `cleanroom/20261024T192637Z-63483` validates only the required
identity, binding and decision fields through the existing strict decoder. It
retains the full original metadata, including unknown members. Global identity
decoding, intent checks and publication handling are unchanged.

Independent review found no P0–P2 issues. The original preservation assertions
remain intact. Seven new malformed-field cases require the sidecar bytes to
remain unchanged and the already published backup to remain inspectable.
Focused checks passed:

- `core-format/20261024T192941Z-64696`: formatting.
- `core-test/20261024T193117Z-65579`: all 20 host lifecycle tests, no skips.
- `core-test/20261024T193201Z-66548`: both selected package backup tests.
- `core-lint/20261024T193243Z-67209`: workspace lint, unchanged limits.
- `private/20261024T193119Z-65830`: no private-token finding.
- `similarity/20261024T193121Z-66057`: no flags or allowlist.

This repair shares the pending integration gate with the error-layout repair
and Mac signing repair, against base
`81c207573c20c337c8834e4ae8b4ee8f29603dbf`.
