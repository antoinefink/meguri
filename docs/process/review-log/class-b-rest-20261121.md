# Review: class (b) rewrites outside stats and sched (PR #88), 2026-11-21

- Branch `unit/class-b-rest`, author spec-writer session `s-20261121T180837Z-26a035e5`.
- Reviewer: a separate Opus agent. It did not write the rewrites, ran no jobs, and read Anki only at the pinned tag (`refs/anki-26.09.3`). It quotes no Anki text.
- Context: `similarity-test-data-dispositions-20261121.md` (class b, rewrite decided by the coordinator).

## Scope

All files of groups B to E were reviewed, with the specs their headers cite:

- Image Occlusion coordinates: `cardgen/empty/occlusion`, `editor/cloze/output/read-back`, `editor/image-occlusion/{add-check,audio-fields}`, `maintenance/check-db-notes/{all-notes,io-field-tags}`, `render/image-occlusion/format/{cards,parse-escapes,parse-numbers}`, `render/image-occlusion/render/{audio,audio-tags,divs}` and `render/image-occlusion/review/fixtures`.
- Cloze nests: `render/cloze/{nesting,numbers}`.
- Subqueries: `package/apkg-import/guid-other-id` and `all-notes`.
- Reordered ids: `storage/revlog/last-review-pick`.

The reviewer also read all 8 plain column-list drops.

## Findings

- Same behaviour: yes, in every file.
  - The masks moved to .15/.12 overlap no other mask, and no unchanged field in the same scenario refers to them.
  - The cloze nests keep the depth limit. Asking 21, the dropped eleventh marker, gives an empty card, as asking 11 did before. Asking 20 and 11 give the deepest and the outermost cloze. In the 12-level nest, asking 20 leaves `}}}}` as text.
  - `parse-numbers` keeps the four spellings of one (F7.1 quotes the spellings, not their keys).
  - The `NOT EXISTS` and `EXISTS` forms are equivalent: note ids are a non-null key, and the correlation in `all-notes` is on `notes.mid` with the same name list.
  - The W3 row of `specs/editor/cloze-wrap.md` was updated to the new field.
- Goldens: every changed value echoes a moved coordinate, a cloze number shifted by 10, or a permuted spelling. All other goldens changed only `scenario_sha256` and `driver_sha256`.
- New overlap: none. The new strings (`left=.15`, `top=.12`, the `c11` nest, `left=1E0`, both `EXISTS` forms, `INSERT INTO revlog SELECT`) do not occur at the tag.
- Problem found: `render/image-occlusion/review/fixtures` is the source of the review runtime's app fixtures. Six files under `app/Packages/MeguriKit/Tests/CardRenderingTests/Fixtures/image-occlusion/` (IO7, IO9 and IO10, both sides) copy its golden word for word (M15), and no test compares them. A spec writer may not edit `app/`.

## Resolution

The fixtures rewrite was moved out of this PR into #89, where an implementer must copy the six fixture sides from the new golden before the merge. The pixel probes in `ImageOcclusionReviewTests.swift` still hold at the new coordinates. The remaining 23 scenarios have no open finding.

Verdict: approve after the split.
