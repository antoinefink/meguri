# Meguri Sync privacy maintenance

Starting commit: `d0e1947bcf2341a338eedf4ccf7ea1053ac0a80d`.

Antoine approved the reviewed public-word refresh on 2026-11-15 and delegated
the remaining technical maintenance. The primary Meguri agent and sync agent
held Git/token writers during the operation; both were released after success.
No collection was opened and no token spelling was shown to an agent.

The operation uses the existing builder's committed-text and fixed-literal
rules. Only the pinned current commit supplies evidence; staged, untracked,
dirty and private-result files cannot make a token public. It keeps a private
original backup and reports only counts and whole-file hashes. Original build
metadata remains historical. Existing writers do not share a lock, so the idle
window is a required precondition; a post-replace flush error is not rollback.

The actual receipt is
`build/mg/private/20261115T123817Z-75989/result.json`: pass, 52 already-public
entries removed and 82,896 entries retained. No source file in the main checkout
was changed. The shared guard continues to scan all other content.

New technical-contract wording still produced incidental matches. Under the
delegated maintenance authority, seven reviewed exact path/content hashes were
registered: the Rust plan, three native contracts, browser and conformance
contracts, and the machine schema. Authors used only user requirements and
neutral/public repository facts. The original review-note entry is unchanged;
the transport contract has no new entry. No path prefix, directory exclusion,
editable manifest or automatic approval rule was added. Changed bytes lose the
exception.

Independent read-only reviews found no actionable defect in the maintenance
operation or the final grant/test changes. Synthetic checks cover committed-only
learning, unknown-word retention, private-result/fixed-literal exclusions, path
and file safety, backup/failure behavior, explicit shared-target admission, and
no token output. Document tests now exercise synthetic approved bytes rather
than requiring a mutable plan to keep its historical hash. Published-library
tests retain their immutable notice/library and vendor fixtures.

Passing Docker checks:

- Scratch: `build/mg/selftest/20261115T121311Z-54472/result.json` in
  `/private/tmp/meguri-public-baseline-v2-d0e1947b`.
- Canonical: `build/mg/selftest/20261115T125457Z-89822/result.json`, three cases,
  including the scanner unit suite; no failures or skipped cases.

Earlier failed receipts are retained: a synthetic word accidentally present in
the fixture's committed text, and two live-document test assumptions exposed by
the Rust plan edit. Their corrections changed test setup, not privacy matching.
