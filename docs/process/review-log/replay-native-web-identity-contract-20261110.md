# Native replay web identity clarification

Spec-writer: `s-20261109T165344Z-77e618c8`. Anki: none.
Spec: `render/display-transform` D4, D12; supports R1964/R1965.

The technical mapping distinguishes the platform's two public AX identity attributes.
R1964 requires stable structural identity; it does not prescribe AXIdentifier.
D4/D12 now require the actual Mac WebKit link's exact AXDOMIdentifier, trusted
generated ownership, current B1/B3 binding and that same link's native press.
Both raw attributes remain visible; no field is normalized into the other.
The export/driver/checker correction is limited to that technical mapping.
No labels, geometry or ordinals become identity, and no audit is waived.

Keep all twenty original activations, names, authored-ID collision preservation,
keyboard ownership, ruby/geometry/hit-target checks and mobile obligations.
No layout, graphics, card styling, native action or bridge permission changes.
This records no cosmetic exception, owner approval or implementation acceptance.
CloudKit R2066 approval supplies no authority for this native UI clarification.

Retain 7686's zero-link package failure and 93192's four-link external failure
with empty native identifiers. They are different observations. Neither proves
that the native web attribute is available or that any of the twenty native
activations succeeded. Implementation and actual destination verification remain.

Public API provenance, independent of Anki:

- Installed MacOSX27/27.2 public SDK `AXWebConstants.h:100` declares
  `kAXDOMIdentifierAttribute` with a CFString value. `AXAttributeConstants.h:1297`
  separately declares `kAXIdentifierAttribute`. These remain distinct facts.
- Apple's [attribute-read API](https://developer.apple.com/documentation/applicationservices/1462085-axuielementcopyattributevalue)
  reads an accessibility object's attribute, with explicit unsupported/error
  outcomes. Its [native-action API](https://developer.apple.com/documentation/applicationservices/1462091-axuielementperformaction)
  invokes the requested action on that actual object; the public SDK declares
  `kAXPressAction`.
- The official [WebKit change record 253917@main](https://results.webkit.org/commit?id=253917%40main&repository_id=webkit)
  describes exposure of a DOM ID through AXDOMIdentifier. This identifies the
  attribute's purpose, not installed-destination behavior or a passing witness.

Read-only supporting reviews: `build/cloud-replay-native-public-api-witness-20261110.md`
and `build/cloud-replay-native-identity-contract-review-20261110.md`.
Only spec/vocabulary and this log changed. No product/tool implementation,
executable schema, builds, tests, UI, network or git commands were used.
