#!/bin/bash
# No silent retries (R1953, U19): a flaky test fails the gate and is
# quarantined with an owner and a deadline; nothing reruns it until green.
#
#   no-retries.sh <checkout> [<rev>]
#
# Checks, in the working tree or in <rev>'s tree:
# - every Xcode test plan (app/TestPlans/*.xctestplan): no test repetition
#   mode but `none`, no maximumTestRepetitions, anywhere in the plan;
# - nextest configuration (any nextest.toml): no `retries` but 0;
# - browser runner configs: literal zero repetition only;
# - tools/, docker/ and server/: no xcodebuild -retry-tests-on-failure or
#   -test-iterations, no nextest --retries or NEXTEST_RETRIES but 0.
# Prints one JSON line per finding ({check, path, line, message}) and exits 1
# when there is one, 2 when it cannot check. Runs under macOS /bin/bash 3.2.

set -u

dir="${1:?usage: no-retries.sh <checkout> [<rev>]}"
rev="${2:-}"
here="$(cd "$(dirname "$0")" && pwd -P)"
if [ -n "$rev" ]; then
  rev="$(git -C "$dir" rev-parse --verify -q "$rev^{commit}")" || {
    echo "no-retries: '$2' is not a commit" >&2
    exit 2
  }
fi

finding() {
  jq -nc --arg p "$1" --argjson l "$2" --arg m "$3" '{check: "no-retries", path: $p, line: $l, message: $m}'
}

# files <ERE>: paths in the tree under test that match, one per line.
files() {
  if [ -n "$rev" ]; then
    git -C "$dir" ls-tree -r --name-only "$rev"
  else
    git -C "$dir" ls-files --cached --others --exclude-standard
  fi | grep -E "$1"
}

# content <path>
content() {
  if [ -n "$rev" ]; then
    git -C "$dir" show "$rev:$1"
  else
    cat "$dir/$1"
  fi
}

# grep_tree <ERE> <pathspec>...: path:line:text of each match.
grep_tree() {
  local re="$1" out rc
  shift
  if [ -n "$rev" ]; then
    out="$(git -C "$dir" grep -I -n -E -e "$re" "$rev" -- "$@")"
    rc=$?
    out="$(printf '%s' "$out" | sed "s|^$rev:||")"
  else
    out="$(git -C "$dir" grep --untracked -I -n -E -e "$re" -- "$@")"
    rc=$?
  fi
  [ "$rc" -le 1 ] || return 2
  [ -z "$out" ] || printf '%s\n' "$out"
}

scan() {
  local plan bad cfg n text hits hit path rest
  for plan in $(files '^app/TestPlans/[^/]*\.xctestplan$'); do
    bad="$(content "$plan" | jq -r '[.. | objects | select((has("testRepetitionMode") and .testRepetitionMode != "none") or has("maximumTestRepetitions"))] | length' 2>/dev/null)"
    if [ -z "$bad" ]; then
      finding "$plan" 0 "not readable as a test plan (JSON)"
    elif [ "$bad" != 0 ]; then
      finding "$plan" 0 "repeats failing tests (testRepetitionMode other than none, or maximumTestRepetitions): turn retries off"
    fi
  done
  # Browser runners may use expressions for CI. Only an explicit zero is
  # acceptable; unknown/multiline values fail closed instead of becoming a
  # hidden second attempt. Configs outside tools/ are covered too.
  # Vitest also automatically loads vite.config files. The browser helper owns
  # NUL-safe enumeration and checked reads in both working and retained trees.
  python3 "$here/browser-retries.py" "$dir" "$rev" || return 2
  for cfg in $(files '(^|/)nextest\.toml$'); do
    content "$cfg" | grep -n -E '^[[:space:]]*retries[[:space:]]*=' | while IFS=: read -r n text; do
      case "${text#*=}" in
        *[!0[:space:]]*) finding "$cfg" "$n" "nextest retries are set (${text#"${text%%[![:space:]]*}"}): use 0" ;;
      esac
    done
  done
  # Retry switches in the scripts that run tests. This file names them, so it
  # is left out.
  hits="$(grep_tree '(-retry-tests-on-failure|-test-iterations|--retries[ =]*[1-9]|NEXTEST_RETRIES=[1-9])' \
    tools/ docker/ server/ ':(exclude)tools/gate/static-checks/no-retries.sh')" || return 2
  [ -n "$hits" ] || return 0
  printf '%s\n' "$hits" | while IFS= read -r hit; do
    path="${hit%%:*}"
    rest="${hit#*:}"
    finding "$path" "${rest%%:*}" "a test command retries failing tests: run once and report"
  done
}

out="$(scan)"
rc=$?
[ -z "$out" ] || printf '%s\n' "$out"
if [ "$rc" -ne 0 ]; then
  echo "no-retries: could not read the tree" >&2
  exit 2
fi
[ -z "$out" ] || exit 1
exit 0
