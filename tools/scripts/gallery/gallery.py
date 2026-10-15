#!/usr/bin/env python3
"""The review gallery (U10, R1988, R1989), in the image-tools container.

  gallery.py build --items ITEMS.json --out DIR [--picks PICKS.json] [--ledger L]...
      a static page: every item as old | new | diff, the screens' open critic
      issues, and variant picks; DIR/gallery.json lists what the page shows
  gallery.py serve --gallery DIR [--port 8790]
      the review server (server.py): serves the page and records Antoine's
      approve / reject / note decisions and picks

ITEMS.json, written by tools/mg gallery: [{"id", "screen", "state", "variant",
"platform", "baseline" (repo-relative), "new", "old" (paths or null),
"checkout" (where decisions are written, as this container sees it),
"branch", "pending" (true for a branch the baseline guard escalated),
"signature", "committed_blob" (the git blob HEAD holds at the baseline's
path when the file there is that blob, else null: only a committed image
can be approved, since the guard reads approved images back from git)}].

Decisions are serialized: one at a time per gallery (the server's threads
and processes), and one at a time per checkout they are written to, whichever
gallery takes them (an flock in the checkout's build/mg/gallery/). Every file
is replaced through a temporary file of its own.
"""

import argparse
import contextlib
import datetime
import fcntl
import hashlib
import html
import json
import os
import re
import shutil
import sys
import tempfile
import threading

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(os.path.dirname(HERE), "image-diff"))
sys.path.insert(0, os.path.join(os.path.dirname(HERE), "screen-status"))

import image_diff  # noqa: E402

APPROVED = "docs/design/approved-baselines.json"
REVIEWS = "docs/design/reviews"
DECISIONS = "docs/design/decisions.md"


class GalleryError(Exception):
    pass


def today():
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%d")


def now():
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def git_blob_id(data):
    """The id `git hash-object` gives these bytes."""
    return hashlib.sha1(b"blob %d\0" % len(data) + data).hexdigest()


BASELINE_PATH = re.compile(r"^app/Snapshots/(?P<key>[^/]+)/(?P<cell>.+)$")


def same_cell(path):
    """(model, cell path) of a baseline: the same cell of the same model under
    another OS build has the same identity."""
    match = BASELINE_PATH.match(path or "")
    if not match:
        return None
    return match.group("key").rsplit("-", 1)[0], match.group("cell")


# ---------------------------------------------------------------------------
# Build

def open_issues(ledgers):
    issues = {}
    for path in ledgers or []:
        if not os.path.exists(path):
            continue
        import yaml

        with open(path) as handle:
            data = yaml.safe_load(handle) or {}
        for screen, entry in (data.get("screens") or {}).items():
            for issue in (entry or {}).get("issues") or []:
                issues.setdefault(screen, []).append(issue)
    return issues


def build(items, out, picks=None, ledgers=None, title="Meguri review gallery"):
    if os.path.exists(out):
        shutil.rmtree(out)
    os.makedirs(os.path.join(out, "images"))
    shown = []
    for n, item in enumerate(items):
        entry = dict(item)
        for side in ("old", "new"):
            path = item.get(side)
            if path and os.path.exists(path):
                rel = os.path.join("images", "%04d-%s.png" % (n, side))
                shutil.copyfile(path, os.path.join(out, rel))
                entry[side] = rel
            else:
                entry[side] = None
        if entry["old"] and entry["new"]:
            diff = os.path.join("images", "%04d-diff.png" % n)
            result = image_diff.compare(os.path.join(out, entry["old"]), os.path.join(out, entry["new"]),
                                        diff_path=os.path.join(out, diff))
            entry["change"] = "unchanged" if result["match"] else "changed"
            entry["diff"] = diff if not result["match"] else None
            entry["comparison"] = {k: result.get(k) for k in ("pixel_precision", "max_delta_e", "reason")}
        else:
            entry["change"] = "new" if entry["new"] else "removed"
            entry["diff"] = None
        shown.append(entry)
    manifest = {
        "title": title,
        "built": now(),
        "items": shown,
        "issues": open_issues(ledgers),
        "picks": picks or [],
    }
    for pick in manifest["picks"]:
        for variant in pick.get("variants", []):
            src = variant.get("image")
            if src and os.path.exists(src):
                rel = os.path.join("images", "pick-%s-%s.png" % (pick["screen"], variant["name"]))
                shutil.copyfile(src, os.path.join(out, rel))
                variant["image"] = rel
    with open(os.path.join(out, "gallery.json"), "w") as handle:
        json.dump(manifest, handle, indent=2)
    with open(os.path.join(out, "index.html"), "w") as handle:
        handle.write(render(manifest))
    return manifest


def esc(value):
    return html.escape(str(value), quote=True)


def full_size(src, alt):
    """An image that opens at its own size in a new tab: the page shows it
    scaled to its column, and a reviewer judges pixels (R1989)."""
    return "<a href='%s' target=_blank rel=noopener><img loading=lazy src='%s' alt='%s'></a>" % (esc(src), esc(src), esc(alt))


def render(manifest):
    """The page: one section per screen, pending approvals first."""
    by_screen = {}
    for item in manifest["items"]:
        group = ("pending: %s" % item.get("branch")) if item.get("pending") else item["screen"]
        by_screen.setdefault(group, []).append(item)
    parts = [PAGE_HEAD.replace("{{title}}", esc(manifest["title"])), "<h1>%s</h1>" % esc(manifest["title"])]
    parts.append("<p class=meta>Built %s. %d images; %d changed or new.</p>" % (
        esc(manifest["built"]), len(manifest["items"]),
        sum(1 for i in manifest["items"] if i["change"] in ("changed", "new"))))
    for group in sorted(by_screen, key=lambda g: (not g.startswith("pending"), g)):
        items = by_screen[group]
        screen = items[0]["screen"]
        parts.append("<section><h2>%s</h2>" % esc(group))
        for issue in manifest["issues"].get(screen, []):
            parts.append("<p class='issue %s'>%s · %s · %s: %s</p>" % (
                esc(issue.get("severity")), esc(issue.get("severity")), esc(issue.get("rule")),
                esc(issue.get("variant")), esc(issue.get("summary"))))
        for item in items:
            parts.append("<article data-item='%s'><h3>%s / %s <span class='badge %s'>%s</span></h3><div class=row>" % (
                esc(item["id"]), esc(item["state"]), esc(item["variant"]), esc(item["change"]), esc(item["change"])))
            for side in ("old", "new", "diff"):
                src = item.get(side)
                parts.append("<figure><figcaption>%s</figcaption>%s</figure>" % (
                    side, full_size(src, "%s %s" % (side, item["id"])) if src else "<div class=none>none</div>"))
            parts.append("</div><div class=actions>"
                         "<input class=note placeholder='Note (optional for approve)'>"
                         "<button data-decision=approve>Approve</button>"
                         "<button data-decision=reject>Reject</button>"
                         "<button data-decision=note>Note</button>"
                         "<span class=state></span></div></article>")
        parts.append("</section>")
    for pick in manifest["picks"]:
        parts.append("<section class=pick data-screen='%s'><h2>Pick a variant: %s</h2><div class=row>" % (esc(pick["screen"]), esc(pick["screen"])))
        for variant in pick.get("variants", []):
            parts.append("<figure><figcaption><label><input type=radio name='pick-%s' value='%s'> %s</label></figcaption>%s</figure>" % (
                esc(pick["screen"]), esc(variant["name"]), esc(variant["name"]), full_size(variant.get("image") or "", variant["name"])))
        parts.append("</div><div class=actions><input class=note placeholder='Why this one'>"
                     "<button data-pick>Pick</button><span class=state></span></div></section>")
    parts.append(PAGE_SCRIPT + "</body></html>")
    return "\n".join(parts)


PAGE_HEAD = """<!doctype html><html lang=en><head><meta charset=utf-8>
<meta name=viewport content="width=device-width, initial-scale=1"><title>{{title}}</title>
<style>
:root{--bg:#fff;--fg:#1d1d1f;--muted:#6e6e73;--line:#d2d2d7;--high:#c62828;--medium:#b26a00;--low:#6e6e73;--ok:#2e7d32}
@media (prefers-color-scheme:dark){:root{--bg:#1c1c1e;--fg:#f5f5f7;--muted:#a1a1a6;--line:#3a3a3c;--high:#ff6b6b;--medium:#ffb74d;--ok:#81c784}}
body{background:var(--bg);color:var(--fg);font:15px -apple-system,system-ui,sans-serif;margin:0 auto;max-width:1600px;padding:16px}
h2{border-top:1px solid var(--line);padding-top:16px}.meta{color:var(--muted)}
.row{display:flex;gap:12px;overflow-x:auto}figure{margin:0;flex:1;min-width:220px}figcaption{color:var(--muted);font-size:13px}
img{max-width:100%;border:1px solid var(--line)}.none{border:1px dashed var(--line);padding:24px;color:var(--muted)}
.issue.high{color:var(--high)}.issue.medium{color:var(--medium)}.issue.low{color:var(--low)}
.badge{font-size:12px;border:1px solid var(--line);border-radius:8px;padding:1px 6px}.badge.changed,.badge.new{color:var(--medium)}
.actions{display:flex;gap:8px;align-items:center;margin:8px 0 24px}.note{flex:1;min-width:120px}.state{color:var(--ok)}
</style></head><body>"""

PAGE_SCRIPT = """<script>
// Decisions go to the review server; a page opened from disk only says so.
const reviewing = location.protocol === 'http:' || location.protocol === 'https:';
function show(label, text) { label.textContent = text; }
function send(endpoint, payload, label) {
  if (!reviewing) return show(label, 'open through tools/mg gallery serve to record decisions');
  const request = new Request(endpoint, {method: 'POST', body: JSON.stringify(payload)});
  request.headers.set('Content-Type', 'application/json');
  fetch(request).then(reply => reply.json().then(answer => {
    show(label, reply.ok ? 'recorded: ' + answer.decision : 'error: ' + answer.error);
  })).catch(problem => show(label, 'error: ' + problem));
}
for (const article of document.querySelectorAll('article')) {
  const label = article.querySelector('.state');
  for (const button of article.querySelectorAll('button')) {
    button.addEventListener('click', () => send('/api/decision',
      {item: article.dataset.item, decision: button.dataset.decision, note: article.querySelector('.note').value}, label));
  }
}
for (const section of document.querySelectorAll('section.pick')) {
  const label = section.querySelector('.state');
  section.querySelector('button').addEventListener('click', () => {
    const chosen = section.querySelector('input[type=radio]:checked');
    send('/api/pick', {screen: section.dataset.screen, variant: chosen ? chosen.value : '',
      reason: section.querySelector('.note').value}, label);
  });
}
</script>"""


# ---------------------------------------------------------------------------
# Decisions (the review server calls these; Antoine is the only reviewer)

def read_json(path, default):
    if not os.path.exists(path):
        return default
    with open(path) as handle:
        return json.load(handle)


def write_json(path, data):
    """Replaces the file through a temporary file of its own."""
    directory = os.path.dirname(path)
    os.makedirs(directory, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=directory, prefix="." + os.path.basename(path) + ".", suffix=".tmp")
    try:
        with os.fdopen(fd, "w") as handle:
            json.dump(data, handle, indent=2)
            handle.write("\n")
        os.replace(tmp, path)
    except BaseException:
        with contextlib.suppress(OSError):
            os.unlink(tmp)
        raise


_DECISIONS = threading.Lock()


@contextlib.contextmanager
def flocked(path):
    """An exclusive flock on `path` (created if missing) for the block."""
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "a") as handle:
        fcntl.flock(handle, fcntl.LOCK_EX)
        try:
            yield
        finally:
            fcntl.flock(handle, fcntl.LOCK_UN)


@contextlib.contextmanager
def one_decision_at_a_time(gallery_dir):
    """Holds a gallery's decisions to one at a time: the server's threads
    share a lock, and other processes serving the same gallery an flock on
    gallery_dir/.decisions.lock (its pending-commits list)."""
    with _DECISIONS:
        with flocked(os.path.join(gallery_dir, ".decisions.lock")):
            yield


def checkout_lock(checkout):
    """Holds the read-modify-write of a checkout's review, approval and
    decision files to one decision at a time, across galleries: two galleries
    (processes, containers) may write to one checkout. Taken after the
    gallery's own lock, never before it."""
    return flocked(os.path.join(checkout, "build", "mg", "gallery", "decisions.lock"))


def record_review(checkout, entry, commit=None):
    """Appends a decision to docs/design/reviews/<date>.json; returns its path."""
    rel = os.path.join(REVIEWS, today() + ".json")
    path = os.path.join(checkout, rel)
    review = read_json(path, {"date": today(), "reviewer": "antoine", "decisions": []})
    if commit:
        review["commit"] = commit
    review["decisions"].append(dict(entry, time=now()))
    write_json(path, review)
    return rel


def decide(gallery_dir, item_id, decision, note=""):
    """Records Antoine's decision on one gallery item: every decision goes in
    the day's review file; an approval also locks the image as the approved
    baseline (approved-baselines.json), retiring in the same write the
    entries it supersedes (the same model's same cell under another OS
    build). Returns what was written."""
    with one_decision_at_a_time(gallery_dir):
        return _decide(gallery_dir, item_id, decision, note)


def _decide(gallery_dir, item_id, decision, note):
    if decision not in ("approve", "reject", "note"):
        raise GalleryError("decision must be approve, reject or note")
    if decision in ("reject", "note") and not note.strip():
        raise GalleryError("a %s needs a note" % decision)
    manifest = read_json(os.path.join(gallery_dir, "gallery.json"), None)
    if manifest is None:
        raise GalleryError("no gallery.json in %s" % gallery_dir)
    item = next((i for i in manifest["items"] if i["id"] == item_id), None)
    if item is None:
        raise GalleryError("no item %s in this gallery" % item_id)
    checkout = item["checkout"]
    with checkout_lock(checkout):
        written = _record(gallery_dir, manifest, item, checkout, decision, note)
    if item.get("pending"):
        with open(os.path.join(gallery_dir, "pending-commits.jsonl"), "a") as handle:
            handle.write(json.dumps({"checkout": checkout, "branch": item.get("branch"), "files": written["files"], "item": item_id}) + "\n")
    return written


def _record(gallery_dir, manifest, item, checkout, decision, note):
    """The decision's writes in the checkout (under its lock)."""
    item_id = item["id"]
    entry = {
        "item": item_id, "screen": item["screen"], "state": item["state"], "variant": item["variant"],
        "baseline": item["baseline"], "decision": decision, "note": note.strip(), "branch": item.get("branch"),
    }
    written = {"decision": decision, "files": []}
    if decision == "approve":
        if not item.get("new"):
            raise GalleryError("%s has no image to approve" % item_id)
        with open(os.path.join(gallery_dir, item["new"]), "rb") as handle:
            data = handle.read()
        entry["sha256"] = hashlib.sha256(data).hexdigest()
        current = os.path.join(checkout, item["baseline"])
        if os.path.exists(current):
            with open(current, "rb") as handle:
                if hashlib.sha256(handle.read()).hexdigest() != entry["sha256"]:
                    raise GalleryError("%s changed since the gallery was built; rebuild it" % item["baseline"])
        # The guard reads the approved image back from git by its blob id:
        # only the image HEAD holds at that path can be approved.
        blob = git_blob_id(data)
        if item.get("committed_blob") != blob:
            raise GalleryError("%s is not committed as shown (HEAD holds %s at that path); commit it, rebuild the gallery, then approve" % (
                item["baseline"], item.get("committed_blob") or "nothing"))
        path = os.path.join(checkout, APPROVED)
        approved = read_json(path, {"version": 1, "baselines": []})
        cell = same_cell(item["baseline"])
        superseded = sorted(b["path"] for b in approved["baselines"]
                            if b["path"] != item["baseline"] and cell is not None and same_cell(b["path"]) == cell)
        if superseded:
            entry["supersedes"] = superseded
    review = record_review(checkout, entry, manifest.get("commit"))
    written["files"].append(review)
    if decision == "approve":
        approved["baselines"] = [b for b in approved["baselines"] if b["path"] != item["baseline"] and b["path"] not in superseded]
        approved["baselines"].append(dict({
            "path": item["baseline"], "screen": item["screen"], "state": item["state"], "variant": item["variant"],
            "signature": bool(item.get("signature")), "sha256": entry["sha256"], "blob": blob,
            "approved_by": "antoine", "approved_on": today(), "review": review,
        }, **({"supersedes": superseded} if superseded else {})))
        approved["baselines"].sort(key=lambda b: b["path"])
        write_json(path, approved)
        written["files"].append(APPROVED)
    return written


def pick(gallery_dir, screen, variant, reason):
    """Antoine's variant pick for a signature screen (R1988): a line in
    decisions.md and the day's review file."""
    with one_decision_at_a_time(gallery_dir):
        return _pick(gallery_dir, screen, variant, reason)


def _pick(gallery_dir, screen, variant, reason):
    manifest = read_json(os.path.join(gallery_dir, "gallery.json"), None) or {}
    offered = next((p for p in manifest.get("picks", []) if p["screen"] == screen), None)
    if offered is None or variant not in [v["name"] for v in offered.get("variants", [])]:
        raise GalleryError("no variant %s offered for %s" % (variant, screen))
    if not reason.strip():
        raise GalleryError("a pick needs its reason")
    checkout = offered.get("checkout", ".")
    with checkout_lock(checkout):
        review = record_review(checkout, {"screen": screen, "decision": "pick", "variant": variant, "note": reason.strip()})
        reason_cell = reason.strip().replace("|", "/").replace("\n", " ")
        with open(os.path.join(checkout, DECISIONS), "a") as handle:
            handle.write("| %s | %s | Picked %s | %s | gallery, %s |\n" % (today(), screen, variant, reason_cell, review.replace("docs/design/", "")))
    return {"decision": "pick", "files": [review, DECISIONS]}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    b = sub.add_parser("build")
    b.add_argument("--items", required=True)
    b.add_argument("--out", required=True)
    b.add_argument("--picks")
    b.add_argument("--ledger", action="append")
    b.add_argument("--commit")
    s = sub.add_parser("serve")
    s.add_argument("--gallery", required=True)
    s.add_argument("--port", type=int, default=8790)
    s.add_argument("--bind", default="0.0.0.0")
    args = parser.parse_args(argv)
    if args.command == "build":
        items = read_json(args.items, [])
        picks = read_json(args.picks, []) if args.picks else []
        manifest = build(items, args.out, picks, args.ledger)
        if args.commit:
            manifest["commit"] = args.commit
            write_json(os.path.join(args.out, "gallery.json"), manifest)
        print(json.dumps({"items": len(manifest["items"]), "changed": sum(1 for i in manifest["items"] if i["change"] != "unchanged"),
                          "picks": len(manifest["picks"])}))
        return 0
    import server

    return server.serve(args.gallery, args.bind, args.port)


if __name__ == "__main__":
    sys.exit(main())
