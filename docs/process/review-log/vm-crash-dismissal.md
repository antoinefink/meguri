# Recorded VM crash alert

Start: `16a9fb6505`.
Fixer session: `s-20261024T174115Z-511e3ad6`.
Filtered export: `cleanroom/20261024T201832Z-95121`.

A scoped inspection identified one old Meguri crash alert blocking keyboard
input in the dedicated VM. The latest matching crash report predates the Mac
transfer run. The version-sharing helper correctly refused this different alert.

The new opt-in helper matches the exact two messages and four button facts on
one existing, bound dialog. It requires the VM account, a dedicated marker and
no secure fields. It checks again before one Ignore click, then requires the
same dialog to disappear within five seconds. It never reopens the app or sends
a report. Other helpers and product behavior are unchanged.

Independent R1 review: no P0–P2 findings. Focused checks passed:

- `lint-ui/20261024T202004Z-96963`
- `private/20261024T201952Z-96031`
- `similarity/20261024T202003Z-96734`
- Native compilation and one helper case:
  `vm-dismiss-crash/20261024T202010Z-97703`, no skips; marker removal verified.

The initial lint and similarity invocations could not reach the Docker socket
inside the command sandbox. Authorized Docker invocations above passed.
The Mac transfer must run again after this setup repair. Its earlier failed
receipt remains failed. Integration remains due from
`81c207573c20c337c8834e4ae8b4ee8f29603dbf`.
