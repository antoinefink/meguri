# Media replay after a crash

Status: two missing U113 witnesses focused-verified; P24 and integration
acceptance remain open. No production behavior changed.
Component start: `c811cabf02489565d8dcefa8c685e8d57a1c047d`.
Unit start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
Integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Implementer: `s-20261101T235351Z-264d9cd0`.
Fixer: `s-20261025T162405Z-9dc656e4`.
Contract: `sync/policy#P24`, its media-conflict item, and
`conformance/cloudkit-acceptance#Q11`.

The four earlier replay cases already passed in
`core-test/20261101T230911Z-3262`. Their individual OK events were checked;
the unchanged cases were not run again. Independent coverage review found that
they did not prove the entire P24 item.

Each test kills and reopens the real child process. The first changes a note
after the rewrite COMMIT while downloaded bytes await installation. Replay
keeps that edit, completes the intent and commits no second rewrite. The second
removes one planned note before replay and preserves changes to the remaining
rows. Both check actual files, collection rows, Service envelopes and durable
acknowledgements on two devices. The moved media name stays live. The second
case propagates the note and card deletion.

Filtered tests: `cleanroom/20261102T011654Z-37273`.
Independent reviewer: `native_bounds_review`; no P0–P2 findings.
Initial focused run `core-test/20261102T012230Z-38235`: two passed, five outside
the selection, with unchanged production.

Source privacy flagged three new fixture strings. The filtered correction
`cleanroom/20261102T012942Z-41451` changed those strings and their exact expected
values together. Review confirmed every predicate, core mutation, crash and
acknowledgement check remained intact. The earlier no-change fixer run had an
incorrect prompt path and supplies no repair evidence.

Final focused run: `core-test/20261102T013545Z-43221`, two passed, five outside
the selection. Source privacy: `private/20261102T013547Z-43446`, pass.
Similarity: `similarity/20261102T013550Z-43702`, pass.
Formatting: `core-format/20261102T013613Z-44140`, pass.
Rust lint: `core-lint/20261102T013647Z-44644`, pass; 17-crate layering check.

P24 still needs a file name without an extension, an existing acknowledged
hash-name entity, and the Q6/fault-lane cases. These focused passes close
neither the full P24 item nor U113. The shared full gate remains required.
No VM, signed network or device test ran during the resize.
