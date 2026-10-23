# RF3: Collection-only file export

## Scope and starting point

- Starting master: `64701ca6eaf6d660eb4246b6faa31a955fddb659`.
- Deliver the latest-format whole-collection `.colpkg` file without media through native Mac Save and iPhone/iPad Files. Reuse the existing live package writer, review flush barrier and bounded job ownership. File export remains available offline and without an Apple or server account.
- This covers parts of U124 and U162. Media, legacy formats, deck/selection options and the rest of those units remain open. No `Unit:` acceptance is claimed for a partial delivery.
- Explicit self-hosted Anki-compatible import/export remains next after file export. Routine CloudKit sync remains separate.
- Carry the original RF2 integration base `81c207573c20c337c8834e4ae8b4ee8f29603dbf` into the full native gate. The original RF2 full gate remains failed; its known failures have passing focused follow-ups and its required T1 checks pass. The next full gate must cover all pending RF2 and RF3 changes.

## Spec and implementation

- Spec-writer session: `s-20261023T001251Z-3706ebdb`, recorded on master as `c82e0aaa`.
- The new contract describes the current writer and job interfaces. Code work and its checks follow the spec commit.
- Spec committed as `a7267243`: `package/file-export#E1` through `#E6`. Privacy matches in new prose were removed by rewriting the complete sentences; the contract and scan rules are unchanged.
- Ready export artifacts must stay outside temporary namespaces swept on restart. Flush on the worker before snapshot acquisition; keep the FFI starter short. Retain existing reader leases through writer completion and drain them before replacement can close the collection.
- Keep export files on disk across FFI and native handoff. Do not materialize the package in Swift `Data`, `FileWrapper` or a `FileDocument` snapshot. Preserve existing destinations until verified atomic publication.

## Retained UI work

- RF2 critic findings remain open in U152/U166. Apply the small native iOS inline import-title correction with this UI work and obtain fresh evidence for the affected large-text cells. Keep the full title.
- Home shell evidence and the other full-screen findings remain recorded. No complete Home or import visual acceptance is implied by the local review journeys or this export delivery.
