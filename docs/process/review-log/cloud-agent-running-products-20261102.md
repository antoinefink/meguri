# Running agent product observations

Status: focused model checks pass; app-hosted execution remains open.
Start: `f5600b74`.
Integration onto: `49b2b97e`, preserving the design and VM work.
U113 start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Implementer: `s-20261101T235351Z-264d9cd0`.
Fixer: `s-20261025T162405Z-9dc656e4`.
Contract: `conformance/cloudkit-agent#J2.1`, J2.2.

The client observer binds the executing host, a class anchor compiled into the
agent image, and the host's Debug implementation to the frozen inventory.
It compares image identity and bounded physical-file bytes through retained
no-follow authority. Receipt copies are revalidated too. Mac kernel build and
simulator runtime input remain distinct observations. The runtime retains the
binding before account admission.

Independent reviewer: `native_bounds_review`.
Round 1 found a P1: the observer required separate-process widget libraries to
be loaded by the host. The repair requires only the host's own Debug library
and still validates each image actually loaded from the host. Positive tests
include embedded and standalone widget libraries without declaring them loaded.
Native compilation then found two deprecated array conversions. Bounded,
strict UTF-8 conversion replaced them. Existing public imports expose the
required declarations, so a redundant import was removed without changing the
privacy guard or adding an exception. Final review has no findings.

Source exports: `cleanroom/20261102T054124Z-29018`,
`cleanroom/20261102T055752Z-36209`, and
`cleanroom/20261102T060728Z-41541`.
`swift-test/20261102T061308Z-43885` passed all 78 selected tests, zero skipped:
64 existing agent tests and 14 new product-observation tests.
`private/20261102T061309Z-44244` passed all three files.
`similarity/20261102T061606Z-46812` scanned all three Swift files and passed.
`static-checks/20261102T061710Z-47769` passed all three checks.
The failed compilation `swift-test/20261102T060225Z-39612` and initial privacy
result `private/20261102T060226Z-40085` remain retained.

The tests use generated images and observations. Actual observer execution in
the app-hosted target, and Mac/simulator project compilation, remain pending
the shared batch checks. The independent signature, complete delivery,
destination, installation and whole-lane counterpart is still missing. Runtime
admission therefore still fails explicitly before account lookup. Generated-base,
integrity and recovery guards remain unchanged. No VM run, Unit trailer, full
gate or signed acceptance is claimed.
