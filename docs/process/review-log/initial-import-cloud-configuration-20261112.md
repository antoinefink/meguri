# Initial import cloud configuration

Start: `10bb0729`. Integration base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Initial Files import retained a host without its configured cloud inputs. A normal reopen supplied those inputs. Inspection now copies the same platform inputs and key provider as the ordinary opener. Absence behavior, host ownership and the exported interface are unchanged.

Diagnosis: `cleanroom/20261112T174152Z-8437`. Regression author: `cleanroom/20261112T174559Z-12516`. Setup correction: `cleanroom/20261112T174945Z-15999`. Production fixer: `cleanroom/20261112T175144Z-18903`. Independent review: `cleanroom/20261112T175404Z-21842`, no P0–P2 findings.

The test imports through actual FFI jobs and reads their typed results. On the retained initial host, before any reopen, it requires workflow, connection, policy and media services. The provider remains lazy. Its weak reference proves retention in the composition; source review covers host-specific ownership.

Evidence:

- `core-test/20261112T174853Z-15133`: earlier test setup failed because the target directory was absent. Native setup creates that directory; the corrected test requires an absent database within it. The source export also waits for derived binding.
- `core-test/20261112T175104Z-17865`: intended red, missing cloud runtime after successful import.
- `core-test/20261112T175246Z-20331`: green, both initial import and existing ordinary-open cloud tests pass.
- `private/20261112T175333Z-20853` and `static-checks/20261112T175333Z-20834`: pass.
- `similarity/20261112T175845Z-26000`: pass, two files, no exceptions.

Native verification without a restart and the integration gate remain pending. The separate native adoption failure and status presentation issues remain open. No unit or acceptance gate is claimed.
