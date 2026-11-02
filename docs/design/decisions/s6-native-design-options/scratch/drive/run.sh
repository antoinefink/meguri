#!/bin/bash
SP=/private/tmp/claude-501/-Users-antoine--herdr-worktrees-meguri-design/bcc4eb73-f59e-4573-8583-70b404dcd81d/scratchpad
cd /Users/antoine/.herdr/worktrees/meguri/design
go() {
  local dev=(); [ "$2" = foldable ] && dev=(--device 2ACBD139-2FA6-4E7F-A1DF-97ED508483FB)
  tools/mg drive "$SP/drive/$1.json" --platform "$2" "${dev[@]}" > "$SP/drive/o.txt" 2>&1
  local R; R=$(tail -1 "$SP/drive/o.txt")
  echo "$1 $2 $(jq -c '{status,message,failures:[.failures[]?.message]}' "$R" | cut -c1-360)"
  dirname "$R" > "$SP/drive/dir-$1-$2.txt"
}
go walkthrough iphone; go walkthrough ipad; go walkthrough foldable; go walkthroughdark foldable
