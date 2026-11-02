#!/bin/bash
# Baseline: the same audits at the commit before this work, then back to the work in progress.
SP=/private/tmp/claude-501/-Users-antoine--herdr-worktrees-meguri-design/bcc4eb73-f59e-4573-8583-70b404dcd81d/scratchpad
cd /Users/antoine/.herdr/worktrees/meguri/design || exit 1
git add -A && git commit -qm "WIP: iPhone Duo design (temporary, for a baseline run)

Role: implementer
Session: s-20261101T202238Z-6c79b857
Spec: R9, R10, R576" || exit 1
WIP=$(git rev-parse HEAD); echo "wip $WIP"
trap 'git checkout -q design; git reset -q --mixed '"$WIP"'~1; echo "restored; dirty files: $(git status --short | wc -l)"' EXIT
git checkout -q --detach d1d2611d || exit 1
mkdir -p /tmp/claude-501
r() { local name=$1; shift; "$@" > /tmp/claude-501/b.txt 2>&1; local R; R=$(tail -1 /tmp/claude-501/b.txt); echo "$name $(jq -c '{status,message,commit:.commit.sha[0:8],failures:[.failures[]?|{id,message:(.message[:420])}][:4]}' "$R" 2>/dev/null | cut -c1-1300)"; }
r base-a11y-iphone tools/mg a11y --platform iphone --suite LocalReviewAccessibilityTests
r base-a11y-ipad tools/mg a11y --platform ipad --suite LocalReviewAccessibilityTests --suite ShellAccessibilityTests
