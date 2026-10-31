# Native single-record asset transport

Source starts at `2df9441127eb989a81b3b6085abd3cd283cebd80`.
The original batch base remains
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The transport prepares one exact collection record read with all fields. It
checks the returned native identity and copies the payload file inside the
per-record callback. Its result owns that copy through the original terminal
callback and the later awaited core handoff. Controls keep their separate
asset-free validation. Rust work and submit guards retain admission authority.

Implementation: `cleanroom/20261031T222108Z-89416`.
Independent review found that an unavailable SDK source lost valid metadata.
Filtered repair `cleanroom/20261031T222902Z-94835` preserves the record without
a payload lease for source failures and staging limits. Cancellation, destination
errors and disk-space failures remain errors. Round 2 independent review is clear.

Compiler checks exposed two test-only issues. Original SDK callbacks now sit in
checked mutex storage, and the required raw value is extracted before its
assertion. The latter repair is `cleanroom/20261031T223220Z-97490`.
The first runtime check passed 37 of 38 tests, with a path-text comparison failing.
Final repair `cleanroom/20261031T223550Z-1101` tests the actual file contract:
the raw path reads the expected bytes, has the same device and inode as the
owned copy, and differs from the removed SDK source. All lifetime and cleanup
assertions remain.

- `swift-test/20261031T223702Z-1890`: pass, 38 tests, zero Swift warnings.
  Covers native record reads, source failure metadata, cancellation and terminal
  joins, original control operations and the asset-read bridge.
- `private/20261031T223135Z-96778`: repaired native source and tests pass.
- `similarity/20261031T223137Z-97026`: repaired native source and tests pass.
- Final whole-slice similarity and privacy receipts are recorded with the
  combined conflict checkpoint after its two comment-only privacy repairs.

The retained per-SDK worker, post-event scheduling, native conflict metadata and
FFI routing, signed cross-device checks and the batch gate remain pending.
This transport changes no rendered UI. U113 and U114 remain unaccepted.
