# Manual policy storage

- This root is installation-private, stable across collections/accounts, and outside swept namespaces. No credentials or raw usernames enter it.
- Publish upload intent before dispatch and exact OK before activation. Keep independent artifacts even after errors. Receipt and exact owner release share one durable transition; selected media cannot use the collection-only finisher.
- All current manual transfers are custom-server only (M6): validate retained history but enforce no wait. New v1 receipts use equal completion/eligibility instants; never write the legacy deadline map.
