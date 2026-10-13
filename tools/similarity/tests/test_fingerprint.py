"""k-gram hashing and winnowing properties."""

import random
import unittest

import numpy as np

from meguri_similarity.fingerprint import IdSpace, kgram_hashes, longest_true_run, winnow


class Hashing(unittest.TestCase):
    def test_equal_kgrams_hash_equal_and_positions_are_starts(self):
        ids = np.array([5, 6, 7, 5, 6, 7, 9], dtype=np.uint64)
        h, starts = kgram_hashes(ids, 3)
        self.assertEqual(list(starts), [0, 1, 2, 3, 4])
        self.assertEqual(h[0], h[3])
        self.assertNotEqual(h[0], h[1])

    def test_groups_drop_kgrams_across_boundaries(self):
        ids = np.arange(1, 9, dtype=np.uint64)
        groups = np.array([0, 0, 0, 1, 1, 1, 1, 1])
        _h, starts = kgram_hashes(ids, 3, groups)
        self.assertEqual(list(starts), [0, 3, 4, 5])

    def test_short_input_has_no_kgrams(self):
        h, starts = kgram_hashes(np.array([1, 2], dtype=np.uint64), 3)
        self.assertEqual(len(h), 0)
        self.assertEqual(len(starts), 0)

    def test_channels_and_families_never_share_ids(self):
        space = IdSpace()
        a = space.get("struct", "rust").ids(["I", "fn"])
        b = space.get("struct", "js").ids(["I", "fn"])
        c = space.get("lex").ids(["I", "fn"])
        self.assertTrue(set(a).isdisjoint(b))
        self.assertTrue(set(a).isdisjoint(c))
        self.assertEqual(list(a), list(IdSpace().get("struct", "rust").ids(["I", "fn"])))


class Winnowing(unittest.TestCase):
    def test_every_window_holds_a_selected_position(self):
        rng = np.random.default_rng(7)
        h = rng.integers(0, 2**63, size=500, dtype=np.uint64)
        w = 10
        sel = set(winnow(h, w).tolist())
        for i in range(len(h) - w + 1):
            self.assertTrue(sel & set(range(i, i + w)), f"window {i} has no fingerprint")

    def test_shared_run_of_w_plus_k_minus_1_tokens_shares_a_fingerprint(self):
        # The winnowing guarantee the struct channel relies on.
        rnd = random.Random(11)
        k, w = 40, 10
        for trial in range(30):
            shared = [rnd.randrange(1, 50) for _ in range(w + k - 1)]
            a = [rnd.randrange(1, 50) for _ in range(rnd.randrange(0, 80))] + shared
            b = shared + [rnd.randrange(1, 50) for _ in range(rnd.randrange(0, 80))]
            ha, _ = kgram_hashes(np.array(a, dtype=np.uint64), k)
            hb, _ = kgram_hashes(np.array(b, dtype=np.uint64), k)
            fa = set(ha[winnow(ha, w)].tolist())
            self.assertTrue(fa & set(hb.tolist()), f"trial {trial}")

    def test_short_sequences_select_the_rightmost_minimum(self):
        h = np.array([4, 1, 3, 1], dtype=np.uint64)
        self.assertEqual(list(winnow(h, 10)), [3])
        self.assertEqual(len(winnow(np.empty(0, np.uint64), 10)), 0)

    def test_longest_true_run(self):
        self.assertEqual(longest_true_run(np.array([1, 1, 0, 1, 1, 1, 0], dtype=bool)), (3, 3))
        self.assertEqual(longest_true_run(np.array([], dtype=bool)), (0, 0))


if __name__ == "__main__":
    unittest.main()
