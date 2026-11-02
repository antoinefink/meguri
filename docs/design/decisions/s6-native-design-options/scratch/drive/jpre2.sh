#!/bin/bash
C=/Users/antoine/.herdr/worktrees/meguri/design-clean; cd "$C" || exit 1; git checkout -q 6183f7c9 && git checkout -q d1d2611d -- tools || exit 1
mkdir -p /tmp/claude-501; tools/mg ui-test --suite CollectionEntryJourneyTests/testPickerImportCreatesDurableReview --suite CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace --platform iphone > /tmp/claude-501/j.txt 2>&1; R=$(tail -1 /tmp/claude-501/j.txt)
echo "pre-entry $(jq -c '{status,message,summary,failures:[.failures[]?|{id:(.id|sub("test/";"")),message:(.message[:170])}][:4]}' "$R" | cut -c1-800)"
