#!/bin/bash
# Provenance trailers over a branch (R1958, U19): every commit in <base>..<rev>
# passes the commit-msg hook's rules (tools/hooks/commit-msg --commit), so a
# --no-verify commit or a message-only amend cannot reach the integration
# branch. Merge commits are not checked, as in the hook.
#
#   trailers.sh <checkout> <base> <rev>
#
# Prints one JSON line per rejected commit ({check, commit, subject, message})
# and exits 1 when there is one, 2 when it cannot check. Runs under macOS
# /bin/bash 3.2.

set -u

dir="${1:?usage: trailers.sh <checkout> <base> <rev>}"
base="${2:?usage: trailers.sh <checkout> <base> <rev>}"
rev="${3:?usage: trailers.sh <checkout> <base> <rev>}"
hook="$dir/tools/hooks/commit-msg"
[ -x "$hook" ] || {
  echo "trailers: $hook is missing" >&2
  exit 2
}
commits="$(git -C "$dir" rev-list --no-merges --reverse "$base..$rev" 2>/dev/null)" || {
  echo "trailers: cannot list $base..$rev" >&2
  exit 2
}
rc=0
for c in $commits; do
  msg="$(cd "$dir" && "$hook" --commit "$c" 2>&1 >/dev/null)" && continue
  rc=1
  jq -nc --arg c "$c" --arg s "$(git -C "$dir" log -1 --format=%s "$c")" --arg m "${msg#commit-msg: }" \
    '{check: "trailers", commit: $c, subject: $s, message: $m}'
done
exit "$rc"
