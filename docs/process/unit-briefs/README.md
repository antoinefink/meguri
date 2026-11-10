# Unit brief index

These are implementer-view brief files, prepared 2026-11-10 by the spec-ahead session. They may go stale; the plan and specs win.

Order follows `docs/process/review-first-delivery.md`: prerequisites, then backup/integrity/recovery and file import/export, then the remaining M1 work (editing, search, statistics), then M4. "Held" means in the worktree, not on master.

- `B0-testkit-prepared-forms.md`: no unit (testkit form recognition only). Steps 2 and 3 wait until the held spec, schema, protocol, scenario, golden, fixture and deviations text is committed, because the cleanroom runner copies only tracked files. 76 held stats scenarios and 22 held draft scenarios have no goldens and stay held; they need no B0 work.
- `B5-maintenance.md`: U128, U129, U130, U131, U132. U132 closes only through a rerun of the open maintenance batch gate from base `1416a520` (U127 R1848 still open). U129 waits on U128; U131 waits on U35. The batch crosses the M1/M1R boundary.
- `B4-package.md`: U120, U121, U122, U123, U124. U120 is ready, then U122. U121 waits on U35 and U34's adapters; U123 waits on U33; U124 waits on U120, U122 and U123.
- `B9-optimizer.md`: U47. The policy leg waits on U113; the app leg waits on U51, U114, U151, U154, U160 and U163. The spec writer must resolve the O19 items and O18 corrections, and the uncommitted O17 sentence needs a fix. The held conformance change needs B0's typed steps first. No screen contract exists for the scheduler diagnostics view.
- `B3-notetypes.md`: U33, U34, U35. U33 is ready and can run beside U34. U37 has no `Unit:` trailer, so U34 and U35 cannot claim its acceptance. U35 waits on U34. U36's full gate on its final tip is not recorded.
- `B8-render-editor-core.md`: U71, U190, U191, U192, U197. U191 and U71 are ready. U190 waits on U151 and U37; U192 on U190 and U158; U197 on U190 and U194 (M2, outside the batch).
- `B2-search.md`: U81, U82, U91, U90. U81 is ready, then U82. U91 waits on U81, U82 and U84; U90 on U85, U86, U87 and U91, and `search/accelerators` must set its fallback bound. Activation of the ported search scenarios waits on the R1881 disposition and on the held scenario, golden, spec and deviations text landing. U91's harvested corpus needs another role.
- `B1-browser.md`: U84, U85, U86, U87, U88. U88 is ready. U84 waits on U82; U85 on U81, U82 and U84; U86 on U85; U87 on U86, U35 and U122. The batch crosses the M1/M2 boundary.
- `B6-stats-core.md`: U95, U96, U97, U98, U100. U95 is ready, then U97. U96 needs U88 (for A9 only); U98 needs U82; U100 needs U96 and U97. The M1 gate needs `meguri-host/src/wiring/pending.rs` empty.
- `B7-stats-graphs-cardinfo.md`: U92, U99, U101, U103, U104. None can start; the roots U95, U81 and U88 can. U101 also needs U47. U104 is M3, nobody owns its T3 run yet and its matrix conflicts with `dsl.md` D5. U92 must confirm or ignore the `card-info-computed-retrievability` value. Heatmap goldens stay held until `tools/mg` runs them.
- `B10-integrations.md`: U135, U136, U141, U142, U143, U146 (U137 to U140 status open). Only U135, U136, U141 can start; U142 needs U86, U87 and U95, and U143 needs U142. Nobody owns U136's oracle record job and compose service. A held fixture that three committed transcripts reference is not committed. The U140 V4 row and three recorder items need their owners to resolve them.
