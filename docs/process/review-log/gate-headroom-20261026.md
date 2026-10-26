# Integration gate headroom

Gate `20261026T035002Z-43555` used the original pending base
`1416a520542a8c5daa38a23bfe398da704c3d6c9` on `e36d5e12`.
Static checks, canaries, lint and dependency checks passed. Rust
`035121Z-52222` completed with 3,094 passed, two failed and ten skipped.
Both large 150,000-row merge/suspension cases passed. Native durability
`041414Z-59768` completed with 24 passed and one failure, the same suspension
fixture. Their focused repairs have separate U32 and RF4 review records.

Free disk fell to 39 GB during the native/conformance build. The orchestrator
verified the owned gate process and sent SIGTERM. MG stopped its children;
the gate result explicitly reports cancellation and incomplete checks. Completed
child results are retained. The interrupted conformance and later checks are
not passes, and this gate is not accepted.

The approved Meguri-only cleanup ran after the jobs stopped. Reproducible source
copies from 80 completed filtered runs were also removed, preserving every
exported patch, seed, manifest, result, log and report. Base commits were retained
and checked for reachability before older copies were reclaimed. Unfinished and
failed work was not removed. No other project's Docker data was touched.

Free space returned to about 60 GB. Compiled products remain available for reuse.
Future gates retain the original base and must have enough headroom for remaining
build configurations. A credential-sensitive Mac proof requires the existing
team-signing mode; an ad-hoc build cannot supply that proof.
