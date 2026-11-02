#!/bin/bash
SP=/private/tmp/claude-501/-Users-antoine--herdr-worktrees-meguri-design/bcc4eb73-f59e-4573-8583-70b404dcd81d/scratchpad
mkdir -p /tmp/claude-501
r() { local name=$1; shift; "$@" > /tmp/claude-501/j.txt 2>&1; local R; R=$(tail -1 /tmp/claude-501/j.txt); echo "$name $(jq -c '{status,message,summary,commit:.commit.sha[0:8],dirty:.commit.dirty,failures:[.failures[]?|{id:(.id|sub("test/";"")),message:(.message[:170])}][:6]}' "$R" 2>/dev/null | cut -c1-1000) $R"; [ -f "$R" ] || tail -3 /tmp/claude-501/j.txt | cut -c1-200; }
cd /Users/antoine/.herdr/worktrees/meguri/design
r fix-mac tools/mg ui-test --suite MacHomeHierarchyTests --suite HomeTests --platform mac
r fix-iphone tools/mg ui-test --suite HomeTests --suite DeckManagementUITests --platform iphone
r fix-a11y-iphone tools/mg a11y --platform iphone --suite LocalReviewAccessibilityTests
cd /Users/antoine/.herdr/worktrees/meguri/design-clean
r pre-entry-iphone tools/mg ui-test --suite CollectionEntryJourneyTests/testPickerImportCreatesDurableReview --suite CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace --platform iphone
