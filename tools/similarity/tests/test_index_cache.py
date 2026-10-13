"""The index cache under concurrent scans and its eviction (no Anki source involved).

Gate scans of different branches run at the same time and, with their own
interop vocabularies, need different index keys. Each must build, publish and
open its index without breaking another one's.
"""

import json
import os
import shutil
import subprocess
import sys
import time
import unittest

from support import PKG_ROOT, make_corpus, synthetic_config, tmpdir, write_tree

FILES = 400


def generated_rust(i):
    """A corpus file written for these tests: enough code that index builds
    take long enough to overlap."""
    out = []
    for j in range(30):
        out += [
            f"pub fn fold_{i}_{j}(seed_{j}: u32, values: &[u64]) -> u64 {{",
            f"    // bucket {j} of file {i}: fold every value whose index divides evenly",
            f"    let mut total_{i} = {j} as u64;",
            f"    for (k, v) in values.iter().enumerate() {{ if k % {j + 2} == 0 {{ total_{i} += v * {i + j}; }} }}",
            f"    total_{i} + seed_{j} as u64",
            "}",
        ]
    return "\n".join(out) + "\n"


def vocab_root(base, word):
    root = os.path.join(base, f"vocab-{word}")
    write_tree(root, {"specs/test/interop-vocab.txt": word + "\n"})
    return root


def scan_command(refs, cache, root, out, config, vocab):
    return [
        sys.executable, "-m", "meguri_similarity", "scan", "--refs", refs, "--cache", cache, "--root", root,
        "--out", out, "--config", config, "--vocab-root", vocab, "core/own.rs",
    ]


class IndexCache(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.base = tmpdir()
        cls.refs = make_corpus(
            os.path.join(cls.base, "refs"),
            [("2.1.51", {f"rslib/src/gen/m{i}.rs": generated_rust(i) for i in range(FILES)})],
        )
        cls.config = synthetic_config(cls.base, "2.1.51", "2.1.51", 1)
        cls.root = write_tree(os.path.join(cls.base, "meguri"), {"core/own.rs": "pub fn own() -> u8 { 7 }\n"})

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.base, ignore_errors=True)

    def setUp(self):
        self.work = tmpdir()
        self.cache = os.path.join(self.work, "cache")

    def tearDown(self):
        shutil.rmtree(self.work, ignore_errors=True)

    def run_scans(self, vocabs):
        """Starts one scan per vocabulary root at once; returns [(exit code, summary, stderr)]."""
        env = dict(os.environ, PYTHONPATH=PKG_ROOT)
        procs = []
        for n, vocab in enumerate(vocabs):
            out = os.path.join(self.work, f"out{n}")
            cmd = scan_command(self.refs, self.cache, self.root, out, self.config, vocab)
            procs.append((out, subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env)))
        results = []
        for out, p in procs:
            _stdout, err = p.communicate(timeout=300)
            summary = {}
            if os.path.exists(os.path.join(out, "summary.json")):
                with open(os.path.join(out, "summary.json")) as f:
                    summary = json.load(f)
            results.append((p.returncode, summary, err.strip().splitlines()[-1:]))
        return results

    def published(self):
        return sorted(n for n in os.listdir(self.cache) if not n.startswith("."))

    def leftovers(self):
        return sorted(n for n in os.listdir(self.cache) if n.startswith(".") and n != ".lock")

    def test_concurrent_scans_with_different_index_keys_all_succeed(self):
        vocabs = [vocab_root(self.work, f"word{n}") for n in range(6)]
        results = self.run_scans(vocabs)
        self.assertEqual([r[0] for r in results], [0] * 6, [r[2] for r in results])
        self.assertEqual({r[1]["status"] for r in results}, {"pass"})
        keys = {r[1]["corpus"]["key"] for r in results}
        self.assertEqual(len(keys), 6)
        self.assertEqual(self.leftovers(), [])
        self.assertEqual(len(self.published()), 3)

    def test_concurrent_scans_with_the_same_index_key_all_succeed(self):
        vocab = vocab_root(self.work, "shared")
        results = self.run_scans([vocab] * 6)
        self.assertEqual([r[0] for r in results], [0] * 6, [r[2] for r in results])
        self.assertEqual(len({r[1]["corpus"]["key"] for r in results}), 1)
        self.assertEqual(self.published(), [results[0][1]["corpus"]["key"]])
        self.assertEqual(self.leftovers(), [])

    def test_the_cache_keeps_the_three_most_recently_used_indexes(self):
        keys = []
        for n in range(5):
            [(code, summary, err)] = self.run_scans([vocab_root(self.work, f"seq{n}")])
            self.assertEqual(code, 0, err)
            keys.append(summary["corpus"]["key"])
            time.sleep(0.05)  # distinct modification times
        self.assertEqual(self.published(), sorted(keys[2:]))
        # Using an index makes it recent again: the next build evicts seq3's, not seq2's.
        [(code, summary, err)] = self.run_scans([vocab_root(self.work, "seq2")])
        self.assertEqual((code, summary["corpus"]["built"]), (0, False), err)
        time.sleep(0.05)
        [(code, summary, err)] = self.run_scans([vocab_root(self.work, "seq5")])
        self.assertEqual(code, 0, err)
        self.assertEqual(self.published(), sorted([keys[2], keys[4], summary["corpus"]["key"]]))

    def test_a_stale_build_directory_is_removed_and_a_fresh_one_kept(self):
        os.makedirs(self.cache)
        stale = os.path.join(self.cache, ".tmp-stale")
        fresh = os.path.join(self.cache, ".tmp-fresh")
        os.makedirs(stale)
        os.makedirs(fresh)
        old = time.time() - 2 * 3600
        os.utime(stale, (old, old))
        [(code, _summary, err)] = self.run_scans([vocab_root(self.work, "one")])
        self.assertEqual(code, 0, err)
        self.assertEqual(self.leftovers(), [".tmp-fresh"])


if __name__ == "__main__":
    unittest.main()
