# tools/similarity/

- Never print, log or write corpus text. Reports carry scores, Meguri lines and, in the reviewer view only, Anki source, tag, path and lines. Test assertions and messages use numbers; calibration positives are built from the corpus inside the container's /tmp and never land in the repo or build/.
- The implementer view (`implementer.*`, `summary.json` flags, tools/mg failure messages) must never name an Anki path or tag: U19 passes it to implementer sessions.
- The corpus build (`corpus.py`) and the scan (`scan.py`) must lex, filter (Rust attribute collapse, struct periodicity) and hash identically; both go through `IdSpace` and the same params. Anything that changes corpus hashes or the lines stored with them goes into `plan_corpus` params or bumps `INDEX_VERSION`, or stale caches give wrong matches.
- Scan paths are made checkout-relative (`normpath`) before scope, allowlist and report: `./oracle/x` must hit the `oracle/**` exclusion. Paths outside `--root` are an error. `tools/mg similarity` normalizes given paths the same way (`mg__similarity_relpath`) before `--rev` checks out files, so keep the two in step.
- A `--rev` scan applies the allowlist of the checkout that runs it (the main checkout, on the integration branch), not the scanned branch's. An approved entry must land on the integration branch before the unit's merge gate can pass.
- Thresholds, filters and the corpus pin (`corpus.tags`) change only with a `tools/mg similarity-selftest` calibration run from the main checkout (it needs refs/).
- `tools/mg.d/similarity.sh` reads the `<dir>/**` entries of `scan.exclude` with awk to keep those directories (oracle/) out of `--rev` trees: keep it a flat list of quoted globs, one per line.
- `build/similarity-index/` is shared by concurrent scans, across containers: build only in a `mkdtemp` `.tmp-*` directory, and publish, evict or open an index only under the `.lock` flock (`corpus.py`).
- The scanner runs from the bind mount (`PYTHONPATH=/work/tools/similarity`): code changes need no image rebuild. numpy and PyYAML come from the image's Ubuntu packages.
- JPlag reports (`similarity-nightly`) embed the compared Anki sources: keep them in the main checkout's build/.
- `similarity --search-data -- <paths>` is a separate paired-data lane for only the two search scenario/golden roots. Default exclusions stay in force elsewhere. Read/hash/scan the same nofollow bytes; keep aliases, links and unpaired inputs refused. A limited-fingerprint warning or any similarity result does not prove R1881 provenance.
