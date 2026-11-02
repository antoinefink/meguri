# Spike S6: native Mac and iPhone Duo design options

Date: 2026-11-01. The Duo captures use the iOS 27.1 runtime on the iPhone Duo simulator. All captures use synthetic fixtures. The picks are in `../decisions.md` and the resulting design in `../native-interface-contract.md` (N12, N13); this record keeps what they were chosen from.

## What is here

- `s6-native-design-options/mac/index.html`: the Mac options, as native captures.
- `s6-native-design-options/duo/index.html`: the iPhone Duo options. Many of these captures are older than the shipped design.
- `s6-native-design-options/final-check/`: the shipped design on the Duo at commit `03d64a36`, opened, half open and closed. `sheet.jpg` marks the fold in red.
- `s6-native-design-options/scratch/`: the session's scripts, with that session's paths. Nothing runs them and nothing maintains them. `duo-preview.patch` and `DuoDesign.swift` are the option switches for the Duo previews, never shipped; `apply-*.py` made the edits now in the app. `tools/vm/duo-fold.sh` replaces the VNC scripts, and `docs/process/mac-ui-vm.md` the disk scripts.

## Not kept

- 11 captions and the images' alt text in these pages, and a few words in the scripts and pages, matched the private-word scan and were removed or changed. Antoine's private pages keep the full text: [Mac](https://claude.ai/artifact/3Eduxn5SCNPtm2hjZrD1oS), [iPhone Duo](https://claude.ai/artifact/Mc8nW8tpmgzuWZy8dqrqQZ).
- The Mac options' code: only their captures remain.
- Raw captures (about 70 MB), tool output and logs. The VNC logs hold session passwords.
