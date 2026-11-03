# U151/U161 publication confirmation component

Status: focused component verified; units and integration remain open.
Component start: ebf727ce. Integration base:1416a520542a8c5daa38a23bfe398da704c3d6c9.
Implementer: s-20261103T081200Z-600c3638.
Fixer: s-20261103T084239Z-e23dce75.

The existing Core confirmation still owns acceptance. Presentation can close
after acceptance while the same operation continues. Refusal has its own
presentation result. Current background errors remain visible without using
historical failure as the current result. Closing never cancels accepted work.
Resume, adoption, SDK ownership and the exhausted U114 repair remain unchanged.

| Review | Result |
| --- | --- |
| R1 | P2: fresh background failures were hidden; repaired in41987 |
| R2 | Clear of P0–P2 |
| R3 | Dead outer catch removal78786: clear of P0–P2 |
| R4 | Test initialization and typed result84590: clear of P0–P2 |
| R5 | Optional macro cast90716: clear of P0–P2 |

Focused Swift92424 passes13 selected tests in2 suites, including the real-Core
publication tests with6 parameter cases. They retain a held original transport,
close presentation, reject repeat submission, cover refusal and reprepare,
separate old and fresh errors, and reject stale presentation completion.
All assertions and the10-second wait limit remain. Initial compile failures
74177,81274 and87777 are retained; they are not test passes.

Privacy77593 passed for the initial three-file UI component;95681 passes the
final5 source/evidence files. Similarity97890 and static97872 pass. Team-signed
standalone build96219 passes on clean82aeb7a6. The exact fresh process path and
executable digest were verified after normal quit/relaunch. Local Study, answer
reveal and Back work; no additional grade was submitted. The actual sync badge
still reports unavailable, so accepted-transfer visual states remain unproved.

No full gate or unit closure is claimed. Keep Local, complete local preview
facts in the interface and real signed publication/adoption remain unfinished.
The startup caller trace later identifies ConfigureAutomatic reaching native
preparation before verification; the proposed lifecycle race is not established.
A separate U113 Rust-policy correction is in progress, without a native repair.
