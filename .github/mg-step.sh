#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# One tools/mg job as a step of a hosted job (docs/process/tiers.md, "Hosted
# checks"). Run from the checkout root, in GitHub Actions only.
#
#   mg-step.sh <title> <job> [args...]
#
# Runs `tools/mg <job> <args>` as the other hosted jobs do (sudo, the shared
# dev image from MEGURI_CI_DEV_IMAGE, the checkout as a safe directory; with
# MG_STEP_SUDO=no, as the runner user, for macOS runners),
# copies its receipt to $RUNNER_TEMP/receipts/<job>.json, writes a section
# named <title> to the step summary with the receipt's message and failures,
# and fails unless the receipt is a pass of exactly that job and those
# arguments on $GITHUB_SHA, from a clean tree.

set -euo pipefail

title="${1:?usage: mg-step.sh <title> <job> [args...]}"
job="${2:?usage: mg-step.sh <title> <job> [args...]}"
shift 2

rc=0
if [[ "${MG_STEP_SUDO:-yes}" == no ]]; then
  # macOS runners: the job runs as the runner user, which owns the checkout.
  receipt="$("$GITHUB_WORKSPACE/tools/mg" "$job" "$@")" || rc=$?
else
  receipt="$(sudo -n env MEGURI_CI_DEV_IMAGE="${MEGURI_CI_DEV_IMAGE:-}" \
    GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=safe.directory GIT_CONFIG_VALUE_0="$GITHUB_WORKSPACE" \
    "$GITHUB_WORKSPACE/tools/mg" "$job" "$@")" || rc=$?
fi

mkdir -p "$RUNNER_TEMP/receipts"
artifact="$RUNNER_TEMP/receipts/$job.json"
prefix="$GITHUB_WORKSPACE/build/mg/$job/"
suffix="${receipt#"$prefix"}"
if [[ "$receipt" == "$prefix"* && "$suffix" =~ ^[0-9]{8}T[0-9]{6}Z-[0-9]+/result\.json$ ]] &&
   sudo -n test -f "$receipt"; then
  # shellcheck disable=SC2024 # root reads the receipt; the runner writes the copy
  sudo -n cat -- "$receipt" >"$artifact.tmp"
  mv -- "$artifact.tmp" "$artifact"
fi

if [[ ! -f "$artifact" ]]; then
  # shellcheck disable=SC2016 # the backticks are Markdown
  printf '### %s: no receipt\n\n`tools/mg %s` exited %s without a receipt; see the log.\n\n' "$title" "$job" "$rc" >>"$GITHUB_STEP_SUMMARY"
  echo "::error::$title: tools/mg $job left no receipt (exit $rc)"
  exit 1
fi

{
  jq -r --arg title "$title" '"### \($title): \(.status)\n\n\(.message // "")\n"' "$artifact"
  jq -r '.failures[:30][]? | "- `\(.id)`: \((.message // "") | split("\n")[0] | .[0:300])"' "$artifact"
  jq -r 'if (.failures | length) > 30 then "- and \((.failures | length) - 30) more (see the uploaded receipt)" else empty end' "$artifact"
  echo
} >>"$GITHUB_STEP_SUMMARY"

want='[]'
(($# == 0)) || want="$(printf '%s\n' "$@" | jq -Rnc '[inputs]')"
if ((rc != 0)) || ! jq -e --arg sha "$GITHUB_SHA" --arg job "$job" --argjson want "$want" '
    .status == "pass" and .commit.sha == $sha and .commit.dirty == false
    and .job == $job and .args == $want' "$artifact" >/dev/null; then
  echo "::error::$title: $(jq -r '.message // "failed"' "$artifact")"
  exit 1
fi
