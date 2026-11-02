#!/bin/bash
cd /Users/antoine/.herdr/worktrees/meguri/design
r() { local name=$1; shift; "$@" > /tmp/claude-501/s.txt 2>&1; local R; R=$(tail -1 /tmp/claude-501/s.txt); echo "$name $(jq -c '{status,message,summary,failures:[.failures[]?|{id,message}][:8]}' "$R" 2>/dev/null | cut -c1-900) $R"; }
S="--suite HomeTests --suite NavigationLayoutTests --suite LocalReviewJourneyTests --suite DeckManagementUITests --suite CollectionEntryJourneyTests"
r ui-iphone tools/mg ui-test $S --platform iphone
r ui-ipad tools/mg ui-test $S --platform ipad
r a11y-iphone tools/mg a11y --platform iphone --suite LocalReviewAccessibilityTests --suite ShellAccessibilityTests
r a11y-ipad tools/mg a11y --platform ipad --suite LocalReviewAccessibilityTests --suite ShellAccessibilityTests
r ui-mac tools/mg ui-test --suite LocalReviewJourneyTests --suite MacHomeHierarchyTests --suite HomeTests --suite CollectionEntryJourneyTests --platform mac
r snapshots-compare tools/mg snapshots --platform iphone,ipad,mac --filter 'home.*,review.*'
