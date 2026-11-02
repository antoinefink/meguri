#!/bin/bash
C=/Users/antoine/.herdr/worktrees/meguri/design-clean; cd "$C" || exit 1; git checkout -q 6183f7c9 || exit 1; git log -1 --format='baseline %h %s' | cut -c1-70
mkdir -p /tmp/claude-501
r() { local name=$1; shift; "$@" > /tmp/claude-501/j.txt 2>&1; local R; R=$(tail -1 /tmp/claude-501/j.txt); echo "$name $(jq -c '{status,message,summary,commit:.commit.sha[0:8],failures:[.failures[]?|{id:(.id|sub("test/";"")),message:(.message[:150])}][:6]}' "$R" 2>/dev/null | cut -c1-1000) $R"; [ -f "$R" ] || tail -3 /tmp/claude-501/j.txt | cut -c1-200; }
r pre-entry-iphone tools/mg ui-test --suite CollectionEntryJourneyTests/testPickerImportCreatesDurableReview --suite CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace --platform iphone
r pre-mac tools/mg ui-test --suite LocalReviewJourneyTests/testInitialKeyboardReviewAndNativeCloseBackActivation --suite MacHomeHierarchyTests --platform mac
