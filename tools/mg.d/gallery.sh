# The review gallery (U10, R1988, R1989). Sourced by tools/mg; host jobs.
#
# `tools/mg gallery` builds a static page (tools/scripts/gallery, in
# image-tools): every screen's key variants as old | new | diff, the
# screens' open critic issues from the status ledger, variant picks, and a
# pending-approvals batch from branches whose snapshot run the baseline
# guard escalated (--pending <worktree>). `tools/mg gallery serve` serves the
# latest page on 127.0.0.1 and records Antoine's decisions: every decision
# in docs/design/reviews/<date>.json, an approval also in
# docs/design/approved-baselines.json, a pick in docs/design/decisions.md.
# Approvals of a pending branch are committed in that branch's worktree
# with an `Approved-by: antoine` trailer once the server stops (U19's
# approved_by: antoine rule; git trailer keys take no underscore); decisions on
# this checkout are left for its owner to commit. Agents never post
# decisions: only Antoine reviews.

mg_job gallery host mg_gallery \
  "Review gallery (R1988, R1989): [--base <rev>] [--pending <worktree>]... [--picks <file>] [--components] builds the page (old | new | diff of every screen's key variants, open critic issues, picks); serve [<gallery run>] [--port N] [--seconds N] serves it on 127.0.0.1 and records Antoine's decisions"

MG_GALLERY_KEY_VARIANT='/(light|dark)-(standard|ax5)-(iphoneLarge|ipadRegular|macDefault)\.png$'

mg_gallery__usage() {
  mg_failure usage "gallery: $1" "tools/mg --list"
  mg_message "gallery: $1"
  mg_status error
}

mg_gallery() {
  if [ "${1:-}" = serve ]; then
    shift
    mg_gallery__serve "$@"
    return 0
  fi
  local base="" picks="" components=false pending=()
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --base) [ "$#" -ge 2 ] || { mg_gallery__usage "--base needs a revision"; return 0; }; base="$2" && shift 2 ;;
      --pending)
        [ "$#" -ge 2 ] && [ -d "$2" ] || { mg_gallery__usage "--pending needs a worktree path"; return 0; }
        pending[${#pending[@]}]="$(cd "$2" && pwd -P)" && shift 2
        ;;
      --picks) [ "$#" -ge 2 ] && [ -f "$2" ] || { mg_gallery__usage "--picks needs a picks file"; return 0; }; picks="$2" && shift 2 ;;
      --components) components=true && shift ;;
      *) mg_gallery__usage "unknown argument '$1'"; return 0 ;;
    esac
  done
  if [ -z "$base" ]; then
    # The commit the last review looked at.
    local last
    last="$(ls docs/design/reviews/*.json 2>/dev/null | sort | tail -n 1)"
    [ -z "$last" ] || base="$(jq -r '.commit // empty' "$last")"
  fi
  if [ -n "$base" ] && ! git -C "$MG_ROOT" rev-parse --verify -q "$base^{commit}" >/dev/null; then
    mg_gallery__usage "--base: no commit '$base'"
    return 0
  fi
  mg_detail base "$(jq -n --arg b "$base" 'if $b == "" then null else $b end')"

  local work="$MG_RUN_DIR/inputs" items="$MG_RUN_DIR/items.json" mounts="$MG_RUN_DIR/mounts.json" f n=0 old
  mkdir -p "$work"
  echo "[]" >"$items"
  echo "[]" >"$mounts"
  # This checkout's baselines in their key variants.
  while IFS= read -r f; do
    f="${f#./}"
    case "$f" in */designsystem.* | */matrix.* | */cardface.*) [ "$components" = true ] || continue ;; esac
    n=$((n + 1))
    old="$(mg_gallery__old "$MG_ROOT" "$f" "$base" "$work/old-$n.png")"
    mg_gallery__add "$items" "$f" "$MG_ROOT/$f" "$old" /work "" false "$MG_ROOT"
  done < <(cd "$MG_ROOT" && find app/Snapshots -name '*.png' -type f 2>/dev/null | grep -E "$MG_GALLERY_KEY_VARIANT" | sort)
  # Pending approvals: the baselines a branch's guard escalated.
  local wt i=0 result branch path
  for wt in ${pending[0]+"${pending[@]}"}; do
    i=$((i + 1))
    result="$wt/build/mg/snapshots/latest/result.json"
    [ -f "$result" ] || { mg_failure "pending/$wt" "no snapshots run in $wt (tools/mg snapshots there first)"; continue; }
    branch="$(git -C "$wt" branch --show-current)"
    jq -n --argjson m "$(cat "$mounts")" --arg w "$wt" --arg c "/pending/$i" '$m + [{worktree: $w, container: $c}]' >"$mounts.tmp" && mv "$mounts.tmp" "$mounts"
    while IFS= read -r path; do
      n=$((n + 1))
      mkdir -p "$work/pending-$i/$(dirname "$path")"
      cp "$wt/$path" "$work/pending-$i/$path" 2>/dev/null || continue
      old="$(mg_gallery__old "$wt" "$path" "" "$work/old-$n.png")"
      mg_gallery__add "$items" "$path" "$work/pending-$i/$path" "$old" "/pending/$i" "$branch" true "$wt"
    done < <(jq -r '.details.guard.needs_human_review[]? // empty' "$result")
  done
  # Picks: images copied in, so the container sees them.
  local picks_in="" k=0 img
  if [ -n "$picks" ]; then
    picks_in="$MG_RUN_DIR/picks.json"
    mkdir -p "$work/picks"
    while IFS= read -r img; do
      case "$img" in /*) ;; *) img="$MG_ROOT/$img" ;; esac
      cp "$img" "$work/picks/$k.png" || { mg_failure picks "no pick image $img"; mg_status error; return 0; }
      k=$((k + 1))
    done < <(jq -r '.[].variants[].image' "$picks")
    jq --arg dir "$(mg_container_path "$work/picks")" '
      reduce .[] as $p ({out: [], n: 0};
        .n as $n0 |
        .out += [$p | .checkout = "/work" | .variants = [range(0; ($p.variants | length)) as $j | $p.variants[$j] | .image = "\($dir)/\($n0 + $j).png"]] |
        .n += ($p.variants | length)) | .out' "$picks" >"$picks_in"
  fi

  local site="$MG_RUN_DIR/site" build_args=()
  [ -z "$picks_in" ] || build_args=(--picks "$(mg_container_path "$picks_in")")
  if ! mg_image_tools python3 tools/scripts/gallery/gallery.py build --items "$(mg_container_path "$items")" \
    --out "$(mg_container_path "$site")" --ledger docs/design/screen-status.yaml \
    --commit "$(git -C "$MG_ROOT" rev-parse --verify -q HEAD)" ${build_args[0]+"${build_args[@]}"} >&2; then
    mg_failure build "could not build the gallery; see log.txt"
    mg_status error
    return 0
  fi
  mg_artifact page "$site/index.html" "review gallery"
  mg_detail gallery "$(jq -c '{items: (.items | length), changed: ([.items[] | select(.change != "unchanged")] | length), pending: ([.items[] | select(.pending)] | length), picks: (.picks | length), screens_with_issues: (.issues | keys)}' "$site/gallery.json")"
  mg_count total "$(jq '.items | length' "$site/gallery.json")"
  mg_message "gallery: $(jq -r '"\(.items | length) images, \([.items[] | select(.change != "unchanged")] | length) changed or new, \([.items[] | select(.pending)] | length) pending approval"' "$site/gallery.json"); open $site/index.html, or tools/mg gallery serve to record decisions"
  [ -s "$MG_STATE/failures.jsonl" ] && mg_status warn || mg_status pass
}

# mg_gallery__old <checkout> <path> <base rev> <out>: the image to compare
# with: the approved one (approved-baselines.json; after an OS rebaseline,
# the approved image of the same model's same cell under the old build),
# else the file at the base revision. Prints its path, or nothing.
mg_gallery__old() {
  local checkout="$1" path="$2" base="$3" out="$4" blob
  blob="$(jq -r --arg p "$path" '
    def cell: capture("^app/Snapshots/(?<key>[^/]+)/(?<cell>.+)$") | {model: (.key | sub("-[^-]+$"; "")), cell};
    first((.baselines[]? | select(.path == $p)), (.baselines[]? | select((.path | cell) == ($p | cell))) | .blob) // empty' \
    "$checkout/docs/design/approved-baselines.json" 2>/dev/null)"
  if [ -n "$blob" ] && git -C "$checkout" cat-file blob "$blob" >"$out" 2>/dev/null; then
    printf '%s\n' "$out"
  elif [ -n "$base" ] && git -C "$checkout" show "$base:$path" >"$out" 2>/dev/null; then
    printf '%s\n' "$out"
  else
    rm -f "$out"
  fi
}

# mg_gallery__committed <checkout> <path>: the blob HEAD holds at <path>,
# when the file there is that blob and git can read it back (the guard reads
# approved images by blob id); nothing otherwise. Only such an image can be
# approved (gallery.py decide).
mg_gallery__committed() {
  local blob
  blob="$(git -C "$1" rev-parse -q --verify "HEAD:$2" 2>/dev/null)" || return 0
  [ "$(git -C "$1" hash-object -- "$1/$2" 2>/dev/null)" = "$blob" ] || return 0
  git -C "$1" cat-file -e "$blob" 2>/dev/null || return 0
  printf '%s\n' "$blob"
}

# mg_gallery__add <items> <baseline path> <new> <old|""> <checkout in container> <branch> <pending> <checkout on the host>
mg_gallery__add() {
  local items="$1" path="$2" new="$3" old="$4" checkout="$5" branch="$6" pending="$7" host="$8" rest subject state cell signature committed
  rest="${path#app/Snapshots/*/}"
  subject="${rest%%/*}"
  rest="${rest#*/}"
  state="${rest%%/*}"
  cell="$(basename "$rest" .png)"
  signature="$(jq --arg s "$subject" 'any(.baselines[]?; .screen == $s and .signature == true)' "$MG_ROOT/docs/design/approved-baselines.json" 2>/dev/null || echo false)"
  [ "$(mg_gallery__signature "$subject")" = true ] && signature=true
  committed="$(mg_gallery__committed "$host" "$path")"
  jq --arg id "$( [ "$pending" = true ] && echo "$branch:")$subject/$state/$cell" --arg s "$subject" --arg st "$state" --arg c "$cell" \
    --arg b "$path" --arg n "$(mg_container_path "$new")" --arg o "$( [ -n "$old" ] && mg_container_path "$old")" \
    --arg co "$checkout" --arg br "$branch" --argjson p "$pending" --argjson sig "${signature:-false}" --arg cb "$committed" \
    '. + [{id: $id, screen: $s, state: $st, variant: $c, platform: ($c | capture("-(?<d>iphone|ipad|mac)") | .d),
      baseline: $b, new: $n, old: (if $o == "" then null else $o end), checkout: $co,
      branch: (if $br == "" then null else $br end), pending: $p, signature: $sig,
      committed_blob: (if $cb == "" then null else $cb end)}]' "$items" >"$items.tmp" && mv "$items.tmp" "$items"
}

# mg_gallery__signature <screen id>: whether the inventory marks it a
# signature screen (its entries keep `- id:` and `signature:` on lines of
# their own; tools/mg design-docs validates the file).
mg_gallery__signature() {
  awk -v id="$1" '
    $1 == "-" && $2 == "id:" { current = $3 }
    $1 == "signature:" && current == id { print $2; exit }' "$MG_ROOT/docs/design/screen-inventory.yaml"
}

# tools/mg gallery serve [<gallery run>] [--port N] [--seconds N]
mg_gallery__serve() {
  local run="" port=8790 seconds="" dir site
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --port) [ "$#" -ge 2 ] && printf '%s' "$2" | grep -Eq '^[0-9]+$' || { mg_gallery__usage "--port takes a port number"; return 0; }; port="$2" && shift 2 ;;
      --seconds) [ "$#" -ge 2 ] && printf '%s' "$2" | grep -Eq '^[0-9]+$' || { mg_gallery__usage "--seconds takes whole seconds"; return 0; }; seconds="$2" && shift 2 ;;
      -*) mg_gallery__usage "unknown option '$1'"; return 0 ;;
      *) run="$1" && shift ;;
    esac
  done
  if [ -n "$run" ]; then
    dir="$MG_ROOT/build/mg/gallery/$run"
  else
    dir="$(ls -dt "$MG_ROOT"/build/mg/gallery/*/site 2>/dev/null | head -n 1)"
    dir="${dir%/site}"
  fi
  site="$dir/site"
  [ -n "$dir" ] && [ -f "$site/gallery.json" ] || { mg_gallery__usage "no built gallery${run:+ $run}; run tools/mg gallery first"; return 0; }
  local mounts=() m
  while IFS= read -r m; do
    mounts=(${mounts[0]+"${mounts[@]}"} -v "$(jq -r .worktree <<<"$m"):$(jq -r .container <<<"$m")")
  done < <(jq -c '.[]' "$dir/mounts.json" 2>/dev/null)
  mg_detail gallery "$(jq -n --arg d "$dir" --arg u "http://127.0.0.1:$port/" '{run: $d, url: $u}')"
  echo "gallery: serving $site on http://127.0.0.1:$port/ (stop with Ctrl-C$([ -z "$seconds" ] || echo ", or after $seconds s"))" >&2
  mg_compose --progress quiet build image-tools >&2 || { mg_failure image "could not build the image-tools image"; mg_status error; return 0; }
  local name="mg-gallery-$MG_RUN_ID"
  mg_on_exit "docker rm -f '$name' >/dev/null 2>&1"
  mg_compose run --rm -T --no-deps --name "$name" --label "meguri.mg.run-id=$MG_RUN_ID" -p "127.0.0.1:$port:8790" \
    ${mounts[0]+"${mounts[@]}"} image-tools python3 tools/scripts/gallery/gallery.py serve --gallery "$(mg_container_path "$site")" --port 8790 \
    </dev/null >&2 &
  local pid=$! waited=0
  while kill -0 "$pid" 2>/dev/null; do
    if [ -n "$seconds" ] && [ "$waited" -ge "$seconds" ]; then
      docker stop -t 5 "$name" >/dev/null 2>&1
      break
    fi
    mg_sleep 1
    waited=$((waited + 1))
  done
  wait "$pid" 2>/dev/null
  mg_gallery__commit_pending "$site" "$dir/mounts.json"
  local decisions
  decisions="$(ls "$MG_ROOT"/docs/design/reviews/*.json 2>/dev/null | sort | tail -n 1)"
  mg_detail changed "$(git -C "$MG_ROOT" status --porcelain -- docs/design | jq -R . | jq -sc .)"
  mg_message "gallery: served $site; decisions on this checkout are in docs/design (uncommitted)${decisions:+, latest review ${decisions#"$MG_ROOT"/}}"
  [ -s "$MG_STATE/failures.jsonl" ] && mg_status error || mg_status pass
}

# Commits each pending branch's approvals in its worktree (U19's chat
# approval rule: the trailer names Antoine).
mg_gallery__commit_pending() {
  local site="$1" mounts="$2" container wt files msg
  [ -f "$site/pending-commits.jsonl" ] || return 0
  for container in $(jq -r '.checkout' "$site/pending-commits.jsonl" | sort -u); do
    wt="$(jq -r --arg c "$container" 'first(.[] | select(.container == $c) | .worktree) // empty' "$mounts")"
    [ -n "$wt" ] || { mg_failure pending-commit "no worktree for $container"; continue; }
    files="$(jq -r --arg c "$container" 'select(.checkout == $c) | .files[]' "$site/pending-commits.jsonl" | sort -u)"
    msg="$MG_RUN_DIR/commit-$(basename "$container").txt"
    {
      echo "Approve changed signature baselines in the review gallery"
      echo
      echo "Antoine approved these images in the review gallery ($(basename "$(dirname "$site")"));"
      echo "the merge gate re-runs the baseline guard against them."
      echo
      echo "Approved-by: antoine"
    } >"$msg"
    # shellcheck disable=SC2086 # one path per word
    if git -C "$wt" add -- $files && git -C "$wt" commit -q -F "$msg" -- $files; then
      mg_detail "committed_$(basename "$container")" "$(jq -n --arg w "$wt" --arg s "$(git -C "$wt" rev-parse HEAD)" '{worktree: $w, commit: $s}')"
    else
      mg_failure pending-commit "could not commit the approvals in $wt; see log.txt"
    fi
  done
}
