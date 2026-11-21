#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# The Rust caches of the hosted checks (docs/process/tiers.md, "Hosted
# checks"). Run from the checkout root, in GitHub Actions only.
#
#   rust-cache.sh prune <name> <cap MB>
#                         before a save: keep only the cargo registry and the
#                         third-party crates under $RUNNER_TEMP. Local packages
#                         (workspace and patched path crates) are rebuilt from
#                         source, so their artifacts never enter a cache.
#                         Writes save=yes, or save=no over the cap, to
#                         $GITHUB_OUTPUT. The caps (on the uncompressed size;
#                         saved entries compress to about a third) are chosen
#                         so that an old and a new entry of every cache (after
#                         a Cargo.lock or image change) fit in GitHub's 10 GB.
#                         They bound each entry only: when the budget is
#                         full, GitHub evicts the least recently used entries,
#                         which are then normally the replaced ones.

set -euo pipefail

prune() {
  local name="$1" cap="$2" target="$RUNNER_TEMP/cargo-target" dir p u size=0
  local -a pkgs paths=()
  echo "Cargo target before pruning: $(sudo du -sm "$target" | cut -f1) MB"
  mapfile -t pkgs < <(find core -name Cargo.toml -not -path '*/target/*' -print0 |
    xargs -0 awk 'FNR == 1 { p = 0 } /^\[package\]/ { p = 1; next } /^\[/ { p = 0 }
      p && /^name[[:space:]]*=/ { gsub(/[" ]/, ""); sub(/^name=/, ""); print }' | sort -u)
  ((${#pkgs[@]} > 0)) || { echo "::error::No local packages found"; exit 1; }
  for dir in "$target"/*/; do
    [[ -d "$dir/deps" ]] || continue
    sudo rm -rf -- "$dir/incremental"
    for p in "${pkgs[@]}"; do
      u="${p//-/_}"
      sudo rm -rf -- "$dir"deps/"$u"-* "$dir"deps/lib"$u"-* "$dir".fingerprint/"$p"-* "$dir"build/"$p"-*
    done
    # Test and other executables of local packages.
    sudo find "$dir/deps" -maxdepth 1 -type f -perm -u+x ! -name '*.*' -delete
    # The profile root holds only the final outputs cargo copies out of deps/
    # for the packages it was asked to build (layering-check, meguri-sync),
    # all local, and its lock file.
    sudo find "$dir" -maxdepth 1 -type f -delete
  done
  sudo rm -rf -- "$target/nextest" "$target/tmp"
  sudo chown -R "$(id -u):$(id -g)" "$RUNNER_TEMP/cargo-home" "$target"
  # Only the paths that exist: du fails on a missing one (no git
  # dependencies, say), and that would fail the step.
  for p in "$RUNNER_TEMP/cargo-home/registry/index" "$RUNNER_TEMP/cargo-home/registry/cache" "$target"; do
    [[ ! -e "$p" ]] || paths+=("$p")
  done
  ((${#paths[@]} == 0)) || size="$(du -smc -- "${paths[@]}" | tail -n 1 | cut -f1)"
  echo "$name: $size MB"
  if ((size > cap)); then
    echo "::warning::$name is $size MB, over its $cap MB cap; not saving it."
    echo "save=no" >>"$GITHUB_OUTPUT"
  else
    echo "save=yes" >>"$GITHUB_OUTPUT"
  fi
}

case "${1:-}" in
  prune)
    [[ $# -eq 3 && "$3" =~ ^[1-9][0-9]*$ ]] || { echo "usage: .github/rust-cache.sh prune <name> <cap MB>" >&2; exit 2; }
    prune "$2" "$3"
    ;;
  *) echo "usage: .github/rust-cache.sh prune <name> <cap MB>" >&2; exit 2 ;;
esac
