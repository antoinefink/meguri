"""Stable token ids, k-gram hashes and winnowing (numpy, wrapping uint64 arithmetic)."""

from __future__ import annotations

import zlib

import numpy as np
from numpy.lib.stride_tricks import sliding_window_view

_B = np.uint64(0x100000001B3)
_M1 = np.uint64(0xBF58476D1CE4E5B9)
_M2 = np.uint64(0x94D049BB133111EB)
_S30, _S27, _S31 = np.uint64(30), np.uint64(27), np.uint64(31)

# Each channel salts its token ids, so equal word sequences in different channels
# never share a hash.
CHANNEL_SALT = {"struct": 1, "lex": 2, "com": 3, "text": 4}


class TokenIds:
    """Maps token strings to stable 32-bit ids (crc32), salted per channel."""

    def __init__(self, channel: str):
        self.prefix = f"{CHANNEL_SALT[channel]}\x1f".encode()
        self.cache: dict[str, int] = {}

    def ids(self, tokens) -> np.ndarray:
        cache = self.cache
        prefix = self.prefix
        out = np.empty(len(tokens), dtype=np.uint64)
        for i, t in enumerate(tokens):
            v = cache.get(t)
            if v is None:
                v = zlib.crc32(prefix + t.encode("utf-8", "surrogatepass")) + 1
                cache[t] = v
            out[i] = v
        return out


class IdSpace:
    """Token-id maps per channel and, for `struct`, per language family. The
    corpus build and the scan must use the same scheme, so both go through this."""

    def __init__(self):
        self.maps: dict[tuple, TokenIds] = {}

    def get(self, channel: str, family: str = "") -> TokenIds:
        key = (channel, family)
        t = self.maps.get(key)
        if t is None:
            t = TokenIds(channel)
            if family:
                t.prefix = t.prefix + family.encode() + b"\x1f"
            self.maps[key] = t
        return t


def _mix(x: np.ndarray) -> np.ndarray:
    x = x ^ (x >> _S30)
    x = x * _M1
    x = x ^ (x >> _S27)
    x = x * _M2
    return x ^ (x >> _S31)


def kgram_hashes(ids: np.ndarray, k: int, groups: np.ndarray | None = None):
    """Hashes of every k-gram of `ids`. With `groups` (one group id per token,
    non-decreasing), k-grams that cross a group boundary are dropped.
    Returns (hashes, start positions)."""
    n = len(ids)
    if n < k:
        return np.empty(0, np.uint64), np.empty(0, np.int64)
    m = n - k + 1
    h = np.zeros(m, dtype=np.uint64)
    with np.errstate(over="ignore"):
        for j in range(k):
            h *= _B
            h += ids[j : j + m]
        h = _mix(h)
    starts = np.arange(m, dtype=np.int64)
    if groups is not None:
        keep = groups[:m] == groups[k - 1 :]
        h, starts = h[keep], starts[keep]
    return h, starts


def periodicity(ids: np.ndarray, k: int, max_period: int = 12) -> np.ndarray:
    """For each k-gram, the largest share of its positions j with
    ids[j] == ids[j + p], over periods p <= max_period. Lists (struct fields,
    column lists, row decoding) score near 1; ordinary code stays low."""
    n = len(ids)
    m = n - k + 1
    if m <= 0:
        return np.empty(0)
    best = np.zeros(m)
    for p in range(1, min(max_period, k - 1) + 1):
        eq = (ids[:-p] == ids[p:]).astype(np.int32)
        cs = np.concatenate(([0], np.cumsum(eq)))
        np.maximum(best, (cs[k - p : k - p + m] - cs[:m]) / (k - p), out=best)
    return best


def winnow(hashes: np.ndarray, w: int) -> np.ndarray:
    """Robust winnowing: indexes of the rightmost minimal hash in every window of
    `w` consecutive hashes, each index reported once, in order."""
    n = len(hashes)
    if n == 0:
        return np.empty(0, np.int64)
    if n <= w:
        return np.array([n - 1 - int(np.argmin(hashes[::-1]))], dtype=np.int64)
    win = sliding_window_view(hashes, w)
    pos = np.arange(n - w + 1, dtype=np.int64) + (w - 1 - np.argmin(win[:, ::-1], axis=1))
    return np.unique(pos)


def longest_true_run(mask: np.ndarray):
    """(length, start index) of the longest run of True values."""
    best_len, best_start, cur_len, cur_start = 0, 0, 0, 0
    for i, v in enumerate(mask):
        if v:
            if cur_len == 0:
                cur_start = i
            cur_len += 1
            if cur_len > best_len:
                best_len, best_start = cur_len, cur_start
        else:
            cur_len = 0
    return best_len, best_start
