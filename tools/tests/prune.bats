#!/usr/bin/env bats
# tools/mg prune on synthetic build/mg trees, in a throwaway repository with a
# copy of tools/ (tools/scripts/prune-run-output.py holds the rule).

bats_require_minimum_version 1.5.0

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

setup_file() {
  local t="$BATS_FILE_TMPDIR/template"
  mkdir -p "$t"
  cp -R "$REPO_ROOT/tools" "$t/tools"
  cp "$REPO_ROOT/.gitignore" "$t/"
  git -C "$t" init -q -b master
  git -C "$t" add -A
  git -C "$t" -c user.name=selftest -c user.email=selftest@localhost commit -qm template
}

setup() {
  SB="$BATS_TEST_TMPDIR/repo"
  cp -R "$BATS_FILE_TMPDIR/template" "$SB"
  MG="$SB/build/mg"
  mkdir -p "$MG"
  FAKE_PID=""
}

teardown() {
  [ -z "$FAKE_PID" ] || kill "$FAKE_PID" 2>/dev/null || true
}

commit() { git -C "$SB" -c user.name=selftest -c user.email=selftest@localhost commit -qm "$1"; }

# prune <args>: run the sandbox's tools/mg prune; sets status, stderr, result.
prune() {
  run --separate-stderr "$SB/tools/mg" prune --no-docker "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}

plan() { echo "$(dirname "$result")/plan.json"; }

# old_run <job> <n>: a finished run of $MG/<job> started and finished on
# 2025-01-01 (minute n); prints its id. Pids this high are never live.
old_run() {
  local id
  id="$(printf '20250101T%02d%02d00Z-%d' $(($2 / 60)) $(($2 % 60)) $((4190000 + $2)))"
  mkdir -p "$MG/$1/$id"
  printf '{"job":"%s","run_id":"%s","status":"pass"}\n' "$1" "$id" >"$MG/$1/$id/result.json"
  echo log >"$MG/$1/$id/log.txt"
  touch -t 202501010000 "$MG/$1/$id/result.json"
  echo "$id"
}

# cleanroom_run <n> <month> [status]: an exported cleanroom run of 2025-<month>
# with its records, a source copy and codex-home; prints its id.
cleanroom_run() {
  local id d f
  id="$(printf '2025%02d01T00%02d00Z-%d' "$2" "$1" $((4190000 + $1)))"
  d="$MG/cleanroom/$id"
  mkdir -p "$d/source/core" "$d/codex-home/sessions"
  echo "fn main() {}" >"$d/source/core/main.rs"
  echo "stand-in" >"$d/codex-home/auth.json"
  echo '{"kind":"session"}' >"$d/codex-home/sessions/rollout.jsonl"
  for f in prompt.md changes.patch seed.patch source-view.json log.txt codex.log codex.out guard-marker; do
    echo "$f of $id" >"$d/$f"
  done
  jq -n --arg id "$id" --arg c "$(git -C "$SB" rev-parse HEAD)" --arg s "${3:-pass}" \
    '{job: "cleanroom", run_id: $id, status: $s, dry_run: false,
      args: ["--prompt", "p.md", "--session", "s"], details: {source_commit: $c}}' >"$d/result.json"
  touch -t 202501010000 "$d/result.json"
  echo "$id"
}

members() { # members <archive>: one member name per line
  python3 -c 'import sys, tarfile; [print(m.name) for m in tarfile.open(sys.argv[1], "r:xz")]' "$1"
}

member() { # member <archive> <name>: its content
  python3 -c 'import sys, tarfile; sys.stdout.write(tarfile.open(sys.argv[1], "r:xz").extractfile(sys.argv[2]).read().decode())' "$1" "$2"
}

@test "prune reports what would go and deletes nothing without --apply" {
  local n before
  for n in $(seq 1 25); do old_run alpha "$n" >/dev/null; done
  before="$(cd "$MG" && find alpha | sort)"
  prune
  [ "$status" -eq 0 ]
  [ "$(jq -r .status "$result")" = pass ]
  jq -e '.details.jobs[] | select(.job == "alpha" and .runs == 25 and .delete == 5 and .keep == 20
    and .kept_because.newest == 20 and .bytes_freed > 0)' "$result"
  [ "$(jq -r .summary.delete "$result")" = 5 ]
  [ "$(cd "$MG" && find alpha | sort)" = "$before" ]
  [ "$(jq '[.[] | select(.job == "alpha" and .action == "delete")] | length' "$(plan)")" = 5 ]
  [[ "$stderr" == *"keep the newest 20 runs"* ]]
}

@test "prune --apply keeps the newest runs of each job, recent runs and the latest run" {
  local ids=() n recent
  for n in $(seq 1 25); do ids[$n]="$(old_run alpha "$n")"; done
  ln -s "${ids[1]}" "$MG/alpha/latest"
  touch "$MG/alpha/${ids[2]}/result.json" # an old run that finished just now
  recent="$(date -u +%Y%m%dT%H%M%SZ)-4190999"
  mkdir -p "$MG/beta/$recent"
  echo '{}' >"$MG/beta/$recent/result.json"
  prune --apply
  [ "$status" -eq 0 ]
  [ "$(jq -r .status "$result")" = pass ]
  [ "$(jq -r .summary.deleted "$result")" = 3 ]
  [ -d "$MG/alpha/${ids[1]}" ]
  [ -d "$MG/alpha/${ids[2]}" ]
  for n in 3 4 5; do [ ! -e "$MG/alpha/${ids[$n]}" ]; done
  for n in $(seq 6 25); do [ -d "$MG/alpha/${ids[$n]}" ]; done
  [ -L "$MG/alpha/latest" ]
  [ -d "$MG/beta/$recent" ]
  jq -e --arg id "${ids[1]}" '.[] | select(.run == $id and (.reasons | index("latest")))' "$(plan)"
  jq -e --arg id "${ids[2]}" '.[] | select(.run == $id and .reasons == ["recent"])' "$(plan)"
}

@test "prune keeps runs named in tracked, uncommitted, other-branch and state files, and in the receipts of kept runs" {
  local ids=() n gate
  for n in $(seq 1 31); do ids[$n]="$(old_run gamma "$n")"; done
  mkdir -p "$SB/docs"
  echo "receipt build/mg/gamma/${ids[1]}/result.json" >"$SB/docs/review.md"
  git -C "$SB" add docs/review.md
  commit review
  echo "see ${ids[2]}" >"$SB/notes.txt"
  git -C "$SB" checkout -q -b other
  echo "${ids[3]}" >"$SB/docs/other.md"
  git -C "$SB" add docs/other.md
  commit other
  git -C "$SB" checkout -q master
  gate="$(date -u +%Y%m%dT%H%M%SZ)-4190998"
  mkdir -p "$MG/gate/$gate"
  echo '{}' >"$MG/gate/$gate/result.json"
  printf '{"result":"%s/gamma/%s/result.json"}\n' "$MG" "${ids[4]}" >"$MG/gate/$gate/results.jsonl"
  printf '{"child":"%s"}\n' "${ids[5]}" >"$MG/gamma/${ids[4]}/receipt.json"
  mkdir -p "$MG/catch-up"
  printf '{"run":"%s"}\n' "${ids[6]}" >"$MG/catch-up/queue.jsonl"
  printf '{"child":"%s"}\n' "${ids[7]}" >"$MG/gamma/${ids[8]}/receipt.json"
  echo "${ids[10]}" >>"$MG/gamma/${ids[9]}/log.txt"
  echo "staged ${ids[11]}" >"$SB/docs/staged.md"
  git -C "$SB" add docs/staged.md
  prune --apply
  [ "$status" -eq 0 ]
  [ "$(jq -r .status "$result")" = pass ]
  for n in 1 2 3 4 5 6 11; do [ -d "$MG/gamma/${ids[$n]}" ] || { echo "run $n was deleted"; false; }; done
  for n in 7 8 9 10; do [ ! -e "$MG/gamma/${ids[$n]}" ] || { echo "run $n was kept"; false; }; done
  for n in $(seq 12 31); do [ -d "$MG/gamma/${ids[$n]}" ]; done
  jq -e --arg id "${ids[5]}" '.[] | select(.run == $id and .reasons == ["referenced"])' "$(plan)"
}

@test "prune keeps unfinished runs and runs whose tools/mg process still runs" {
  local ids=() n live
  for n in $(seq 1 24); do ids[$n]="$(old_run delta "$n")"; done
  rm "$MG/delta/${ids[1]}/result.json"
  mkdir "$MG/delta/${ids[2]}/.mg"
  mkdir -p "$BATS_TEST_TMPDIR/fake/tools"
  printf '#!/bin/bash\nsleep 60 &\nwait\n' >"$BATS_TEST_TMPDIR/fake/tools/mg"
  chmod +x "$BATS_TEST_TMPDIR/fake/tools/mg"
  "$BATS_TEST_TMPDIR/fake/tools/mg" 3>&- &
  FAKE_PID=$!
  live="20250101T000030Z-$FAKE_PID"
  mkdir -p "$MG/delta/$live"
  echo '{}' >"$MG/delta/$live/result.json"
  touch -t 202501010000 "$MG/delta/$live/result.json"
  prune --apply
  [ "$status" -eq 0 ]
  [ -d "$MG/delta/${ids[1]}" ]
  [ -d "$MG/delta/${ids[2]}" ]
  [ -d "$MG/delta/$live" ]
  [ ! -e "$MG/delta/${ids[3]}" ]
  [ ! -e "$MG/delta/${ids[4]}" ]
  jq -e --arg id "$live" '.[] | select(.run == $id and .reasons == ["live"])' "$(plan)"
  jq -e --arg id "${ids[2]}" '.[] | select(.run == $id and (.reasons | index("unfinished")))' "$(plan)"
}

@test "prune --apply finishes a deletion an earlier --apply cut short, and the report leaves it" {
  local n
  for n in $(seq 1 21); do old_run alpha "$n" >/dev/null; done
  mkdir -p "$MG/alpha/.prune-deleting-20250101T000000Z-4190700/sub"
  echo partial >"$MG/alpha/.prune-deleting-20250101T000000Z-4190700/sub/file"
  prune
  [ -d "$MG/alpha/.prune-deleting-20250101T000000Z-4190700" ]
  jq -e '.details.pending == ["alpha/.prune-deleting-20250101T000000Z-4190700"]' "$result"
  prune --apply
  [ "$(jq -r .status "$result")" = pass ]
  [ ! -e "$MG/alpha/.prune-deleting-20250101T000000Z-4190700" ]
  [ "$(jq -r .details.pending_finished "$result")" = 1 ]
  [ -z "$(find "$MG/alpha" -maxdepth 1 -name '.prune-deleting-*')" ]
  [ "$(find "$MG/alpha" -mindepth 1 -maxdepth 1 -type d | wc -l)" -eq 20 ]
}

@test "prune keeps a run folder that holds a tracked file" {
  local ids=() n
  for n in $(seq 1 25); do ids[$n]="$(old_run alpha "$n")"; done
  git -C "$SB" add -f "build/mg/alpha/${ids[1]}/log.txt"
  commit tracked
  prune --apply
  [ "$status" -eq 0 ]
  [ -f "$MG/alpha/${ids[1]}/log.txt" ]
  [ -z "$(git -C "$SB" status --porcelain)" ]
  [ ! -e "$MG/alpha/${ids[2]}" ]
}

@test "prune --apply refuses a build/mg that resolves outside the checkout" {
  local outside="$BATS_TEST_TMPDIR/outside" n
  mkdir -p "$outside/mg" "$SB/build"
  rmdir "$MG"
  ln -s "$outside/mg" "$MG"
  for n in $(seq 1 25); do old_run alpha "$n" >/dev/null; done
  prune --apply
  [ "$(jq -r .status "$result")" = error ]
  jq -e '.failures[] | select(.id == "refused")' "$result"
  [ "$(find "$outside/mg/alpha" -mindepth 1 -maxdepth 1 -type d | wc -l)" -eq 25 ]
}

@test "prune deletes a run folder's own files only: links in it, linked runs and linked job folders stay" {
  local outside="$BATS_TEST_TMPDIR/outside" ids=() n linked="20250101T000000Z-4190500"
  mkdir -p "$outside/run" "$outside/job/20250101T000000Z-4190600"
  echo keep >"$outside/file"
  echo '{}' >"$outside/run/result.json"
  echo '{}' >"$outside/job/20250101T000000Z-4190600/result.json"
  for n in $(seq 1 25); do ids[$n]="$(old_run alpha "$n")"; done
  ln -s "$outside/file" "$MG/alpha/${ids[1]}/link-to-file"
  ln -s "$outside" "$MG/alpha/${ids[1]}/link-to-folder"
  ln -s "$outside/run" "$MG/alpha/$linked"
  ln -s "$outside/job" "$MG/linked"
  prune --apply
  [ "$status" -eq 0 ]
  [ "$(jq -r .status "$result")" = pass ]
  [ ! -e "$MG/alpha/${ids[1]}" ]
  [ "$(cat "$outside/file")" = keep ]
  [ -f "$outside/run/result.json" ]
  [ -L "$MG/alpha/$linked" ]
  [ -f "$outside/job/20250101T000000Z-4190600/result.json" ]
  jq -e '.details.skipped[] | select(.path | endswith("/build/mg/linked"))' "$result"
  jq -e --arg p "/alpha/$linked" '.details.skipped[] | select(.path | endswith($p))' "$result"
}

@test "prune --cleanroom archives records per month, verifies them, then removes source copies, codex-home and old folders" {
  local ids=() n a10="$MG/cleanroom-archive/2025-10.tar.xz" a11="$MG/cleanroom-archive/2025-11.tar.xz"
  for n in $(seq 1 13); do ids[$n]="$(cleanroom_run "$n" 10)"; done
  for n in $(seq 14 25); do ids[$n]="$(cleanroom_run "$n" 11)"; done
  for n in 1 2; do old_run alpha "$n" >/dev/null; done
  prune --cleanroom
  [ "$(jq -r .status "$result")" = pass ]
  jq -e '.details.cleanroom | .runs == 25 and .remove == 5 and .strip == 20 and .untouched == 0' "$result"
  [ ! -e "$MG/cleanroom-archive" ]
  [ -d "$MG/cleanroom/${ids[1]}/source" ]

  prune --cleanroom --apply
  [ "$status" -eq 0 ]
  [ "$(jq -r .status "$result")" = pass ]
  [ "$(jq -r .summary.archived "$result")" = 25 ]
  [ "$(jq -r .summary.removed "$result")" = 5 ]
  [ "$(members "$a10" | wc -l)" -eq 117 ]
  [ "$(members "$a11" | wc -l)" -eq 108 ]
  [ -z "$({ members "$a10"; members "$a11"; } | grep -E '/source(/|$)|codex-home|auth' || true)" ]
  [ "$(members "$a10" | grep -c "^${ids[1]}/")" -eq 9 ]
  [ "$(member "$a10" "${ids[1]}/changes.patch")" = "changes.patch of ${ids[1]}" ]
  for n in 1 2 3 4 5; do [ ! -e "$MG/cleanroom/${ids[$n]}" ]; done
  for n in $(seq 6 25); do
    [ -f "$MG/cleanroom/${ids[$n]}/prompt.md" ]
    [ ! -e "$MG/cleanroom/${ids[$n]}/source" ]
    [ ! -e "$MG/cleanroom/${ids[$n]}/codex-home" ]
  done
  [ "$(find "$MG/alpha" -mindepth 1 -maxdepth 1 | wc -l)" -eq 2 ]

  cp "$a10" "$BATS_TEST_TMPDIR/before.tar.xz"
  prune --cleanroom --apply
  [ "$(jq -r .status "$result")" = pass ]
  [ "$(jq -r .summary.removed "$result")" = 0 ]
  cmp "$a10" "$BATS_TEST_TMPDIR/before.tar.xz"
}

@test "prune --cleanroom restarts: an archive already holding some runs gains the rest once" {
  local ids=() n a="$MG/cleanroom-archive/2025-10.tar.xz"
  for n in $(seq 1 3); do ids[$n]="$(cleanroom_run "$n" 10)"; done
  prune --cleanroom --apply
  [ "$(members "$a" | wc -l)" -eq 27 ]
  for n in $(seq 4 6); do ids[$n]="$(cleanroom_run "$n" 10)"; done
  # A run interrupted after archiving, part way through its folder's removal.
  mkdir -p "$MG/cleanroom/${ids[1]}/source"
  rm "$MG/cleanroom/${ids[1]}/log.txt"
  echo "stale" >"$MG/cleanroom-archive/2025-10.tar.xz.partial"
  prune --cleanroom --apply
  [ "$(jq -r .status "$result")" = pass ]
  [ "$(members "$a" | wc -l)" -eq 54 ]
  [ "$(members "$a" | sort | uniq -d)" = "" ]
}

@test "prune --cleanroom leaves every part of a run whose records are not verified in an archive" {
  local ids=() n a10="$MG/cleanroom-archive/2025-10.tar.xz"
  for n in $(seq 1 13); do ids[$n]="$(cleanroom_run "$n" 10)"; done
  for n in $(seq 14 25); do ids[$n]="$(cleanroom_run "$n" 11)"; done
  mkdir -p "$MG/cleanroom-archive"
  python3 - "$a10" "${ids[1]}" <<'EOF'
import io, sys, tarfile
with tarfile.open(sys.argv[1], "w:xz") as tf:
    data = b"another prompt\n"
    info = tarfile.TarInfo(sys.argv[2] + "/prompt.md")
    info.size = len(data)
    tf.addfile(info, io.BytesIO(data))
EOF
  echo "not an archive" >"$MG/cleanroom-archive/2025-11.tar.xz"
  prune --cleanroom --apply
  [ "$(jq -r .status "$result")" = fail ]
  jq -e --arg id "${ids[1]}" '.failures[] | select(.id == "archive/" + $id)' "$result"
  jq -e '.failures[] | select(.id == "archive/2025-11")' "$result"
  [ -d "$MG/cleanroom/${ids[1]}/source" ]
  [ -d "$MG/cleanroom/${ids[1]}/codex-home" ]
  [ "$(member "$a10" "${ids[1]}/prompt.md")" = "another prompt" ]
  [ ! -e "$MG/cleanroom/${ids[2]}" ]
  for n in $(seq 14 25); do
    [ -d "$MG/cleanroom/${ids[$n]}/source" ]
    [ -d "$MG/cleanroom/${ids[$n]}/codex-home" ]
  done
  [ "$(cat "$MG/cleanroom-archive/2025-11.tar.xz")" = "not an archive" ]
}

@test "prune --cleanroom keeps the source of a run that did not pass and leaves runs with unexpected files" {
  local ok failed odd
  ok="$(cleanroom_run 1 10)"
  failed="$(cleanroom_run 2 10 error)"
  odd="$(cleanroom_run 3 10)"
  echo '{}' >"$MG/cleanroom/$odd/smoke-evidence.json"
  prune --cleanroom --apply
  [ "$(jq -r .status "$result")" = pass ]
  [ ! -e "$MG/cleanroom/$ok/source" ]
  [ -d "$MG/cleanroom/$failed/source" ]
  [ ! -e "$MG/cleanroom/$failed/codex-home" ]
  [ -d "$MG/cleanroom/$odd/source" ]
  [ -d "$MG/cleanroom/$odd/codex-home" ]
  [ -z "$(members "$MG/cleanroom-archive/2025-10.tar.xz" | grep "^$odd/" || true)" ]
  jq -e --arg id "$odd" '.[] | select(.run == $id and .action == "untouched"
    and (.reasons | any(startswith("unexpected entries: smoke-evidence.json"))))' "$(plan)"
}

@test "prune --root and --all-worktrees only report, and --root must be a checkout of this repository" {
  local second="$BATS_TEST_TMPDIR/second" n
  git -C "$SB" worktree add -q -b second "$second"
  for n in $(seq 1 25); do MG="$second/build/mg" old_run alpha "$n" >/dev/null; done
  prune --root "$second"
  [ "$(jq -r .status "$result")" = pass ]
  [ "$(jq -r .details.root "$result")" = "$(cd "$second" && pwd -P)" ]
  jq -e '.details.jobs[] | select(.job == "alpha" and .delete == 5)' "$result"
  [[ "$result" == "$SB/build/mg/prune/"* ]]
  [ ! -e "$second/build/mg/prune" ]
  prune --root "$second" --apply
  [ "$(jq -r .status "$result")" = error ]
  prune --all-worktrees --apply
  [ "$(jq -r .status "$result")" = error ]
  prune --root "$BATS_TEST_TMPDIR"
  [ "$(jq -r .status "$result")" = error ]
  prune --all-worktrees
  [ "$(jq -r .status "$result")" = pass ]
  [ "$(jq '.details.checkouts | length' "$result")" = 2 ]
  jq -e --arg p "$(cd "$second" && pwd -P)" '.details.checkouts[] | select(.checkout == $p and .delete == 5)' "$result"
  prune --bogus
  [ "$(jq -r .status "$result")" = error ]
  jq -e '.failures[] | select(.id == "usage")' "$result"
  [ "$(find "$second/build/mg/alpha" -mindepth 1 -maxdepth 1 -type d | wc -l)" -eq 25 ]
}
