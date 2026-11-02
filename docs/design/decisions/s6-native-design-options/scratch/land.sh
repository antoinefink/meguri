#!/bin/zsh
# Waits for a quiet moment in the main checkout, then fast-forwards master to design.
# Refuses on any overlap with the other agent's uncommitted files. Never forces or pushes.
M=/Users/antoine/code/meguri
W=/Users/antoine/.herdr/worktrees/meguri/design
busy() { pgrep -f 'tools/mg |xcodebuild|swift-build|swift-test' >/dev/null; }
for i in {1..110}; do
  busy || { sleep 8; busy || break; }
  sleep 10
done
if busy; then echo "RESULT still busy after waiting; not landed"; exit 3; fi
if ! git -C "$W" merge-base --is-ancestor master HEAD; then
  git -C "$W" rebase master >/dev/null 2>&1 || { git -C "$W" rebase --abort; echo "RESULT rebase conflict; not landed"; exit 4; }
  echo "rebased onto $(git -C "$W" rev-parse --short master)"
fi
overlap=$(comm -12 <(git -C "$M" status --short | cut -c4- | sort) <(git -C "$W" diff master --name-only | sort))
if [ -n "$overlap" ]; then echo "RESULT overlap with uncommitted work, not landed:"; echo "$overlap"; exit 5; fi
git -C "$M" merge --ff-only design 2>&1 | tail -2 | cut -c1-160
rc=${pipestatus[1]}
echo "RESULT merge exit $rc; master is $(git -C "$M" rev-parse --short master), design is $(git -C "$W" rev-parse --short HEAD)"
git -C "$M" log --oneline -6 | cut -c1-90
exit $rc
