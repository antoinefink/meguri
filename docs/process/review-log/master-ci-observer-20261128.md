# Manual-transfer observer self-test

Base `d26571256c43ca949f614e51f3e73fae81cc44df`.
[The master run](https://github.com/antoinefink/meguri/actions/runs/46121671750)
reported one failed test out of 152 in shard 1. Bats showed the Python exit
status assertion and omitted the traceback. The original Python failure
remains unconfirmed.

The unchanged case passed in isolation (`selftest/20261128T075657Z-98670`).
All 152 cases passed in a local run of shard 1 (`selftest/20261128T075720Z-99173`).

The shutdown test read the PID file as soon as the path existed. A probe
paused the synthetic reader after creating the file and before writing its
PID. The test parsed an empty string (`selftest/20261128T075932Z-512`).
The synthetic reader now writes the PID under a temporary name, then renames
the file into place. The test passed with the same pause before publication
(`selftest/20261128T080032Z-1725`). A separate probe confirmed that a direct
Python failure appears in Bats output (`selftest/20261128T080106Z-2470`).
All 39 manual-transfer Bats cases passed with the fix
(`selftest/20261128T080031Z-1417`).

The independent Codex testing reviewer found no blocking issues and confirmed
that the shutdown assertions and deadlines still apply to EOF, TERM and HUP.
The reviewer classified the complete session record
`s-20261128T075956Z-d52b67df.json` as public operational metadata at SHA-256
`381e37ac8f9781b29288298e12efee819a8c33a3634d18059f428b78349442fe`.
