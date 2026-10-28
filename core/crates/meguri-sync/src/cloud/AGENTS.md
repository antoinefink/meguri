# CloudKit tracking

- Keep routine CloudKit code here. The sibling `../policy/` holds frozen manual-transfer policy.
- Serialize the private WAL-copy probe/open against every tracking writer and checkpoint, including other stores/builds. The copy shares no SQLite locks with the source; serializing schema setup alone is insufficient.
- `recover` needs caller-held exclusive ownership through move replay, salvage and finalization. It acquires no host lock. Plain `open` refuses outstanding recovery; neither API may be used concurrently with another build's writer/checkpoint.
- Salvage owners live outside ordinary bindings: absent provenance stays optional, and ordinary binding APIs cannot select or mutate them. Conflict envelopes/digests are storage inputs, not semantic record validation.
