# Unicode 10 mark reference data

Spec: render/unicode10-marks
Requirements: R537, R1907; `render/type-answer.md` A6.5
Source: Unicode Character Database 10.0.0, official public data

`unicode10-marks.json` is the canonical set for the leading-mark rule in type-in comparison. A Unicode scalar belongs to this set exactly when its Unicode 10.0.0 `General_Category` is `Mn`, `Mc` or `Me`. This reference does not set the Unicode version used for NFC, NFKD or removal of marks in the nc mode: those remain Unicode 16.0.0.

## Source and integrity

- Data: <https://www.unicode.org/Public/10.0.0/ucd/UnicodeData.txt>
- SHA-256: `52423e4d7492167b62f518f68d54db88930abbbff7f11edfcaec8f726498cab1`
- Input size: 1,735,010 bytes.
- Release notice: <https://www.unicode.org/Public/10.0.0/ucd/ReadMe.txt>, dated 2017-06-18. It identifies the final Unicode Character Database 10.0.0 and carries © 2017 Unicode, Inc.
- The input digest matches the existing independently pinned Unicode 10 reference used by Meguri's filename rules. The input was downloaded from the official URL and its bytes checked before derivation.

No crate table, crate implementation, Anki source or Anki graphic was used to derive this set.

## Derivation and representation

Read semicolon-delimited UnicodeData records. Field 0 is the hexadecimal code point; field 1 is the name; field 2 is `General_Category`. Expand a matched `First`/`Last` record pair inclusively with its shared category. Select only `Mn`, `Mc` and `Me`; exclude surrogate code points. Sort the resulting scalars and coalesce every consecutive run.

The JSON endpoints are uppercase hexadecimal strings with at least four digits. Both endpoints are inclusive. Ranges are sorted, disjoint and maximally coalesced; adjacent ranges have at least one excluded scalar between them. Category changes within a contiguous selected run do not split that run. Missing points, including points not assigned in Unicode 10, are outside the set.

The independent cross-check selected the mark records directly: UnicodeData 10 has no `First`/`Last` pair in these three categories. Expanding the JSON ranges gives exactly that record set, with no additions or omissions. This also checks that all output points are scalars and that every interval is maximally coalesced.

| Category | Scalars |
|---|---:|
| Mn | 1,763 |
| Mc | 401 |
| Me | 13 |
| Union | 2,177 |

The union has **263 ranges**. The canonical JSON's SHA-256 is `53455c47f2d7be861bc4c5f65f8253608462db98276de23e824802e98ad5e72d`.

| Scalar | Unicode 10 category | In set |
|---|---|---|
| U+0300 | Mn | yes |
| U+0335 | Mn | yes |
| U+0488 | Me | yes |
| U+08D3 | Cn | no |
| U+0903 | Mc | yes |
| U+1AC1 | Cn | no |
| U+FE0F | Mn | yes |
| U+E0100 | Mn | yes |
| U+E01EF | Mn | yes |

## License and provenance

- Complete notice: `unicode-data-LICENSE.txt` (Unicode License V3).
- Official URL: <https://www.unicode.org/license.txt>; download date 2026-10-19.
- SHA-256: `e7a93b009565cfce55919a381437ac4db883e9da2126fa28b91d12732bc53d96`.
- Ship the full notice with derived tables and the application's third-party documentation.

This reference was independently derived during spec-writer session `s-20261019T184501Z-da40d377`. It replaces the specification's dependency on a crate's category table with official versioned facts. Rendering behavior and comparison goldens remain unchanged. The implementation may use a static interval lookup; no runtime Unicode-table package is required by this contract.
