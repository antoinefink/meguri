# U114 Profile ACK observer

Starting commit: `f046120fd7bb379eb0c948bc45c9f9d1c002b5d2`. Pending full-gate base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The Profile package check `swift-test/20261115T095515Z-5187` failed before execution because Profile-active tests referenced the Debug-only acknowledgement observer. The correction makes the internal CausalBoundary enum and nil-default, synchronous MainActor observer available in each package configuration. Only the actual commandAcknowledgementJoined emission also becomes available. Its position remains after the original checked continuation resumes and before afterOutputs, once per actual iteration. Other emissions, fault seams, metadata and observeQuiescentDependency stay Debug-only. No assertion, test limit, consumer, compilation flag or sync decision changes.

Contract basis: KTD18/R1967 and U7 permit harmless package injection seams. KTD21 keeps app compilation conditions out of packages. Optional invocation keeps arguments lazy with a nil observer. Storage and a nil check remain; zero performance cost is not claimed.

Source audit: `cleanroom/20261115T101630Z-25703`. Filtered author: `cleanroom/20261115T102732Z-31343`. Independent review: `cleanroom/20261115T103250Z-35072`, CLEAR, no P0–P2 findings in the complete one-file diff and callers. Root read the entire patch. No reference source was read.

Evidence:

- Profile: `swift-test/20261115T103832Z-38011`, two unchanged ACK methods passed, zero failed or skipped. Held-command and quit cases ran; all test targets compiled. Debug-only suites were not activated.
- Debug: `swift-test/20261115T104648Z-43611`, three methods passed, zero failed or skipped. The same ACK controls and the original publication-status test ran with their original assertions and limits.
- Privacy: `private/20261115T104648Z-43632`, one source file passed.
- Similarity: `similarity/20261115T104648Z-43547`, one file, zero flags or allowlist entries. Root read only the implementer report.

The failed Profile receipt is retained. This fixes its missing-observer compile error; it does not extend Profile coverage to Debug-only suites or resolve the earlier unexplained media error. Required integration gates, native activation/replay, broader recovery and peer review evidence remain open. No Unit, P24, milestone or release acceptance is claimed.
