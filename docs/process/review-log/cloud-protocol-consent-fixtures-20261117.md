# Explicit consent in cloud protocol fixtures

Starting commit: `874b9e3ad52c1972ef01d5eeb1fdac1a40d99ba0`.
The app batch retains base `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Account inputs alone grant no provider consent. Older opted-in protocol tests
now use named private helpers that import prior settings through the production
absent-only migration. Generic Host builders, shared defaults, no-consent cases
and all four restoration tests remain unchanged. Reopening preserves the
existing provider document, including handoff and service selection.

Filtered author `cleanroom/20261117T091731Z-53761` changes ten test files.
Independent review `092454Z-66563` found one P2: a local Restore donor needed
no cloud consent. Integration omitted that single substitution hunk.
Independent non-author `/root/adoption_empty_decks` verified the omission and
the unchanged donor body. The 14 protected parent functions were also checked
byte for byte. Corrected patch SHA-256:
`6abfdf04ac2f84022ca05285d68e3c80c51c19b4141b28aa824da7a720e09b4f`.

Ordinary privacy check `private/20261117T093357Z-5302` passes all ten files.
`core-test/20261117T093808Z-11326`, clean `976738d2`, runs all 136 methods:
125 pass, 11 fail, zero skip. Before this arrangement repair, `091221Z-42310`
had 15 passes and 121 failures. No assertion, test name, deadline, fault
point or production behavior changed.

Ten remaining failures reach provider admission later in publication. The
media recovery check found an unexpected sequence. Both groups need
diagnosis and verification. No full-binary, unit, gate or release acceptance
is claimed; prior failed receipts remain.

Independent complete-file public review by `/root/incoming_contract_review`
admits only the current verification register at SHA-256
`764f02e2bd3a9bbed0be59111919600d1babe04ab01fc53ce44d9bc0d6c16069`.
The routine admission changes no matcher, token list or privacy access.

## Replacement consent and media recovery

Starting commit: `976738d2bc4e5776866b91622f6494365921b277`.
Filtered test author `cleanroom/20261117T100042Z-10155` exports two files.
Independent `/root/provider_test_delta_review` clears the full test patch at
SHA-256 `2e35ef5456a085f8b6e60ea02a86c1429c137be9ae3266cb9864e545d7fe2d6d`.
Integration omits five comment lines after ordinary privacy flags; the same
reviewer verifies that all code and assertions are unchanged. Ordinary privacy
then passes. Test inputs are committed at `de00fbd1f`.

Actual `core-test/20261117T101316Z-69350` fails the new replacement case at
its pending-consent assertion: the accepted setup was qualified against the
predecessor before replacement Bind. This is the expected old-production
failure. Its exact child-name selector omitted the media case; no execution
credit is assigned to that omitted case.

The corrected media-only selector in `core-test/20261117T101353Z-70039`
passes one method, zero failures or skips, on clean `de00fbd1f`. The test settles
queued rollover work before measuring settlement, preserves exact sequence
equality, compares all captured rows and requires matching WAL identity and
coverage. The original unexpected Seq12 owner remains unattributed; this
fixture correction grants no exception for future sequence changes.

Production replacement qualification repair and independent review remain
pending. The affected whole binary and integration gate are still failed.
