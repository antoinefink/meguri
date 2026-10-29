# U162 native export footer alignment

Status: focused repair verified; unit and integration acceptance remain open.
Start: `af2cee18745c64d2cf36ef1dfbf5c7ede88428b0`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Independent review of Mac run `snapshots/20261029T074632Z-91949` found one
P2: the footer and body had different alignment in wide windows. The
production destination has no fixed width.
The same issue appears on regular iPad in run `20261029T075400Z-97755`.
The deck-list render has no P0-P2 visual finding. Its native outline remains
a changed-reference candidate, not an approved replacement.

Fixer `cleanroom/20261029T075643Z-1214` gives the footer the body's existing
inner and outer width containers. It preserves the native button, padding,
keyboard shortcut, accessible action, safe-area placement and dismissal rules.
Independent source review is clear. No new test was added for this layout-only
change; existing hosted renders exercise it.

Verification:

- `snapshots/20261029T075824Z-3059`: Mac normal, regular iPad and compact iPad
  AX5 render the exact repaired input
  `7940deb05619f448d3a865df950d9004cecd719bd6e78e99929a0297a7f075bd`.
  Root and independent visual review confirm alignment, visible controls and
  wrapping, with no new P0-P2 finding. Native lint and builds pass within the job.
- The job remains RED on all three reference comparisons. No reference was
  changed, recorded or approved. Earlier 66 Mac and 16 iPhone differences remain
  open. The preceding iPad job selected two export variants; the requested home
  filter matched none and supplies no home evidence.
- `private/20261029T075827Z-4116`: one source file passes.
- `similarity/20261029T075825Z-3177`: one source file, no findings or exemptions.

Synthetic static renders do not prove scrolling, activation, actual modal size,
all states or final accessibility. Required platform, integration and human
design evidence remains outstanding. No Unit trailer.
