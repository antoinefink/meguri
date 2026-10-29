# U114 native owner validation

Status: focused repair verified; broader CloudKit acceptance remains open.
Phase start: `33dd1745abbcbb448b6d457aa5908bbfea37b670`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

This closes retained P2-OWNER from the SDK review. Native IDs are checked before
domain values drop the owner. Mixed fetched input is rejected before delivery;
sent and pending record IDs must match the configured collection zone. Fetch
scopes, zone callbacks and pending database changes validate owners before
projection or mutation. Rejection retains terminal failure.

Filtered fixer: `cleanroom/20261029T053841Z-12430`.
Independent narrow repair review: no P0-P2 findings. Existing grouping and the
single SDK event entry/return boundary remain unchanged. Ten constructed-ID
tests supplement the four existing native routing tests.

The initial privacy scan flagged comments and custom local names. Repair
`cleanroom/20261029T054629Z-18332` removes the comment and renames only the
custom argument. Independent exact-delta review confirms equivalence. SDK
spellings, checks, comparisons and test limits remain unchanged.

Verification:

- `swift-test/20261029T054516Z-16447`: initial 14 native tests pass.
- `swift-test/20261029T054812Z-19755`: final 47 native and adjacent adapter
  cases pass, including regenerated-binding compilation and required UI lint.
- `private/20261029T054815Z-20337`: final two files pass.
- `similarity/20261029T054817Z-20853`: two files, zero findings or exemptions.

No Unit trailer, signed-device proof or full sync claim. P2-REG remains open;
publication, adoption, incoming data, media and integration requirements remain.
