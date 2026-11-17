# First-party MIT license

Antoine requested a subagent audit and a switch to MIT if supported on
2026-11-17. Starting commit: `3ba186e736ff135e49b3dfcbe272042782bb2f79`.

Author `/root/mit_license_audit` found no conflicting contribution in the
permitted repository evidence. The owner's instruction authorizes the change
for independently authored Meguri material. Attribution names alone do not
establish ownership.

The root MIT text equals the existing server license. The previous root MPL
text is retained in `LICENSES/MPL-2.0.txt`. Third-party notices and license
obligations remain. Prior dependency reports remain as recorded; the server
assessment has a dated current-policy note.

Independent non-author `/root/provider_public_snapshot_review` cleared the
25-path patch with no P0–P2 findings. Candidate manifest SHA-256:
`f8452f3f73c008d48729ea676e43e3f0242691e393fcb0c9ed7c659d78e4c666`.
Patch SHA-256:
`0e64e8d4d880445ae27a2a6bcf5d8a2a430bb8535c12027bd8067b86bfd8486d`.
The integrator checked every base and candidate hash.

The reviewer also read the complete contents, including all text and paths,
of these files and classified them PUBLIC:

| Path | Lines | Bytes | SHA-256 |
|---|---:|---:|---|
| `LICENSE` | 21 | 1076 | `69a460b3387fadfb592adc582b2e85a4171c2a49969b8cfcb5f0f888ba1796b2` |
| `LICENSING.md` | 9 | 1040 | `98c119e6dedf0192268fc328b9a8e0be58771760a4bc2156387db57ae97c7f2a` |
| `LICENSES/MPL-2.0.txt` | 373 | 16726 | `fab3dd6bdab226f1c08630b1dd917e11fcb4ec5e1e020e2c16f83a0a13863e85` |
| `server/DEPENDENCIES.md` | 278 | 28587 | `33c9b563af531ec0b5cd6b02b0b43d3dec2c5db2aa025d7528a23a896ac79a23` |

The audit used [Mozilla's MPL text](https://www.mozilla.org/en-US/MPL/2.0/),
its [FAQ](https://www.mozilla.org/en-US/MPL/2.0/FAQ/) and the
[OSI MIT text](https://opensource.org/license/mit). Separate first-party MIT
files do not remove the conditions on MPL-covered dependencies.

Only license metadata, comments and current license policy change. No runtime
behavior, dependency version, license allowlist or test assertion changes.
The required app gate and outstanding failures remain in the verification
register; this license change grants no unit or release acceptance.

Focused verification: `selftest/20261117T080738Z-39821` passes both selected
schema/scanner cases. `static-checks/20261117T080742Z-40163` passes all three
checks. Initial privacy failures `080639Z-37615` and `080718Z-39405` remain
recorded. Complete-content review supports only exact public-file hash updates;
scanner logic, private values and access rules are unchanged.
