# wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
"""Pure-Python unit tests for the new compat-gate helpers. No Chrome/oracle.

Run: python3 -m unittest tests.compat.test_compare   (from wkhtmltox-rs/)
  or: cd tests/compat && python3 -m unittest test_compare
"""
import unittest
import compare


class OutlineTreeRatio(unittest.TestCase):
    def _toc(self, *items):
        # fitz TOC rows are [level, title, page]; page is ignored by the metric.
        return [[lvl, title, 1] for (lvl, title) in items]

    def test_identical_order_is_one(self):
        a = self._toc((1, "Intro"), (2, "Background"), (1, "Method"))
        self.assertEqual(compare.outline_tree_ratio(a, a), 1.0)

    def test_reordered_scores_below_one(self):
        a = self._toc((1, "Intro"), (1, "Method"))
        b = self._toc((1, "Method"), (1, "Intro"))
        # Set-intersection would call these identical; ordered must not.
        self.assertLess(compare.outline_tree_ratio(a, b), 1.0)

    def test_empty_both_is_one(self):
        self.assertEqual(compare.outline_tree_ratio([], []), 1.0)

    def test_empty_one_side_is_zero(self):
        self.assertEqual(compare.outline_tree_ratio(self._toc((1, "X")), []), 0.0)


class GateCheck(unittest.TestCase):
    def test_pass_when_all_meet_floor(self):
        m = {"mean_ssim": 0.80, "outline_ratio": 1.0, "abs_delta_pages": 0}
        t = {"mean_ssim": 0.70, "outline_ratio": 1.0, "max_abs_delta_pages": 1}
        self.assertEqual(compare.gate_check(m, t), [])

    def test_fail_lists_offending_metric(self):
        m = {"mean_ssim": 0.50, "outline_ratio": 1.0, "abs_delta_pages": 0}
        t = {"mean_ssim": 0.70, "outline_ratio": 1.0, "max_abs_delta_pages": 1}
        fails = compare.gate_check(m, t)
        self.assertEqual(len(fails), 1)
        self.assertIn("mean_ssim", fails[0])

    def test_fail_on_page_drift(self):
        m = {"mean_ssim": 0.9, "outline_ratio": 1.0, "abs_delta_pages": 3}
        t = {"mean_ssim": 0.7, "outline_ratio": 1.0, "max_abs_delta_pages": 1}
        self.assertTrue(any("pages" in f for f in compare.gate_check(m, t)))


if __name__ == "__main__":
    unittest.main()
