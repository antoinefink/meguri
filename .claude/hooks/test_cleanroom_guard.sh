#!/bin/bash
# Tests for cleanroom_guard.py: fake workflow agents, one decision per case.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
mkdir -p "$T/sess/subagents/workflows/wf_x" "$T/tmp"
for a in "aimpl U99:implement" "afix U99:fix-r2" "acfix U99:codexfix-r3" "arev U99:review-conformance-r1" "aspec U99:spec-pre"; do
  set -- $a; echo "{\"description\":\"$2\"}" > "$T/sess/subagents/workflows/wf_x/agent-$1.meta.json"
done
fails=0
t() { # agent tool input-json expected
  out=$(jq -nc --arg a "$1" --arg t "$2" --argjson i "$3" --arg tp "$T/sess.jsonl" \
    '{agent_id:$a,tool_name:$t,tool_input:$i,transcript_path:$tp,cwd:"/Users/antoine/code/meguri",session_id:"s"}' |
    TMPDIR="$T/tmp" MEGURI_HOLD_FILE="$T/no-hold" python3 "$HERE/cleanroom_guard.py")
  got=$([ -n "$out" ] && echo deny || echo allow)
  if [ "$got" != "$4" ]; then echo "FAIL: expected $4, got $got: $1 $2 $3"; fails=$((fails + 1)); fi
}
M=/Users/antoine/code/meguri W=/Users/antoine/code/meguri-wt
t aimpl Read "{\"file_path\":\"$M/refs/anki-26.09.3/x.rs\"}" deny
t aimpl Read "{\"file_path\":\"$M/oracle/driver/ops/notes.py\"}" deny
t aimpl Read "{\"file_path\":\"$M/local-data/c.anki2\"}" deny
t aimpl Read "{\"file_path\":\"$W/U21-spec/oracle/driver/x.py\"}" deny
t aimpl Read "{\"file_path\":\"$W/U21/specs/model/records.md\"}" allow
t aimpl Read "{\"file_path\":\"$M/specs/text/strip.md\"}" allow
t aimpl Bash "{\"command\":\"cd $M && tools/mg oracle record smoke/sync\"}" allow
t aimpl Bash '{"command":"cat oracle/driver/ops/notes.py"}' deny
t aimpl Bash '{"command":"git show spec/U13:oracle/driver/ops/sync_server.py"}' deny
t aimpl Bash '{"command":"git log -p spec/U13 -- oracle"}' deny
t aimpl Bash "{\"command\":\"cd $W/U21 && git diff feat/meguri-v1...HEAD\"}" deny
t aimpl Bash "{\"command\":\"cd $W/U21 && git diff feat/meguri-v1...HEAD -- . ':(exclude)oracle'\"}" allow
t aimpl Bash "{\"command\":\"git -C $W/U21 diff --stat feat/meguri-v1...HEAD\"}" allow
t aimpl Bash "{\"command\":\"git -C $W/U21 diff feat/meguri-v1 -- core/crates/meguri-model\"}" allow
t aimpl Bash "{\"command\":\"git -C $W/U21 log --oneline -5\"}" allow
t aimpl Bash "{\"command\":\"git -C $W/U21 show HEAD\"}" deny
t aimpl Bash "{\"command\":\"git -C $W/U21 show HEAD --stat\"}" allow
t aimpl Bash '{"command":"rg check_endpoint"}' deny
t aimpl Bash "{\"command\":\"cd $W/U13 && rg check_endpoint core/\"}" allow
t aimpl Bash "{\"command\":\"cd $W/U13 && git merge --no-edit spec/U13\"}" allow
t aimpl Bash '{"command":"docker run --rm meguri-dev bash"}' deny
t aimpl Bash '{"command":"pip install anki==26.9.3"}' deny
t aimpl Grep "{\"pattern\":\"fn strip\",\"path\":\"$M\"}" deny
t aimpl Grep '{"pattern":"fn strip"}' deny
t aimpl Grep "{\"pattern\":\"fn strip\",\"path\":\"$W/U20/core\"}" allow
t aimpl WebFetch '{"url":"https://github.com/ankitects/anki"}' deny
t aimpl WebFetch '{"url":"https://docs.rs/fsrs/6.6.2/fsrs/"}' allow
t aimpl Write "{\"file_path\":\"$W/U21/AGENTS.md\",\"content\":\"Never read oracle/.\"}" allow
t aimpl StructuredOutput "{\"summary\":\"denied: $M/oracle/x\"}" allow
t aimpl Bash "{\"command\":\"git -C $W/U20 log --oneline attic/U20-unclean\"}" deny
t afix Read "{\"file_path\":\"$M/oracle/x\"}" deny
t acfix Read "{\"file_path\":\"$M/refs/anki/x\"}" deny
t arev Read "{\"file_path\":\"$M/refs/anki-26.09.3/x\"}" allow
t aspec Read "{\"file_path\":\"$M/refs/anki-26.09.3/x\"}" allow
t "" Read "{\"file_path\":\"$M/refs/anki-26.09.3/x\"}" allow
[ "$fails" -eq 0 ] && echo "cleanroom_guard: all cases pass" || { echo "cleanroom_guard: $fails failing"; exit 1; }
