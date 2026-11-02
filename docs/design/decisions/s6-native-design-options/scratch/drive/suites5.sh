#!/bin/bash
cd /Users/antoine/.herdr/worktrees/meguri/design
SP=/private/tmp/claude-501/-Users-antoine--herdr-worktrees-meguri-design/bcc4eb73-f59e-4573-8583-70b404dcd81d/scratchpad
mkdir -p /tmp/claude-501
r() { local name=$1; shift; "$@" > /tmp/claude-501/s.txt 2>&1; local R; R=$(tail -1 /tmp/claude-501/s.txt); echo "$name $(jq -c '{status,message,failures:[.failures[]?|{id,message:(.message[:200])}][:4]}' "$R" 2>/dev/null | cut -c1-800) $R"; python3 -I "$SP/asum.py" "$R" 2>/dev/null | tail -n +2 | cut -c1-420; }
r a11y-iphone tools/mg a11y --platform iphone --suite LocalReviewAccessibilityTests --suite ShellAccessibilityTests
r a11y-ipad tools/mg a11y --platform ipad --suite LocalReviewAccessibilityTests
r ui-iphone tools/mg ui-test --suite HomeTests --suite NavigationLayoutTests --suite DeckManagementUITests --platform iphone
r ui-ipad tools/mg ui-test --suite HomeTests --suite NavigationLayoutTests --suite DeckManagementUITests --platform ipad
r mac-build tools/mg build-app --platform mac --config Debug
