#!/bin/bash
# AnkiWeb host scan (R1876, U19): no test, script or scenario names an AnkiWeb
# host. Tests build AnkiWeb cases from meguri-sync's ANKIWEB_HOST constant
# (product code, outside this scan) and never write the name out.
#
#   ankiweb-hosts.sh <checkout> [<rev>]
#
# Without <rev> it scans the working tree (tracked and untracked files, not
# ignored ones); with <rev>, that commit's tree. Prints one JSON line per hit
# ({check, path, line, match}) and exits 1 when there is one, 2 when it cannot
# scan. In implementer and fixer worktrees oracle/ is never read (KTD1): the
# merge scans it from the main checkout. Only the files listed in
# ankiweb-allow.txt (the U116 and U255 binary-scan scripts, which search for
# these names) are left out. Runs under macOS /bin/bash 3.2.

set -u
set -f

dir="${1:?usage: ankiweb-hosts.sh <checkout> [<rev>]}"
rev="${2:-}"
here="$(cd "$(dirname "$0")" && pwd -P)"

# Where tests, scripts and scenarios live. Product sources (core/crates/*/src,
# app/Meguri, app/Packages/*/Sources) are not in scope: KTD21 feature-gates
# AnkiWeb there, and U116 scans the built app.
scope=(
  tools/ docker/ conformance/ oracle/
  core/fuzz/ core/tools/ core/crates/meguri-testkit/
  ':(glob)core/crates/*/tests/**' ':(glob)core/crates/*/benches/**'
  app/MeguriUITests/ app/MeguriSnapshotTests/ ':(glob)app/Packages/*/Tests/**' app/Fixtures/ app/TestPlans/
)
# The host name, also with its dot escaped as a regular expression would
# write it (a backslash before it, or a bracket expression around it).
pattern='ankiweb(\.|\\\.|\[\.\])net'

excludes=()
role=""
gitdir="$(git -C "$dir" rev-parse --absolute-git-dir 2>/dev/null)" || {
  echo "ankiweb-hosts: $dir is not a git checkout" >&2
  exit 2
}
[ -f "$gitdir/meguri-session.json" ] && role="$(jq -r '.role // empty' "$gitdir/meguri-session.json" 2>/dev/null)"
case "$role" in implementer | fixer) excludes[${#excludes[@]}]=':(exclude)oracle/' ;; esac
while IFS= read -r line; do
  line="${line%%#*}"
  set -- $line
  [ "$#" -ge 2 ] || continue
  case "$2" in
    U116 | U255) excludes[${#excludes[@]}]=":(exclude,literal)$1" ;;
    *)
      echo "ankiweb-hosts: $here/ankiweb-allow.txt: only the U116 and U255 binary-scan scripts may be allowed, not '$1' ($2)" >&2
      exit 2
      ;;
  esac
done <"$here/ankiweb-allow.txt"

if [ -n "$rev" ]; then
  rev="$(git -C "$dir" rev-parse --verify -q "$rev^{commit}")" || {
    echo "ankiweb-hosts: '$2' is not a commit" >&2
    exit 2
  }
  out="$(git -C "$dir" grep -I -n -o -i -E -e "$pattern" "$rev" -- "${scope[@]}" ${excludes[0]+"${excludes[@]}"})"
  rc=$?
  out="$(printf '%s' "$out" | sed "s|^$rev:||")"
else
  out="$(git -C "$dir" grep --untracked -I -n -o -i -E -e "$pattern" -- "${scope[@]}" ${excludes[0]+"${excludes[@]}"})"
  rc=$?
fi
# git grep: 0 found, 1 nothing found, anything else an error.
case "$rc" in
  0) ;;
  1) exit 0 ;;
  *)
    echo "ankiweb-hosts: git grep exited with $rc" >&2
    exit 2
    ;;
esac
printf '%s\n' "$out" | while IFS= read -r hit; do
  [ -n "$hit" ] || continue
  path="${hit%%:*}"
  rest="${hit#*:}"
  jq -nc --arg p "$path" --arg l "${rest%%:*}" --arg m "${rest#*:}" \
    '{check: "ankiweb-hosts", path: $p, line: ($l | tonumber), match: $m}'
done
exit 1
