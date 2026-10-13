#!/bin/bash
# UI lint path rules (U9, R1994, KTD20): which files the gate lints, and what
# is left of SwiftLint's findings once the per-file allowlist is applied.
# `tools/mg lint-ui` runs SwiftLint with app/.swiftlint.yml in Docker and this
# script on the host (bash 3.2, BSD tools, jq).
#
# usage:
#   path-rules.sh files <root>
#       Swift files of the app's UI code under <root>/app, relative to <root>,
#       NUL-separated and sorted.
#   path-rules.sh rules <config>
#       Public rule ids: the config's custom rules (a key `id--variant` reports
#       as `id`) plus this script's own.
#   path-rules.sh findings <root> <config> <swiftlint json> <path prefix>
#       Findings left after the allowlist, as JSON lines
#       {rule, file, line, column, message}; <path prefix> is what SwiftLint
#       put before each <root>-relative path (e.g. `/work/` or `/work/<root>/`),
#       and findings in files outside it are left out.
#
# <root> is laid out like the repository: the real checkout, or a fixture case
# under tools/lint-ui/tests/fixtures/. Its allowlist is
# <root>/tools/lint-ui/allowlist.tsv: `rule<TAB>path<TAB>reason`, where path is
# a file or a directory ending in `/`, relative to <root>. An entry needs a
# known rule, an existing path and a reason, or the gate fails on it. Inline
# `swiftlint:` commands are never honoured: they fail as `inline-disable`, so
# the allowlist is the only exemption.

set -u

# Directories under app/ that hold no app UI code: tests and the Rust bridge.
# Everything else under app/ is linted, so new UI code needs no registration.
scoped_files() {
  local root="$1"
  [ -d "$root/app" ] || return 0
  (
    cd "$root" || exit 2
    find app \( -path app/MeguriUITests -o -path app/MeguriSnapshotTests \
      -o -path app/Packages/MeguriCore -o -path 'app/Packages/*/Tests' \
      -o -name .build -o -name .swiftpm -o -name DerivedData -o -path app/build \) -prune \
      -o -type f -name '*.swift' ! -name Package.swift -print0 | sort -z
  )
}

# Rule ids the script itself reports.
OWN_RULES="inline-disable allowlist-invalid"

config_rules() {
  # Keys indented by two spaces under `custom_rules:` (keep that layout).
  awk '
    /^custom_rules:/ { inside = 1; next }
    /^[^[:space:]#]/ { inside = 0 }
    inside && /^  [A-Za-z0-9_-]+:[[:space:]]*$/ { key = $1; sub(/:$/, "", key); sub(/--.*/, "", key); print key }
  ' "$1" | sort -u
}

# allowlist_entries <root> <rules>: prints `ok<TAB>rule<TAB>path` for a valid
# entry and `bad<TAB><JSON finding>` for an invalid one.
allowlist_entries() {
  local root="$1" rules="$2" file="$1/tools/lint-ui/allowlist.tsv" n=0 line rule path reason rest
  [ -f "$file" ] || return 0
  while IFS= read -r line || [ -n "$line" ]; do
    n=$((n + 1))
    case "$line" in "" | "#"*) continue ;; esac
    rule="${line%%	*}"
    rest="${line#*	}"
    path="${rest%%	*}"
    reason="${rest#*	}"
    local problem=""
    if [ "$rest" = "$line" ] || [ "$reason" = "$rest" ]; then
      problem="needs three tab-separated fields: rule, path, reason"
    elif ! printf '%s\n' "$rules" | grep -qxF -- "$rule" || [ "$rule" = inline-disable ] || [ "$rule" = allowlist-invalid ]; then
      problem="unknown or non-exemptible rule '$rule'"
    elif [ -z "$path" ] || [ ! -e "$root/$path" ]; then
      problem="no file or directory '$path'"
    elif [ -z "$(printf '%s' "$reason" | tr -d '[:space:]')" ]; then
      problem="entry for '$path' has no reason"
    fi
    if [ -n "$problem" ]; then
      printf 'bad\t%s\n' "$(jq -nc --arg f tools/lint-ui/allowlist.tsv --argjson l "$n" --arg m "$problem" \
        '{rule: "allowlist-invalid", file: $f, line: $l, column: 1, message: $m}')"
    else
      printf 'ok\t%s\t%s\n' "$rule" "$path"
    fi
  done <"$file"
}

# Inline SwiftLint commands in the linted files.
inline_disables() {
  local root="$1" f
  scoped_files "$root" | while IFS= read -r -d '' f; do
    grep -n 'swiftlint:' "$root/$f" 2>/dev/null | while IFS=: read -r lineno _; do
      jq -nc --arg f "$f" --argjson l "$lineno" \
        '{rule: "inline-disable", file: $f, line: $l, column: 1,
          message: "inline swiftlint: commands are not honoured; fix the code or add an allowlist entry with a reason (tools/lint-ui/allowlist.tsv)"}'
    done
  done
}

findings() {
  local root="$1" config="$2" json="$3" prefix="$4" rules listed entries
  rules="$( (config_rules "$config" && printf '%s\n' $OWN_RULES) | sort -u)"
  listed="$(allowlist_entries "$root" "$rules")"
  printf '%s\n' "$listed" | sed -n 's/^bad	//p'
  entries="$(printf '%s\n' "$listed" | sed -n 's/^ok	//p')"
  jq -c --arg prefix "$prefix" --arg entries "$entries" '
    ($entries | split("\n") | map(select(length > 0) | split("\t") | {rule: .[0], path: .[1]})) as $allow
    | .[]
    | select(.file | startswith($prefix))
    | {rule: (.rule_id | sub("--.*$"; "")),
       file: (.file | .[($prefix | length):]),
       line: (.line // 0), column: (.character // 1), message: .reason}
    | . as $f
    | select([$allow[] | .path as $p | select(.rule == $f.rule
        and ($p == $f.file or (($p | endswith("/")) and ($f.file | startswith($p)))))] | length == 0)
  ' "$json" || return 1
  inline_disables "$root"
}

case "${1:-}" in
  files) scoped_files "${2:?usage: path-rules.sh files <root>}" ;;
  rules) (config_rules "${2:?usage: path-rules.sh rules <config>}" && printf '%s\n' $OWN_RULES) | sort -u ;;
  findings)
    [ "$#" -eq 5 ] || { echo "usage: path-rules.sh findings <root> <config> <swiftlint json> <path prefix>" >&2 && exit 2; }
    findings "$2" "$3" "$4" "$5"
    ;;
  *)
    echo "usage: path-rules.sh files <root> | rules <config> | findings <root> <config> <swiftlint json> <path prefix>" >&2
    exit 2
    ;;
esac
