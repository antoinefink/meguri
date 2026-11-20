#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# The shared dev image of the hosted checks (docs/process/tiers.md, "Hosted
# checks"). Run from the checkout root, in GitHub Actions only.
#
#   dev-image.sh hash     the content hash of the image inputs (the tag)
#   dev-image.sh ensure   "Prepare dev image": reuse the shared image of this hash,
#                         or build it once and push it. Writes hash= and ref= to
#                         $GITHUB_OUTPUT; an empty ref means no shared image.
#                         Env: TOKEN, CAN_PUSH, COLD and TRUSTED (true or false).
#   dev-image.sh pull     pull REF, which must be one of this checkout's two refs,
#                         and name it in $GITHUB_ENV (MEGURI_CI_DEV_IMAGE), so
#                         tools/mg uses it instead of building. Without REF,
#                         pull the master image of this hash. Env: TOKEN, REF.
#                         Writes used=yes to $GITHUB_OUTPUT on success. On any
#                         failure, tools/mg builds the image itself.
#
# Two repositories: ghcr.io/<owner>/meguri-dev holds images built by master
# pushes, the schedule and manual runs on master (TRUSTED); meguri-dev-pr
# holds images built for pull requests. Master never reads meguri-dev-pr, so
# an unreviewed change cannot put its image into master's checks. A pull
# request reuses master's image first.
#
# The image build reads only these files: docker/dev.Dockerfile (its bases
# are pinned by digest), its .dockerignore (it lets in only
# core/rust-toolchain.toml), that toolchain file, docker/compose.yml (the
# build section; the image is built through it, as tools/mg does) and this
# script (the build command). The source under test is mounted at run time
# and is not an input. The tag is content addressed: a commit that changes
# none of these files reuses the image on every branch, and a job never takes
# an image whose label names another hash. The Dockerfile's apt packages are
# not pinned, so a shared image keeps the packages of its first build until
# an input changes; a cold run builds them afresh.

set -euo pipefail

inputs=(docker/dev.Dockerfile docker/dev.Dockerfile.dockerignore core/rust-toolchain.toml docker/compose.yml .github/dev-image.sh)
label=org.meguri.dev-image.inputs

hash() {
  {
    echo "meguri-dev target=dev platform=$(uname -m)"
    for f in "${inputs[@]}"; do
      printf '%s\0' "$f"
      sha256sum <"$f"
    done
  } | sha256sum | cut -c1-32
}

# ref_of <hash> [pr]: the master or the pull request image of a hash.
ref_of() { printf 'ghcr.io/%s/meguri-dev%s:%s\n' "${GITHUB_REPOSITORY_OWNER,,}" "${2:+-pr}" "$1"; }

# build <hash> <ref>: build the dev service through compose, as tools/mg
# does, then add the labels in a layerless image on top.
build() {
  local project=meguri-ci-image
  BUILDX_NO_DEFAULT_ATTESTATIONS=1 docker compose -p "$project" -f docker/compose.yml --progress plain build dev &&
    printf 'FROM %s\nLABEL %s=%s org.opencontainers.image.source=https://github.com/%s\n' \
      "$project-dev" "$label" "$1" "$GITHUB_REPOSITORY" |
    docker build -q -t "$2" - >/dev/null
}

login() { printf '%s' "$TOKEN" | docker login ghcr.io -u "$GITHUB_ACTOR" --password-stdin >/dev/null; }

logout() { docker logout ghcr.io >/dev/null 2>&1 || true; }

ensure() {
  local h ref
  h="$(hash)"
  echo "hash=$h" >>"$GITHUB_OUTPUT"
  report() {
    echo "ref=$1" >>"$GITHUB_OUTPUT"
    printf '### Dev image\n\n%s\n' "$2" >>"$GITHUB_STEP_SUMMARY"
    echo "$2"
  }
  if [[ "$COLD" == true ]]; then
    report "" "Cold run: every job builds the image itself."
    return 0
  fi
  local master pr
  master="$(ref_of "$h")"
  pr="$(ref_of "$h" pr)"
  if ! login; then
    echo "::warning::Cannot log in to ghcr.io; every job builds the dev image itself."
    report "" "No shared image (registry login failed): every job builds it."
    return 0
  fi
  trap logout EXIT
  ref="$pr"
  [[ "$TRUSTED" != true ]] || ref="$master"
  if docker manifest inspect "$master" >/dev/null 2>&1; then
    report "$master" "Reused \`$master\`."
  elif [[ "$ref" == "$pr" ]] && docker manifest inspect "$pr" >/dev/null 2>&1; then
    report "$pr" "Reused \`$pr\`."
  elif [[ "$CAN_PUSH" == true ]] && build "$h" "$ref" && docker push -q "$ref"; then
    report "$ref" "Built and pushed \`$ref\`."
  else
    echo "::warning::No shared dev image for $h; every job builds it itself."
    report "" "No shared image for \`$h\`: every job builds it."
  fi
}

pull() {
  local h ref got
  h="$(hash)"
  ref="${REF:-$(ref_of "$h")}"
  if [[ "$ref" != "$(ref_of "$h")" && "$ref" != "$(ref_of "$h" pr)" ]]; then
    echo "::warning::The shared image $ref is not one of this checkout's; tools/mg builds the dev image."
    return 0
  fi
  if login && docker pull -q "$ref" &&
     got="$(docker image inspect -f "{{ index .Config.Labels \"$label\" }}" "$ref")" && [[ "$got" == "$h" ]]; then
    echo "MEGURI_CI_DEV_IMAGE=$ref" >>"$GITHUB_ENV"
    echo "used=yes" >>"$GITHUB_OUTPUT"
    echo "Using the shared dev image $ref."
  else
    echo "::warning::Could not use $ref; tools/mg builds the dev image."
  fi
  logout
}

case "${1:-}" in
  hash) hash ;;
  ensure) ensure ;;
  pull) pull ;;
  *)
    echo "usage: .github/dev-image.sh hash | ensure | pull" >&2
    exit 2
    ;;
esac
