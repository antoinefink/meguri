# Tag and note type deletion

## Provenance

An independent reviewer read each complete new session record, compared the committed files with their working copies, and admitted them as public operational metadata.

| Session | SHA-256 |
| --- | --- |
| s-20261126T060612Z-93e5d28f | 2ab4e5aa55b3fbdd17c36e564b90093c4c7caba2c8beaf98d3abf0daebd13fc3 |
| s-20261126T060643Z-b3948586 | cdd73d018993523ea43b1576fc830418d3ac2c127e224dfd460ddfc73d496b0e |
| s-20261126T060644Z-6c95dad3 | 9f4c44323272a6942122c4d6d076b398413379f08299c1f827e238755694c5ec |
| s-20261126T061058Z-3869651f | d2f38aca8f99877dcb470d60d1d590dfc1dc03227d5d2d4e26ac8b8bbe5e0c2e |
| s-20261126T061101Z-3b4c7df4 | 5199dd2472d3f435261ecd731ac3e9eb5afef82779f0d5ed41256226ffcb402f |

## Independent review

The original review identified three defects. A pending deletion could reach a replacement collection, note type deletion could refuse a verified unbound sync configuration, and the Mac sidebar lacked its Delete keyboard action. The independent reviewer inspected the corrected call sites, checked the legacy bridge boundary and found all three resolved.

The follow-up covered the Mac menu, keyboard and retry targets, the touch Settings list, local and Meguri Sync note type removal, Undo routing, job-listener failure and observation of a filtered tag tree. The review used the public source in session `s-20261126T060644Z-6c95dad3`. Its two read-only runs were `20261126T062202Z-18483` and `20261126T065207Z-59228`.

The Docker engine regressions and the two new collection replacement regressions passed. The latter use matching identifiers in successive collections and check the successor's data and undo history. The Swift bridge suite exercised committed, refused and cancelled outcomes after listener setup failure. The native journeys use the generated public `typical-1` fixture.

The Mac note type question uses a window-modal AppKit alert. Cancel is its Return default. Escape activates Cancel only in the alert window. Independent review covered presentation ownership, safe dismissal and reconfirmation, and the tag tree invalidation that can occur when saving an inspector edit.

Review of the real FFI route found that a successful structure upload advances the generation before media completion. The fix authorizes that continuation with its recorded operation, account and resulting collection publication. All three production FFI regressions passed, including refusal of a changed publication in the same generation. Review runs `20261126T072108Z-2643`, `20261126T073008Z-18020` and `20261126T074304Z-34407` record the finding and final source review.
