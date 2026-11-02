#!/bin/bash
SP=/private/tmp/claude-501/-Users-antoine--herdr-worktrees-meguri-design/bcc4eb73-f59e-4573-8583-70b404dcd81d/scratchpad; M=/Users/antoine/code/meguri; W=/Users/antoine/.herdr/worktrees/meguri/design
C=/Users/antoine/.herdr/worktrees/meguri/design-clean; rm -rf "$C"; git clone -q --shared "$M" "$C" || exit 1; cd "$C" && git checkout -q 943b7f83 || exit 1
mkdir -p build; cp -cR "$W/build/fixtures" build/ 2>/dev/null; for d in core-xcframework xcframework core; do [ -d "$W/build/$d" ] && cp -cR "$W/build/$d" build/ 2>/dev/null; done
mkdir -p /tmp/claude-501
r() { local name=$1; shift; "$@" > /tmp/claude-501/j.txt 2>&1; local R; R=$(tail -1 /tmp/claude-501/j.txt); echo "$name $(jq -c '{status,message,summary,commit:.commit.sha[0:8],failures:[.failures[]?|{id,message:(.message[:220])}][:6]}' "$R" 2>/dev/null | cut -c1-1200) $R"; [ -f "$R" ] || tail -3 /tmp/claude-501/j.txt | cut -c1-200; }
r flows-iphone tools/mg ui-test --suite LocalReviewJourneyTests --suite CollectionEntryJourneyTests --platform iphone
r flows-ipad tools/mg ui-test --suite LocalReviewJourneyTests --suite CollectionEntryJourneyTests --platform ipad
r flows-mac tools/mg ui-test --suite LocalReviewJourneyTests --suite MacHomeHierarchyTests --suite HomeTests --platform mac
