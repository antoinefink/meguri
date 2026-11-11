# Export directory access — 2026-11-11

Role: fixer. Starting commit: `f80e88b3`.
Integration batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Contract: `package/file-export#E7.2`.

The signed Mac preview could not prepare an export, with iCloud active or
paused. A debugger inspection identified package I/O and the fixed OS error
“Operation not permitted.” No collection replacement was performed.

Protected external output paths were walked with readable directory handles.
A scoped probe in that sandbox showed that ancestor reads fail while directory
search succeeds along the same path. Root itself permits both.

The walk now uses Darwin `O_SEARCH` or Linux `O_PATH`. Directory, no-follow,
close-on-exec and identity checks remain. Media and retained-tree reads keep
their existing handles. No entitlement or public interface changed.

## Review and focused checks

Filtered author `cleanroom/20261111T092345Z-48748` supplied the patch.
Independent review `cleanroom/20261111T092916Z-61261` found no P0–P2 issues.
The formatter changed line wrapping only.

- Red `core-test/20261111T094835Z-99782`: inventory fails with an access error
  on unchanged production code. The subprocess uses a non-root UID.
- Green `core-test/20261111T095137Z-7437`: that test passes. It checks a
  read-denied, searchable ancestor, missing outputs, payload exclusion and
  hard-link alias refusal.
- `core-test/20261111T095441Z-12692`: all 117 package and host export-safety
  checks pass on the formatted source.
- Lint `20261111T095722Z-22577` and similarity `20261111T095724Z-22860` pass.

Signed-app export verification and the required integration gate remain open.
No Unit or acceptance is claimed.
