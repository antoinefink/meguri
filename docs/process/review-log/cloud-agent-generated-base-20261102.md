# Generated collection installation for the signed agent

Status: reviewed WIP; compilation and focused execution are pending.
Component source base: 60b8cc90. Integration start: 60e54c56.
U113 start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Implementer: s-20261101T235351Z-264d9cd0.
Fixer: s-20261025T162405Z-9dc656e4.
Contract: conformance/cloudkit-agent J2.3.

The direct small generated base is checked against its original fixture receipt,
generation record, header and complete file maps. Installation follows durable
initial launch/started, then retains an intent and completion outside live data.
Relaunch keeps the changed collection, media and tracking files. Interrupted
installation refuses reopening rather than copying over live data. Prepared
fixture variants still report implementation_missing. Product, account and
independent-counterpart admission remain separate and unchanged.

Prepared source export: cleanroom/20261102T084935Z-12169.
Independent reviewer: native_bounds_review. Round 1 found a P2: consumed
artifact paths admitted duplicate declarations. The repair requires exactly one
declaration per consumed path, including conflicting artifact kinds. Round 2
found no remaining required findings in the actual patch.

Generated tests cover exact original bytes, optional WAL, complete media, receipt
and file-map changes, links and replaced paths, interrupted installation, and
relaunch retention. Duplicate artifact cases cover all three consumed roles.
These tests do not prove SQLite content or signed SDK execution.

No native build or focused test has run for this patch. Disk space remains
below 40 GB after idle Meguri-cache cleanup; build work is held. The failed full
gate gate/20261102T075235Z-35258 predates this candidate. No Unit trailer,
passing gate or signed acceptance is claimed.

The staged privacy check initially flagged a redundant two-line test comment.
Integration removes that comment only. Test statements and scanner rules stay
unchanged; the original source export remains retained.

Native attempt swift-test/20261102T114910Z-2067 failed before executing tests:
the current Swift compiler rejects the deprecated C-string initializer spelling
as a warning. Filtered cleanup cleanroom/20261102T115252Z-12925 replaces
only that spelling with validatingCString. Independent review found no issue;
UTF-8 validation, nil rejection and all directory checks remain unchanged.
Compilation and execution after this cleanup are still pending.
