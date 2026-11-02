# Isolated CloudKit development setup

Status: imported and exported schema verified. Product-grant verification remains open; readiness remains NO.
Source: a12118d3. U113 start and original integration base are unchanged.

Antoine authorized native screen control for setup, approved creation of the
isolated development container, and approved its exact development-only app
assignment. Apple saved the assignment. A fresh app configuration showed one
container with CloudKit support and push enabled. The signed physical check
ui-test/20261102T175312Z-9378 passes1/1, with no failures or skips. Its isolated
review stand-in is not a real review or cloud synchronization result.

CloudKit Console then showed only the built-in Users type in development.
Root prepared an additive schema from that exported baseline and K3: preserve
Users and add MGActive, MGEpoch, MGMediaRoot, MGEntity, MGGeneration and MGBlob.
Each custom type has only encrypted bytes e, plus pa on MGEntity, ma on
MGGeneration and a on MGBlob as ASSET. No new grants or custom indexes were
requested. The artifact remains in ignored build output.

Antoine approved the exact file upload and schema import, then explicitly
approved the Console warning that importing replaces the development schema.
The validated file was imported through the Console. It reported successful
import, and its saved inventory showed all six custom types: seven fields for
the three payload-only types and eight for each type with an asset, including
CloudKit's six implicit system fields. Production was not changed.

The export preview was truncated. Root then downloaded only the schema
export and parsed the complete2145-byte file. It contains exactly Users and
the six custom types. Every custom e is ENCRYPTED BYTES; pa, ma and a are
ASSET on their specified types. There are no extra custom fields or grants.
The Users definition is preserved. The local fixed-result artifact records
six encrypted fields and three asset fields; it proves schema shape only.

No unrelated page action was taken. Product profile grants and packaged
settings still require their integration checks before enabling development
readiness. Both source readiness flags remain NO. No account identity,
credential, record contents or private UI capture is stored in this log.
Test-account approval and signed sync evidence remain open.
