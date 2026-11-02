#!/bin/bash
cd /Users/antoine/.herdr/worktrees/meguri/design
SP=/private/tmp/claude-501/-Users-antoine--herdr-worktrees-meguri-design/bcc4eb73-f59e-4573-8583-70b404dcd81d/scratchpad
mkdir -p /tmp/claude-501
r() { local name=$1; shift; "$@" > /tmp/claude-501/s.txt 2>&1; local R; R=$(tail -1 /tmp/claude-501/s.txt); echo "$name $(jq -c '{status,message,failures:[.failures[]?|{id,message:(.message[:170])}][:4]}' "$R" 2>/dev/null | cut -c1-700) $R"; python3 -I "$SP/asum.py" "$R" 2>/dev/null | tail -n +2 | cut -c1-360; }
r a11y-iphone tools/mg a11y --platform iphone --suite LocalReviewAccessibilityTests --suite ShellAccessibilityTests
r design-swift tools/mg swift-test --package MeguriKit -- --filter 'FoldAwareLayoutTests|ContrastTests|TokenCoverageTests'
r snap-record tools/mg snapshots --platform iphone,ipad,mac --filter 'home.deckList*,review.question*,review.answer*,review.congratulations*' --record 'home.deckList*,review.question*,review.answer*,review.congratulations*'
r snap-check tools/mg snapshots --platform iphone,ipad --filter 'home.*,review.*'
