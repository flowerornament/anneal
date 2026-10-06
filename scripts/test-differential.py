#!/usr/bin/env python3
"""Adversarial controls: expected failing comparisons must actually refuse/differ."""

import json
import shlex
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.dont_write_bytecode = True

import differential


class DifferentialControls(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.root = self.base / "corpus"
        self.root.mkdir()
        (self.root / "fixture.md").write_text("# A pinned input\n")
        self.counter = 0

    def binary(self, name, output, code=0, extra=""):
        path = self.base / name
        path.write_text(
            "#!/bin/sh\n"
            + extra
            + "\nprintf %s "
            + shlex.quote(output)
            + "\nprintf diagnostic >&2\nexit "
            + str(code)
            + "\n"
        )
        path.chmod(0o755)
        return path

    def run_pair(self, left, right, options=()):
        self.counter += 1
        output = self.base / f"receipt-{self.counter}"
        command = [
            sys.executable,
            str(Path(differential.__file__)),
            "--left",
            str(left),
            "--right",
            str(right),
            "--root",
            str(self.root),
            "--output",
            str(output),
            *options,
        ]
        result = subprocess.run(
            command, capture_output=True, text=True, check=False, timeout=30
        )
        receipt = json.loads((output / "receipt.json").read_text())
        return result.returncode, receipt

    def test_both_empty_are_refusals_not_equal_artifacts(self):
        left = self.binary("left", "")
        right = self.binary("right", "")
        code, receipt = self.run_pair(left, right)
        self.assertEqual(code, 2)
        self.assertEqual(receipt["status"], "refused")
        self.assertEqual([x["artifact"] for x in receipt["sides"]], [None, None])
        self.assertNotEqual(receipt["sides"][0], receipt["sides"][1])
        self.assertTrue(all(x["refusals"] for x in receipt["sides"]))

    def test_nonzero_stdout_is_not_a_success_and_stderr_is_captured(self):
        left = self.binary("left", '{"a":1}\n', code=7)
        right = self.binary("right", '{"a":1}\n')
        code, receipt = self.run_pair(left, right)
        self.assertEqual(code, 2)
        failed = receipt["sides"][0]
        self.assertEqual(failed["side"], "left")
        self.assertEqual(failed["exit_status"], 7)
        self.assertIsNone(failed["artifact"])
        self.assertEqual(Path(failed["stderr_path"]).read_text(), "diagnostic")
        self.assertEqual(receipt["sides"][1]["artifact"]["row_count"], 1)

    def test_identical_hash_different_row_count_goes_red(self):
        # Construct the otherwise hard-to-produce collision requested by the bead.
        a = differential.NonEmptyArtifact("ndjson", (), 1, "f" * 64)
        b = differential.NonEmptyArtifact("ndjson", (), 2, "f" * 64)
        self.assertFalse(differential.compare(a, b))
        with self.assertRaisesRegex(TypeError, "nonempty"):
            differential.compare(None, None)
        with self.assertRaisesRegex(ValueError, "positive"):
            differential.NonEmptyArtifact("ndjson", (), 0, "f" * 64)

    def test_undeclared_exclusion_is_a_difference(self):
        left = self.binary("left", '{"identity":{"native_id":"old"},"value":2}\n')
        right = self.binary("right", '{"identity":{"native_id":"new"},"value":2}\n')
        code, receipt = self.run_pair(left, right)
        self.assertEqual(code, 1)
        self.assertEqual(receipt["status"], "different")
        self.assertEqual(receipt["declared_exclusions"], [])
        code, receipt = self.run_pair(left, right, ["--exclude", "/identity/native_id"])
        self.assertEqual(code, 0)
        self.assertEqual(receipt["declared_exclusions"], ["/identity/native_id"])
        self.assertTrue(
            all(
                x["removed_fields"] == {"/identity/native_id": 1}
                for x in receipt["sides"]
            )
        )

    def test_complete_rows_sorted_without_losing_duplicates(self):
        a = self.binary("left", '{"z":2,"a":1}\n{"a":3}\n{"a":3}\n')
        b = self.binary("right", '{"a":3}\n{"a":1,"z":2}\n{"a":3}\n')
        code, receipt = self.run_pair(a, b)
        self.assertEqual(code, 0)
        self.assertTrue(all(x["artifact"]["row_count"] == 3 for x in receipt["sides"]))
        b = self.binary("right", '{"a":3}\n{"a":1,"z":2}\n')
        code, receipt = self.run_pair(a, b)
        self.assertEqual(code, 1)
        self.assertEqual([x["artifact"]["row_count"] for x in receipt["sides"]], [3, 2])

    def test_numeric_differences_are_not_rounded_away(self):
        a = self.binary("left", '{"number":1.00000000000000001}\n')
        b = self.binary("right", '{"number":1.00000000000000002}\n')
        self.assertEqual(self.run_pair(a, b)[0], 1)

    def test_raw_bytes_preserve_order_and_whitespace(self):
        a = self.binary("left", "alpha\nbeta\n")
        b = self.binary("right", "beta\nalpha\n")
        code, receipt = self.run_pair(a, b, ["--mode", "raw"])
        self.assertEqual(code, 1)
        self.assertTrue(all(x["artifact"]["row_count"] == 2 for x in receipt["sides"]))
        b = self.binary("right", "alpha\nbeta\n")
        self.assertEqual(self.run_pair(a, b, ["--mode", "raw"])[0], 0)

    def test_bad_json_and_blank_ndjson_refuse(self):
        for data in ['{"x":1,"x":2}\n', '{"x":NaN}\n', "\n \n", "[]\n", "no\n"]:
            a = self.binary("left", data)
            b = self.binary("right", '{"ok":1}\n')
            self.assertEqual(self.run_pair(a, b)[0], 2)

    def test_corpus_mutation_refuses_even_if_output_matches(self):
        a = self.binary("left", '{"ok":1}\n', extra="printf changed > fixture.md")
        b = self.binary("right", '{"ok":1}\n')
        code, receipt = self.run_pair(a, b)
        self.assertEqual(code, 2)
        self.assertIn(
            "corpus content changed", " ".join(receipt["sides"][0]["refusals"])
        )
        self.assertIsNone(receipt["sides"][1]["artifact"])

    def test_missing_binary_names_the_refused_side(self):
        b = self.binary("right", '{"ok":1}\n')
        code, receipt = self.run_pair(self.base / "missing", b)
        self.assertEqual(code, 2)
        self.assertEqual(receipt["sides"][0]["side"], "left")
        self.assertTrue(receipt["sides"][0]["refusals"])
        self.assertIsNotNone(receipt["sides"][1]["artifact"])

    def test_timeout_is_a_refusal(self):
        a = self.binary("left", '{"ok":1}\n', extra="sleep 0.2")
        b = self.binary("right", '{"ok":1}\n')
        code, receipt = self.run_pair(a, b, ["--timeout", "0.01"])
        self.assertEqual(code, 2)
        self.assertIn("timed out", " ".join(receipt["sides"][0]["refusals"]))


if __name__ == "__main__":
    unittest.main()
