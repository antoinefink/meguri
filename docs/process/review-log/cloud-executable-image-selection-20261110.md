# Loaded executable selection

Starting commit: `6c21ded4`. Pending iCloud batch; no unit or signed-sync acceptance.

Actual signed Mac parent `11247` passed build and VM delivery, then its path
preflight executed one test with no failures or skips. Parent admission refused
`setup-image-path`. Read-only checks of the original evidence (`17320`, `19253`,
`20037`, `20790`) found that the reported main image was the injected system
library. The OS executable, host executable, agent anchor and all three owned
images matched the frozen products. The original evidence stays unchanged.

Filtered fixer `23443` replaces the dyld index assumption with selection of one
loaded image matching the observed OS and host executable. Both path producers
use exact observed paths; product binding alone resolves manifest-declared
internal aliases. Binding and refresh keep the existing executable, anchor,
owned-image, CPU, subtype, UUID, hash, receipt and filesystem checks. Unrelated
system image order does not select or replace the executable.

Independent review found no P0–P2 issue. Tests cover system injection, missing or duplicate image records, wrong
identity, internal aliases and retained refresh fences. Focused Swift run `28347` passed all 16 tests. Similarity run `29975` passed.
Privacy run `30301` flagged a new comment; filtered follow-up `30969` changed
only that comment, preserving every tested source statement and assertion. The
final combined source scan passed. Real app-hosted preflight remains pending.
No owner setup entry point was invoked. Cross-device CloudKit remains unverified.

Actual signed Mac smoke `32638` built `33348`, passed VM delivery and admitted
the new preflight evidence. It then reported `awaiting_user` for
`cloudkit-test-accounts` before command delivery or agent execution. Read-only
metadata run `39718` proved the host approved source file is absent while its
private parent directories have the expected authority. No account values were
read, and no setup or exchange was attempted. The missing owner-step heading
is now documented; the prepared local command has not been invoked.
Final committed-source similarity run `33618` passed. No full gate or signed
exchange pass is claimed.
