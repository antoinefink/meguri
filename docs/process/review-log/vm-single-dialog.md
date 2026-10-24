# Single-dialog VM inspection

Start: `d40747787d0f9fc78f58110663f561cc9a438652`.
Fixer session: `s-20261024T174115Z-511e3ad6`.
Filtered export: `cleanroom/20261024T200819Z-86884`.

The Mac transfer run `20261024T194841Z-76432` encountered a system dialog before
Settings opened. The existing exact-prompt action `20261024T200136Z-82951`
stopped at its text/button guard without input. Its opt-in marker was removed
and removal was verified; the dialog has not been approved.

The inspection helper now requires one existing dialog and captures only its
bounded text, tree and image. It rejects secure fields and checks stability
before releasing any attachment. It no longer captures other windows or the
helper-wide tree. The existing action helper is unchanged.

Independent review: no P0–P2 findings. Checks passed:

- `lint-ui/20261024T201229Z-88738`
- `private/20261024T201228Z-88517`
- `similarity/20261024T201230Z-88927`
- Native compilation and one inspection case:
  `vm-single-dialog/20261024T201246Z-89620`; marker removal verified.

The three scoped attachments identify a Meguri crash alert. No action was taken.
The failed Mac transfer remains open. The shared integration gate remains due
from `81c207573c20c337c8834e4ae8b4ee8f29603dbf`.
