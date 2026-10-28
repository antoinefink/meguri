# Tags scenario inventory repair

Starting commit: `218ab2e2543a2c4822b79c1ac3520230ca3c1998`.
Pending integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Prepared scenario intake increased the tags inventory from 19 to 24. Preserve
all 19 existing play tests. Name the five prepared completion cases explicitly
and require their typed requests, fresh golden preflight, inactive status and
exactly the missing `tag_complete` capability. A new case or implemented
capability requires corresponding play coverage; it cannot pass silently.

Filtered author: `cleanroom/20261028T110636Z-37610`. Independent review is clear.
Comment-only privacy repair: `cleanroom/20261028T155951Z-87783`, independently
confirmed equivalent; no assertion or executable change.

- `core-test/20261028T160609Z-93896`: all 20 tags tests pass. The shared run
  fails two separate durability observation assertions; those stay open.
- `core-format/20261028T160443Z-92760`: pass.
- `private/20261028T160614Z-94244`: pass.
- `similarity/20261028T160954Z-96386`: final source passes.

This repairs inventory coverage only. U84, its completion adapter and the five
pending cases remain unaccepted. No unit or milestone acceptance is added.
