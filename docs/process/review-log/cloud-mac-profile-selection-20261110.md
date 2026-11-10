# Mac CloudAgent development profile selection

Starting commit: `94cf6e71`. Infrastructure work within the pending iCloud batch;
the integration base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
No unit or signed-sync acceptance is claimed.

Antoine delegated the profile setup and approved saving it. Read-only check
`16270` verifies the installed file for the host, VM, app and certificate.
Setup `13422` installed it but its subsequent
verification failed because its temporary wrapper reused an exhausted input
descriptor. That failure is retained; fresh-descriptor verification did not
repeat the cache write.

Actual smoke `22453` built Mac product `23490` successfully, including the new
Rust/Swift bridge. Delivery `26978` still refused at
`recheck-build/profile-device`: Xcode's automatic selection embedded the older
profile. Installed-profile readiness therefore did not establish build or
delivery readiness. No cloud operation ran.

The filtered app configuration export `29397` adds only app-target macOS style
and profile settings with unchanged defaults. The tooling selects them only for
an explicit team-signed Mac Debug CloudAgent build-for-testing. An optional
private profile file must match its installed copy. Checks cover the host, app
identity, entitlements and certificate.
The same app-only selection reaches the build and first-freeze settings; profile
and source checks remain required. Widget/test settings and ordinary/mobile
routes remain unchanged. Existing final signature, profile and delivery guards
are unchanged. No actual profile, account or connection values are recorded here.

Focused tooling check `32927` passed both cases. Independent review found one
P2: CMS decoding reopened the profile path after its bytes had been captured.
The repair must decode those exact bytes and add a substitution regression while
retaining cache/source checks. Further focused checks and actual signed execution
remain pending.

The repair decodes captured bytes on stdin, with bounded timeout/output and
fixed refusal messages. It adds transient and persistent input substitution
cases and native decode failures. Independent delta review is clear of P0–P2
findings. `selftest/20261110T165508Z-34326` passed all 27 affected CloudAgent
tooling checks. Actual native profile selection and cloud execution remain
pending; fixture checks do not establish either.

Native parent `37903` stopped at build preflight `38548`, before compilation,
with a profile-validation refusal. Added a closed-code diagnostic so that this
failure can be diagnosed without exporting its private native output. Independent
review is clear; focused tooling check `39922` passed all three cases. The failed
parent remains retained. Next is an individual build check, not another full
parent run.

Individual build `41954` reported `profile-target-scope`: the execution scheme's
settings include the app and test host, but omit the implicit widget target.
The preflight now queries all targets read-only with Debug/macOS settings and
output roots under its existing DerivedData. It still requires exactly one
widget and test target and unchanged signing properties; missing/duplicate
targets in either query remain failures. Real build/freeze scheme and plan are
unchanged. Independent delta review is clear. Focused check `44089` passed all
three cases. Native verification follows this correction.

Native `45132` then stopped at `profile-settings`. Read-only maintenance
`48298`/`49962` identified two app rows in Xcode's all-target output; both had
the expected app identity and signing intent. Widget query `50559` returned one
correct widget row with unchanged signing. These were explicit maintenance
runs on a dirty checkout, with no product mutation or signed evidence.

The repair restores the actual scheme query for the app/test pair and adds an
explicit widget query for each comparison. Every query retains exact role/count
checks; it neither deduplicates rows nor permits missing targets. Independent
delta review is clear. Focused `51545` passed all three cases, including missing,
duplicate and unexpected-role regressions. Native verification remains pending.

Native `53014` refused the scheme-query shape. The earlier
`agent-cloud-settings.json` artifact was a filtered policy projection and did
not prove the full native response shape. Read-only maintenance `54122` then
verified separate explicit queries for all three targets: each returned exactly
one matching role, identity, platform and signing intent, with only the app's
profile selection changed. No product was mutated.

The resolver now uses those proved named-target queries for both phases. Exact
role/identity checks remain; the actual build/freeze scheme remains unchanged.
Independent review is clear, and focused `54770` passed all three cases, including
per-role metadata mismatch checks. This supersedes the query choices above;
their failed native receipts remain retained. Native product verification is
still required.

Native `61801` reached signer selection and refused `profile-signer`. Maintenance
`73247` proved one valid private-key identity and one matching profile certificate,
but zero matches under the helper's display-label suffix filter. The actual DER
team field, development certificate name and expiry checks passed. The selector
now checks those certificate facts directly and retains its one-candidate rule.
Regressions cover a person suffix different from the team, wrong DER team/name,
expiry and ambiguity. Independent review is clear; focused `83778` passed all
three tooling cases. Native verification remains pending.
