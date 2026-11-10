# Account setup failure evidence

Starting source commit: `a32c096f`; spec-only integration advanced to `364a4b3c`.
Pending iCloud batch. No account setup, signed exchange or unit pass is claimed.

Antoine delegated starting the prepared setup command. Agent attempt `48015`
and Antoine's Terminal attempt `51920` both ended with `setup-refused` before
an approval prompt. No approval was entered. Read-only original-evidence runs
`51344` and `55405` retained those failures. For `51920`, native preflight passed
one test, its attachment checks passed and the actual preflight admission
accepted the unchanged original product. A later owner-stage plan, socket and
incomplete result remained. No hello/candidate stream was retained, and the
exception handler discarded the first failure. This does not prove an account failure
or a missing user step. No runtime producer was repeated by those probes.

The infrastructure repair records only closed phase, refusal and error-kind
categories in the existing private helper stream. Unknown values become fixed
categories. It never prints exception text, observations, account data or
private paths. Public result shape and exit status, approval, fresh matching,
publication, uncertain-publication handling and process cleanup stay unchanged.
Independent review found no P0–P2 issue. Combined tooling run `58031` passed all
three cases, including four new diagnostic tests. The source privacy scan
passed. A new actual setup observation remains pending.

Filtered fixer `57174` addresses a separate source-supported P2: the owner
compared the full dyld image array while its evidence contains only owned
images. Normal SDK calls may add system frameworks. A shared throwing product
comparison now serves owner setup and binding refresh. It keeps complete bundle,
build, OS, executable and anchor identity, unique executable selection, bounded
images and exact ordered owned images. Binding retains its manifest resolver;
owner uses exact paths and encodes the newly checked actual observation. Parent
hash, source, receipt, channel, configuration and fresh-account guards remain.
This was not established as the original cause of `51920`.

Independent review found no P0–P2 issue. Focused Swift run `59381` passed all 17
tests, including direct shared-comparison system-load and product-mutation
regressions. Final source privacy and similarity checks passed. Actual setup
with these corrections remains pending.
