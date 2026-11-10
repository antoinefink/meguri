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
