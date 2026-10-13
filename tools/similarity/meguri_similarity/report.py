"""The two report views.

The implementer view names only Meguri files, regions, channels and scores: it
is what an implementer session may see. The reviewer view adds the matching
Anki source, tag, path and lines, for human review (R1957). Neither view holds
any corpus text.
"""

from __future__ import annotations

import json
import os

REWRITE = "rewrite this region from the spec"


def _channel_dict(c):
    d = {
        "fingerprints": c.fingerprints,
        "matched": c.matched,
    }
    if c.score is not None:
        d["score"] = round(c.score, 4)
    if c.channel == "struct":
        d["longest_run"] = c.longest_run
    return d


def _region(r, reviewer):
    d = {"lines": [r.start, r.end], "matched": r.matched}
    if r.labels:
        d["keys"] = r.labels[:10]
    if reviewer:
        d["anki"] = r.sources
    return d


def _file_entry(f, reviewer):
    entry = {
        "path": f.path,
        "language": f.language,
        "status": f.status,
        "channels": {name: _channel_dict(c) for name, c in f.channels.items()},
    }
    if f.error:
        entry["error"] = f.error
    flags = []
    for name, c in f.channels.items():
        if c.flagged:
            flags.append(
                {
                    "channel": name,
                    "reasons": c.reasons,
                    "regions": [_region(r, reviewer) for r in c.regions[:20]],
                    "action": REWRITE,
                }
            )
    if flags:
        entry["flags"] = flags
    if f.allowlisted:
        entry["allowlisted"] = [
            {
                "channel": name,
                "entries": entries,
                "reasons": raw.reasons,
                "regions": [_region(r, reviewer) for r in raw.regions[:20]],
            }
            for name, entries, raw in f.allowlisted
        ]
    return entry


def build(files, summary, reviewer):
    view = dict(summary)
    view["view"] = "reviewer" if reviewer else "implementer"
    if not reviewer:
        view.pop("corpus", None)
    view["files"] = [_file_entry(f, reviewer) for f in files]
    return view


def _markdown(view):
    reviewer = view["view"] == "reviewer"
    out = [f"# Similarity scan: {view['status']} ({view['view']} view)", ""]
    out.append(
        f"{view['counts']['scanned']} files scanned, {view['counts']['flagged']} flagged, "
        f"{view['counts']['allowlisted']} allowlisted, {view['counts']['skipped']} out of scope."
    )
    if reviewer and view.get("corpus"):
        c = view["corpus"]
        out.append(f"Corpus: {c.get('tags')} tags {c.get('first_tag')}..{c.get('last_tag')}, index {c.get('key')}.")
    out.append("")
    for f in view["files"]:
        if f["status"] not in ("flagged", "allowlisted", "error"):
            continue
        out.append(f"## {f['path']} ({f['status']})")
        if f.get("error"):
            out.append(f"- error: {f['error']}")
        for flag in f.get("flags", []):
            out.append(f"- {flag['channel']}: {'; '.join(flag['reasons'])}. Action: {flag['action']}.")
            for r in flag["regions"]:
                line = f"  - lines {r['lines'][0]}-{r['lines'][1]} ({r['matched']} matching fingerprints)"
                if r.get("keys"):
                    line += f", keys {', '.join(r['keys'])}"
                out.append(line)
                for a in r.get("anki", []):
                    out.append(f"    - Anki {a['source']} {a['tag']} {a['path']}:{a['lines'][0]}-{a['lines'][1]} ({a['matched']})")
        for al in f.get("allowlisted", []):
            out.append(f"- {al['channel']}: allowlisted by entries {al['entries']} ({'; '.join(al['reasons'])})")
        out.append("")
    return "\n".join(out) + "\n"


def write(out_dir, files, summary):
    os.makedirs(out_dir, exist_ok=True)
    paths = {}
    for reviewer in (False, True):
        view = build(files, summary, reviewer)
        name = "reviewer" if reviewer else "implementer"
        with open(os.path.join(out_dir, f"{name}.json"), "w") as f:
            json.dump(view, f, indent=1)
        with open(os.path.join(out_dir, f"{name}.md"), "w") as f:
            f.write(_markdown(view))
        paths[name] = os.path.join(out_dir, f"{name}.json")
    return paths
