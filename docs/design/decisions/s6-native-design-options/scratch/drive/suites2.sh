#!/bin/bash
cd /Users/antoine/.herdr/worktrees/meguri/design
mkdir -p /tmp/claude-501
r() { local name=$1; shift; "$@" > /tmp/claude-501/s.txt 2>&1; local R; R=$(tail -1 /tmp/claude-501/s.txt); echo "$name $(jq -c '{status,message,summary,failures:[.failures[]?|{id,message:(.message[:300])}][:7]}' "$R" 2>/dev/null | cut -c1-1500) $R"; }
S="--suite HomeTests --suite NavigationLayoutTests --suite DeckManagementUITests"
r ui-iphone tools/mg ui-test $S --platform iphone
r ui-ipad tools/mg ui-test $S --platform ipad
r a11y-iphone tools/mg a11y --platform iphone --suite LocalReviewAccessibilityTests --suite ShellAccessibilityTests
r a11y-ipad tools/mg a11y --platform ipad --suite LocalReviewAccessibilityTests --suite ShellAccessibilityTests
