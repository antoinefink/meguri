# Docker image jobs. Sourced by tools/mg; orchestrates Docker from the host.

mg_job dev-image host mg_dev_image \
  "Build the dev and fuzz images (docker/dev.Dockerfile); record image ids and layer-cache reuse"

# mg__image_build_stats <buildkit plain log>: instruction steps look like
# `#7 [dev 3/5] RUN ...`, and a reused step is followed by `#7 CACHED`.
mg__image_build_stats() {
  awk '
    /^#[0-9]+ \[[^]]+ [0-9]+\/[0-9]+\] / { if ($0 !~ /\] FROM /) steps[$1] = 1 }
    /^#[0-9]+ CACHED$/ { cached[$1] = 1 }
    END {
      n = 0; c = 0
      for (id in steps) { n++; if (id in cached) c++ }
      printf "{\"steps\": %d, \"cached\": %d, \"executed\": %d}\n", n, c, n - c
    }' "$1"
}

mg_dev_image() {
  if ! docker info >/dev/null 2>&1; then
    mg_failure docker-unreachable "Docker is not reachable; start Docker Desktop" "tools/mg doctor"
    mg_status error
    return 0
  fi
  local project rc devlog="$MG_RUN_DIR/build-dev.log" fuzzlog="$MG_RUN_DIR/build-fuzz.log"
  project="$(mg_compose_project)"

  mg_compose --progress plain build dev 2>&1 | tee "$devlog"
  rc=${PIPESTATUS[0]}
  mg_artifact log "$devlog" "dev image build"
  if [ "$rc" -ne 0 ]; then
    mg_failure dev-build "building the dev image failed; see build-dev.log"
    mg_status fail
    return 0
  fi

  BUILDX_NO_DEFAULT_ATTESTATIONS=1 docker build --progress plain -f docker/dev.Dockerfile \
    --target fuzz -t "$project-fuzz" "$MG_ROOT" 2>&1 | tee "$fuzzlog"
  rc=${PIPESTATUS[0]}
  mg_artifact log "$fuzzlog" "fuzz image build"
  if [ "$rc" -ne 0 ]; then
    mg_failure fuzz-build "building the fuzz image failed; see build-fuzz.log"
    mg_status fail
    return 0
  fi

  local devid fuzzid devrc fuzzrc images
  devid="$(docker image inspect "$project-dev" --format '{{.Id}}')"
  devrc=$?
  fuzzid="$(docker image inspect "$project-fuzz" --format '{{.Id}}')"
  fuzzrc=$?
  if [ "$devrc" -ne 0 ] || [ "$fuzzrc" -ne 0 ] || [ -z "$devid" ] || [ -z "$fuzzid" ]; then
    mg_failure image-inspect "inspecting the built images failed or returned an empty image ID; see log.txt"
    mg_status error
    return 0
  fi
  images="$(jq -nc \
    --arg dtag "$project-dev" --arg did "$devid" --argjson dstats "$(mg__image_build_stats "$devlog")" \
    --arg ftag "$project-fuzz" --arg fid "$fuzzid" --argjson fstats "$(mg__image_build_stats "$fuzzlog")" \
    '{dev: ({tag: $dtag, id: $did} + $dstats), fuzz: ({tag: $ftag, id: $fid} + $fstats)}')"
  mg_detail images "$images"
  mg_message "$(jq -r '"dev-image: dev \(.dev.id[7:19]) (\(.dev.cached)/\(.dev.steps) steps cached), fuzz \(.fuzz.id[7:19]) (\(.fuzz.cached)/\(.fuzz.steps) steps cached)"' <<<"$images")"
  mg_status pass
}
