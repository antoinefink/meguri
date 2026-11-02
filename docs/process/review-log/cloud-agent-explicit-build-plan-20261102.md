# Explicit CloudAgent builds

Status: focused checks pass; signed execution remains open.
Start: `c5389d77`.
Fixer: `s-20261025T162405Z-9dc656e4`.
Contract: `conformance/cloudkit-agent#J2.1`.

The public app-build command accepts an explicit CloudAgent plan for Debug
Mac and iOS simulator builds. It forwards that plan to build-for-testing and
records it in build details. Defaults stay UI; preview and incompatible
configuration combinations fail before build work.

Independent reviewer: `native_bounds_review`.
Round 1 found one P2: failed CloudAgent builds printed a UI-plan reproducer.
The repair retains the exact plan in that command. Round 2 has no findings.
`selftest/20261102T040414Z-94552` passed seven focused routing checks.
Privacy `private/20261102T040416Z-94782` passed for the combined source.

`build-app/20261102T035630Z-91874` and the final account-runtime build
`build-app/20261102T042320Z-2551` compiled the actual CloudAgent target with
zero errors and warnings. The iOS simulator build
`build-app/20261102T042643Z-3862` also passed with zero errors and warnings.
The earlier UI-plan result is not CloudAgent proof.
These builds launch nothing and establish no signing, delivery or SDK outcome.
No Unit, full gate or signed acceptance is claimed.
