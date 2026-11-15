# Logical maps preserve stored gaps

Start: `59a8c2f91`. The full-batch base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`. Source remains a prepared candidate; no Unit or integration acceptance.

N1 defines ordinals by list position. F4 keeps stored keys. The candidate now counts the actual rows. Large stored gaps add no map entries. The previous draft used the wrong domain. Existing memory limits still apply. No new format or limit is introduced.

The independent spec reading is `build/cloud-resume-diagnosis/U113-map-domain-reading-20261115.md`. It also records a narrow future field-editor cross-reference ambiguity; this repair does not change that text.

Author `cleanroom/20261115T194007Z-81395` added two tests. `core-test/20261115T194508Z-87550` executed both and failed safely on the old extent check. Both reached the intended paths before any large allocation. The native case calls production rename after separate fixture setup.

Fixer `cleanroom/20261115T194801Z-91290` changes list arities, identity construction, position validation and before-shape counts. Its fixture migration declares positional origins and preserves all existing physical-row assertions. The integrator read every incremental source change and verified exact applied bytes. Independent review `cleanroom/20261115T195414Z-1503`: bounded CLEAR P0–P2.

- `core-test/20261115T195829Z-6528`: PASS, 52 executed, zero failures or skips. Six filtered cases are excluded. This includes the previous 50 checks, large stored keys, exact metadata and undo/redo. The new zero-note case makes no card-generation claim.
- `private/20261115T195955Z-9241`: PASS for four changed files; guard unchanged.
- `similarity/20261115T195955Z-9359`: PASS, four files, zero flags or exceptions. Filtered implementer report read.

Temporary source changes were reversed exactly. The prepared export is retained at `build/cloud-resume-diagnosis/structure-report-prepared-logical-20261115.patch`, SHA-256 `792620bd171914c9c13717e0b6320fe2a992e3cfaab3cbc8932b020e46ce196f`. The older pinned export is unchanged.

The larger candidate still needs the accepted-aggregate implementation, cloze composition/redo, new-type and token work, remaining fixture migrations and full checks. The separate incoming-mid conflict also remains open. No structural-generation, live peer, milestone or release pass is claimed.
