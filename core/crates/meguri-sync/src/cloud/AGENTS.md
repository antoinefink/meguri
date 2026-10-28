# CloudKit tracking

- Keep routine CloudKit code here. The sibling `../policy/` holds frozen manual-transfer policy.
- Serialize the private WAL-copy probe/open against every tracking writer and checkpoint, including other stores/builds. The copy shares no SQLite locks with the source; serializing schema setup alone is insufficient.
