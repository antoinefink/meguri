#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# The shared dev image of the hosted checks (docs/process/tiers.md, "Hosted
# checks"). Run from the checkout root, in GitHub Actions only.
#
#   dev-image.sh hash     the content hash of the image inputs (the tag)
#   dev-image.sh ensure   "Prepare dev image": reuse ghcr.io/<owner>/meguri-dev:<hash>,
#                         or build and push it. Writes hash= and ref= to
#                         $GITHUB_OUTPUT; an empty ref means no shared image.
#                         Env: TOKEN, CAN_PUSH and COLD (true or false).
#   dev-image.sh pull     pull the image of this checkout's hash and name it in
#                         $GITHUB_ENV (MEGURI_CI_DEV_IMAGE), so tools/mg uses it
#                         instead of building. Env: TOKEN, and REF when given
#                         (it must be this checkout's ref). Writes used=yes to
#                         $GITHUB_OUTPUT on success. On any failure, tools/mg
#                         builds the image itself, as without CI.
#
# The image build reads only these files: docker/dev.Dockerfile (its bases
# are pinned by digest), its .dockerignore (it lets in only
# core/rust-toolchain.toml), that toolchain file, docker/compose.yml (build
# context and target) and this script (the build command). The source under
# test is mounted at run time and is not an input. The tag is content
# addressed: a commit that changes none of these files reuses the image on
# every branch, and a job never takes an image whose label names another hash.

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

ref_of() { printf 'ghcr.io/%s/meguri-dev:%s\n' "${GITHUB_REPOSITORY_OWNER,,}" "$1"; }

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
  ref="$(ref_of "$h")"
  if ! login; then
    echo "::warning::Cannot log in to ghcr.io; every job builds the dev image itself."
    report "" "No shared image (registry login failed): every job builds it."
    return 0
  fi
  trap logout EXIT
  if docker manifest inspect "$ref" >/dev/null 2>&1; then
    report "$ref" "Reused \`$ref\`."
  elif [[ "$CAN_PUSH" == true ]] &&
       BUILDX_NO_DEFAULT_ATTESTATIONS=1 docker build --progress plain -f docker/dev.Dockerfile --target dev \
         --label "org.opencontainers.image.source=https://github.com/$GITHUB_REPOSITORY" --label "$label=$h" \
         -t "$ref" . &&
       docker push "$ref"; then
    report "$ref" "Built and pushed \`$ref\`."
  else
    echo "::warning::No shared dev image for $h; every job builds it itself."
    report "" "No shared image for \`$h\`: every job builds it."
  fi
}

pull() {
  local h ref got
  h="$(hash)"
  ref="$(ref_of "$h")"
  if [[ -n "${REF:-}" && "$REF" != "$ref" ]]; then
    echo "::warning::The shared image $REF is not this checkout's $ref; tools/mg builds the dev image."
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
