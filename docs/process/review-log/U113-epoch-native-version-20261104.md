# Native epoch version proof

Status: WIP; no unit acceptance.
Unit start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Component start: f90fb368.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Fixer: s-20261103T133750Z-f276f58e.

Drive 20558 proves equal epoch data but unequal opaque archives. It does not
prove equal native version tags. Contract review permits a native projection
under K10.1, K18.4 and V14.2, with Core's exact comparison unchanged.

Export 23875 carries the epoch from the current operation, matched by work.op,
through the existing Fetch action. Native preparation validates that archive
and context. Projection checks the actual identity, type, current owner,
asset absence, exact data and equal nonempty change tags before returning
the original expected record. Other mismatches remain failures. Core runtime,
paging bounds, authority, abort and cursor behavior are unchanged.

Review finds two Kit fixture switches with the old arity and missing payload
cases. Export 25686 corrects them. The Service fixture reads the real payload,
checks its record/version/hash and returns an owned lease. The simpler fixture
explicitly refuses unsupported payload work; it claims no external-payload
coverage. Existing assertions remain.

Core 28578 finds private-field access in two new provenance assertions.
Export 30021 adds a Rust test-only, non-UniFFI accessor returning the actual
work operation ID. Both comparisons remain. Core 31060 passes twenty adoption
tests; Core-lint 33692 passes. Source privacy passes after a comment removal.

Native 33908 compiles but aborts because a new fixture assigns CKAsset into
encryptedValues. The public SDK header forbids that assignment. Independent
review approves export 38072: one legal NSString wrong-type value replaces
an impossible case, its duplicate is removed, and callback case numbering is
updated. Existing asset, identity, tag, terminal and count checks remain;
production guards and the other nine source files are identical. The failed
receipt stays failed; partial results do not count as a pass.

Native 39931 passes 85 selected tests in four suites. The positive archive
fixture uses public XML/binary secure coding with synthetic version tags.
Default production construction retains the real SDK tag getter. This unit
proof does not establish equal tags on the live server. The corrected fixture
also passes privacy. Independent review reports no remaining P0–P2 findings.

Kit 41843 compiles the app-layer fixtures, then fails the transient-page-error
case: initial adoption and recovery are no longer retained. An index trap
aborts the process after those failed predicates. The failed suite and its
assertions remain required; earlier individual results do not make it pass.
Contract diagnosis proceeds separately from the version-projection repair.
A signed phone run and real convergence remain pending. No integration or
unit acceptance is claimed, and the batch gate remains open.
