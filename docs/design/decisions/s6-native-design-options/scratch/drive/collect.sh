#!/bin/bash
SP=/private/tmp/claude-501/-Users-antoine--herdr-worktrees-meguri-design/bcc4eb73-f59e-4573-8583-70b404dcd81d/scratchpad
for k in walkthrough-iphone walkthrough-ipad walkthrough-foldable walkthroughdark-foldable; do
  d=$(cat "$SP/drive/dir-$k.txt"); rm -f "$SP"/captures/duo/v-$k-*.png
  for s in home deck question answer; do f=$(ls "$d"/steps/*snapshot-$s.png 2>/dev/null | head -1); [ -n "$f" ] && cp "$f" "$SP/captures/duo/v-$k-$s.png"; done
  ls "$SP"/captures/duo/v-$k-*.png 2>/dev/null | wc -l | tr -d ' ' | sed "s/^/$k captured: /"
done
P="$SP/vncenv/bin/python"; S="$SP/captures/duo/"
one() { local out=$1; shift; local have=(); for n in "$@"; do [ -f "$S$n.png" ] && have+=("$n"); done; [ ${#have[@]} -gt 0 ] && "$P" -I "$SP/sheet.py" "$S" "${have[@]}" >/dev/null && cp "${S}sheet.png" "${S}$out.png"; }
one sheet-iphone v-walkthrough-iphone-home v-walkthrough-iphone-deck v-walkthrough-iphone-question v-walkthrough-iphone-answer
one sheet-ipad v-walkthrough-ipad-home v-walkthrough-ipad-deck v-walkthrough-ipad-question v-walkthrough-ipad-answer
one sheet-duo v-walkthrough-foldable-home v-walkthrough-foldable-answer v-walkthroughdark-foldable-home v-walkthroughdark-foldable-answer
