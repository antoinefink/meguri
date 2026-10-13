"""Scanning Meguri files against the corpus index, with allowlist handling."""

from __future__ import annotations

import os
from collections import Counter, defaultdict
from dataclasses import dataclass, field

import numpy as np

from .config import glob_match
from .corpus import grouped_hashes
from .fingerprint import IdSpace, kgram_hashes, longest_true_run, periodicity, winnow
from .lexer import STRUCT_FAMILY, language_of


@dataclass
class Region:
    start: int
    end: int
    matched: int
    labels: list = field(default_factory=list)
    sources: list = field(default_factory=list)  # reviewer view only


@dataclass
class ChannelResult:
    channel: str
    fingerprints: int = 0
    matched: int = 0
    score: float | None = None
    longest_run: int = 0
    reasons: list = field(default_factory=list)
    regions: list = field(default_factory=list)

    @property
    def flagged(self):
        return bool(self.reasons)


@dataclass
class FileResult:
    path: str
    language: str
    status: str = "clean"  # clean | flagged | allowlisted | error
    channels: dict = field(default_factory=dict)  # effective results (allowlist applied)
    allowlisted: list = field(default_factory=list)  # [(channel, [entry indexes])]
    error: str | None = None


class Scanner:
    def __init__(self, cfg, index, lexer, allow):
        self.cfg = cfg
        self.index = index
        self.lexer = lexer
        self.allow = allow
        self.ids = IdSpace()
        self.ch = cfg["channels"]

    # -- scope -------------------------------------------------------------

    def in_scope(self, path):
        lang = language_of(path)
        if lang is None or lang not in self.cfg["scan"]["channels_by_language"]:
            return None
        for pattern in self.cfg["scan"].get("exclude") or []:
            if glob_match(path, pattern):
                return None
        return lang

    # -- fingerprints ------------------------------------------------------

    def _fingerprints(self, lexed, channels):
        """{channel: (hashes, first lines, last lines, labels or None, winnowed?)}"""
        out = {}
        if "struct" in channels:
            family = STRUCT_FAMILY.get(lexed.language)
            if family and lexed.struct:
                k, w = self.ch["struct"]["k"], self.ch["struct"]["w"]
                ids = self.ids.get("struct", family).ids(lexed.struct)
                h, starts = kgram_hashes(ids, k)
                keep = periodicity(ids, k) < self.ch["struct"]["max_periodicity"]
                h, starts = h[keep], starts[keep]
                sel = winnow(h, w)
                lines = np.asarray(lexed.struct_lines, dtype=np.uint32)
                out["struct"] = (h[sel], lines[starts[sel]], lines[starts[sel] + k - 1], None)
        if "lex" in channels and lexed.lex:
            k, w = self.ch["lex"]["k"], self.ch["lex"]["w"]
            ids = self.ids.get("lex").ids(lexed.lex)
            h, starts = kgram_hashes(ids, k)
            sel = winnow(h, w)
            lines = np.asarray(lexed.lex_lines, dtype=np.uint32)
            out["lex"] = (h[sel], lines[starts[sel]], lines[starts[sel] + k - 1], None)
        if "com" in channels and lexed.comments:
            h, first, last = grouped_hashes(lexed.comments, "com", self.ch["com"]["n"], self.ids)
            out["com"] = (h, first, last, None)
        if "text" in channels and lexed.texts:
            n = self.ch["text"]["n"]
            seqs = [(w, l) for w, l, _ in lexed.texts]
            h, first, last = grouped_hashes(seqs, "text", n, self.ids)
            labels = []
            for words, _l, label in lexed.texts:
                if len(words) >= n:
                    labels.extend([label] * (len(words) - n + 1))
            out["text"] = (h, first, last, labels)
        return out

    # -- evaluation --------------------------------------------------------

    def _evaluate(self, path, channel, fp, allow):
        hashes, first, last, labels = fp
        keep = np.ones(len(hashes), dtype=bool)
        for i in range(len(hashes)):
            for entry in allow:
                if entry.covers(path, channel, int(first[i]), int(last[i])):
                    keep[i] = False
                    break
        hashes, first, last = hashes[keep], first[keep], last[keep]
        if labels is not None:
            labels = [l for l, k in zip(labels, keep) if k]
        found, blobs, clines = self.index.lookup(channel, hashes)
        res = ChannelResult(channel, fingerprints=int(len(hashes)), matched=int(found.sum()))
        cfg = self.ch[channel]
        if channel in ("struct", "lex"):
            res.score = res.matched / res.fingerprints if res.fingerprints else 0.0
            if res.fingerprints >= cfg["min_fingerprints"] and res.score >= cfg["threshold"]:
                res.reasons.append(
                    f"{channel} {res.score:.0%} of {res.fingerprints} fingerprints match (threshold {cfg['threshold']:.0%})"
                )
            if channel == "struct":
                run, run_start = longest_true_run(found)
                res.longest_run = run
                if run >= cfg["consecutive"]:
                    res.reasons.append(
                        f"struct run of {run} consecutive matching fingerprints (lines {int(first[run_start])}-{int(last[run_start + run - 1])}; limit {cfg['consecutive']})"
                    )
        elif res.matched:
            n = cfg["n"]
            what = "comment" if channel == "com" else "text"
            res.reasons.append(f"{res.matched} {what} run(s) of {n}+ words match")
        if res.matched:
            res.regions = self._regions(found, first, last, blobs, clines, labels)
        return res

    def _regions(self, found, first, last, blobs, clines, labels):
        spans = sorted(
            (int(first[i]), int(last[i]), i) for i in np.flatnonzero(found)
        )
        regions = []
        members = []
        for s, e, i in spans:
            if regions and s <= regions[-1].end + 1:
                r = regions[-1]
                r.end = max(r.end, e)
                r.matched += 1
                members[-1].append(i)
            else:
                regions.append(Region(s, e, 1))
                members.append([i])
        for r, idx in zip(regions, members):
            if labels is not None:
                r.labels = sorted({labels[i] for i in idx})
            by_source = defaultdict(list)
            for i in idx:
                source, tag, path = self.index.blob(blobs[i])
                by_source[(source, path)].append((tag, int(clines[i])))
            ranked = sorted(by_source.items(), key=lambda kv: -len(kv[1]))[:3]
            for (source, path), hits in ranked:
                tags = Counter(t for t, _ in hits)
                lines = [l for _, l in hits]
                r.sources.append(
                    {
                        "source": source,
                        "path": path,
                        "tag": tags.most_common(1)[0][0],
                        "lines": [min(lines), max(lines)],
                        "matched": len(hits),
                    }
                )
        return regions

    def scan_file(self, path, text):
        lang = self.in_scope(path)
        result = FileResult(path, lang)
        channels = self.cfg["scan"]["channels_by_language"][lang]
        try:
            lexed = self.lexer.lex(text, lang, want_struct="struct" in channels)
        except ValueError as e:
            result.status, result.error = "error", f"cannot parse {path}: {e}"
            return result
        fps = self._fingerprints(lexed, channels)
        for channel in channels:
            fp = fps.get(channel)
            if fp is None:
                result.channels[channel] = ChannelResult(channel, score=0.0 if channel in ("struct", "lex") else None)
                continue
            effective = self._evaluate(path, channel, fp, self.allow)
            result.channels[channel] = effective
            if not effective.flagged and self.allow:
                raw = self._evaluate(path, channel, fp, [])
                if raw.flagged:
                    entries = sorted(
                        e.index for e in self.allow if any(e.overlaps(path, channel, r.start, r.end) for r in raw.regions)
                    )
                    result.allowlisted.append((channel, entries, raw))
        if any(c.flagged for c in result.channels.values()):
            result.status = "flagged"
        elif result.allowlisted:
            result.status = "allowlisted"
        return result


def read_text(root, path):
    full = os.path.join(root, path)
    with open(full, "rb") as f:
        return f.read().decode("utf-8", "replace")
