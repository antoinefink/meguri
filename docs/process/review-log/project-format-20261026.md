# Project format integration

Starting commit: `be370f3d9a0e7d0f80aa4d03e94b07302294be6a`.
Session: `s-20261026T022553Z-c480b018` (infra).
Pending integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The same project rewrite repeatedly returned after it was preserved separately,
preventing clean-room intake. Three held patches were byte-identical. Source
inspection found no explicit MG project writer; the exact writing process is
unproven. The format matches Xcode serialization, but this alone is not proof
that every change is safe.

Independent review checked decoded shell bodies, build phase ordering and
target references. Defaults and ordering changed; widget destination 13 became
PlugIns. Eight signing-file membership exceptions were removed. Product evidence
and the structural parser repair are required before accepting this update.

## Focused evidence

- `build-app/20261026T022222Z-85779`: Mac Debug build passed.
- `build-app/20261026T022310Z-87230`: iOS simulator Debug build passed.
- Artifact inspection found zero entitlement resource files in either app.
  The Mac widget is under `Contents/PlugIns`; the simulator widget is under
  `PlugIns`. Build receipts retain the normal signing checks.
- The project SHA256 was unchanged before and after both builds:
  `f3445516e72415d9eb3c1fce2bf5feb3047309e1b9f5df533f7fb55c45e1aec4`.
- `selftest/20261026T022443Z-88795`: the existing structural test failed because
  it read only the first line of the script field. The repair decodes both
  literal forms, checks the exact script body, and rejects unsupported forms.
  Phase ordering, dependencies, sandbox paths and resource wiring stay checked.
- `selftest/20261026T022819Z-90501`: PASS; 14 executed; 0 skipped.
- `private/20261026T022907Z-91058`: PASS for the owned format/parser/doc paths.

Final independent source review found no P0–P2 issue for bounded adoption.

Profile, Release, AppStore and physical-device signing evidence remain required
through the normal Xcode and integration gates. This update carries no Unit
acceptance trailer and does not accept a milestone or release.
