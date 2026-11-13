# Mac driver background-state check

Start: `f5bb13d4`. Role: fixer. Session: `s-20261110T214025Z-20fa15fd`.

The Mac SDK has no suspended-background case. The driver predicate and its
two test lists now use each platform's supported states. iOS keeps both
background cases. Test functions, assertions and bounds are intact.

Filtered exports: `20261113T075641Z-32221` and `20261113T080416Z-74990`.
Independent code review: `20261113T081140Z-98543` cleared this two-file part.
Its other findings concern review lifecycle code, excluded from this commit.

The initial Mac build failed in `ui-test/20261113T075514Z-26205`.
With these two files, `ui-test/20261113T082650Z-14633` passed 16/16, no skips:
The selected cases are named in the receipt. They use the existing production
review code, with no opacity edit.

The full batch gate uses `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
This focused result gives no unit or milestone acceptance.

Static checks `20261113T083030Z-17928` passed 3/3. Similarity
`20261113T083033Z-18564` passed 2/2 with no exemptions.
Staged privacy `20261113T083115Z-18974` passed.
