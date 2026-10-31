# R5 publication diagnostic repair

- Role: fixer. Supplied candidate base `d9df5e6ce59a6c60de93eda65f1dc208d745acf7`;
  original batch base `1416a520542a8c5daa38a23bfe398da704c3d6c9` unchanged.
  Filtered starting HEAD `9e2cfba6bb2378848e7cfc755006a656d0b6409f`.
- Supplied R5 evidence `core-test/20261031T165001Z-89795`: five of six targeted
  regressions pass; retained-flight/current equality still fails in
  `own_landed_save_is_noop_even_after_undo_or_edit_and_retains_a_newer_flight`.
  `Runtime::publication_facts` now resolves each enumerated entity through the
  existing diagnostic `entity_dump`, preserving the original pagination key,
  propagated errors and all binding/root/zone observations. Missing dump rows
  use `InvalidData`, consistent with the existing diagnostic page.
- Direct callers are testkit observation/fixture transport and host tests;
  no production app caller. One source/callsite consistency check only; lazy
  production reads, compact memory proof and assertions remain untouched.
  No network, forbidden sources, builds, tests, formatter or commits. Root owns
  actual verification and independent review; no acceptance claim.
