# Mac settings search gate

Existing failure: `mac-hosted-test/20261031T205446Z-50756`, zero matched buttons.
Diagnostic start: `1c122e4cac42e3375cd7d5ee1c1f34f0596c1939`.
This is an open technical check of the current U113/U114 integration batch.

`cleanroom/20261101T004611Z-60159` adds bounded public metadata to the test.
Its controls and checks stay fixed. Independent review finds no issues;
`private/20261101T004944Z-61627` passes for the test source.

`mac-hosted-test/20261101T005000Z-61955` compiles and runs the selected method
in the dedicated VM. The window is hidden; the attached field is not hidden
but its visible rectangle is empty. There are zero matched buttons. The field
has one accessibility child and that child has two children. The diagnostic
then fails with a Swift NSArray element-type bridge error.

These facts do not prove a product identifier defect. The next test-only
experiment will show this exact synthetic window before synchronous layout
and use the public getter selectors to retain an untyped NSArray, traversed by
count/object(at:). Preserve matcher rules, both mounts, all assertions and
bounded diagnostics. Review approves that experiment. No waits or retries,
private APIs or product-control changes are needed.

The gate remains failed until its selected-method receipt passes.

`cleanroom/20261101T005608Z-65474` uses the public selectors and an NSArray
retained through enumeration. It orders the exact synthetic window front
before layout. Independent review finds no issues; all original checks remain.
`private/20261101T010029Z-66493` passes for this repair.

`mac-hosted-test/20261101T010038Z-66757` completes the diagnostic. Its window
is visible, its field has an empty visible rectangle, and the match assertion
still fails. Native leaf objects fail the full protocol cast. The product's
identifier behavior remains unproven.

Next: getter-availability booleans through NSObject/responds/perform, followed
by the original role, label, identifier and identity checks. Keep both mounts
and the search-text assertions. Use modern public getter selectors only.
The re-key implementation takes priority while this test stays failed.

## Public-getter test diagnostic packet

Fixer session: `s-20261025T162405Z-9dc656e4`.
Packet base: `54af9f9a5cfdb815f61645961e63bc5a111d4551`.
Original U113 start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The test now traverses actual NSObject accessibility children through
`responds(to:)` and `perform` of modern public role, label, identifier and
children getters. Navigation children remain diagnostic only. Child NSArrays
stay retained during enumeration; identity guards and the existing diagnostic
record/depth budgets remain. Leaf diagnostics report getter availability and
allowlisted public classes/roles, flags and counts only, with no raw labels,
identifiers or private runtime class names.

Functional matching still requires the exact native button role and exact
localized label supplied by the native search cell. Both mounts retain the
single-match, exact identifier, duplicate-ID, object-identity, field-identifier
and search-text assertions, window visibility and synchronous layout.
Production `SettingsSearchIdentifiers` is unchanged; its full protocol cast
may still prevent identifier assignment. An actual selected-method run must
distinguish a test bridge problem from a missing product identifier.

Result: pending. One bounded source consistency check only; no build or test
run in this filtered fixer packet. Root owns MG build/test, independent review,
privacy, provenance and similarity verification. All original failed receipts
above remain retained; this packet grants no acceptance.

Public getter result:
- native_bounds_review: no P0-P2 source findings.
- private/20261101T101730Z-72615 and similarity/20261101T101729Z-72379: PASS.
- mac-hosted-test/20261101T101731Z-72877: FAIL, zero matches. The window is
  visible and field attached, but its visible rectangle remains empty.
- The field reports AXUnknown and its cell reports AXTextField. The native
  leaves expose none of the tested modern getters. No label or identifier
  values were printed. Product behavior is still unproven by this test.
- Window/layout and native-leaf representation need a bounded diagnosis.
  No product change, suppression or acceptance is claimed.
