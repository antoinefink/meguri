#!/bin/bash
SP=/private/tmp/claude-501/-Users-antoine--herdr-worktrees-meguri-design/bcc4eb73-f59e-4573-8583-70b404dcd81d/scratchpad; U=2ACBD139-2FA6-4E7F-A1DF-97ED508483FB
"$SP/duo.sh" vm "xcrun simctl boot $U 2>&1 | cut -c1-60; xcrun simctl bootstatus $U -b 2>&1 | tail -1 | cut -c1-60"
cd /Users/antoine/.herdr/worktrees/meguri/design
for s in walkthrough walkthroughdark; do tools/mg drive "$SP/drive/$s.json" --platform foldable --device $U > "$SP/drive/o.txt" 2>&1; R=$(tail -1 "$SP/drive/o.txt"); echo "$s foldable $(jq -c '{status,message,failures:[.failures[]?.message][:3]}' "$R" | cut -c1-360)"; dirname "$R" > "$SP/drive/dir-$s-foldable.txt"; done
