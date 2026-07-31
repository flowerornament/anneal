#!/usr/bin/env python3

from __future__ import annotations

import sys
import unittest

sys.dont_write_bytecode = True

import release


INTRO = (
    "# Changelog\n\n"
    "All notable changes to `anneal` are documented in this file.\n\n"
)


class ChangelogBumpTests(unittest.TestCase):
    def test_repeated_bumps_keep_unreleased_above_newest_release(self) -> None:
        original = (
            INTRO
            + "## v0.2.0 - 2026-02-01\n\n"
            + "### Added\n\n"
            + "- Second release.\n\n"
            + "## Unreleased\n\n"
            + "### Added\n\n"
            + "- Deferred capability with a\n"
            + "  multiline explanation.\n\n"
            + "## v0.1.0 - 2026-01-01\n\n"
            + "### Added\n\n"
            + "- First release.\n\n"
        )

        first, first_pending = release.changelog_insert_entry_text(
            original, "0.3.0", "2026-03-01"
        )
        second, second_pending = release.changelog_insert_entry_text(
            first, "0.4.0", "2026-04-01"
        )
        repeated, repeated_pending = release.changelog_insert_entry_text(
            second, "0.4.0", "2026-04-01"
        )

        pending = ["Deferred capability with a multiline explanation."]
        self.assertEqual(first_pending, pending)
        self.assertEqual(second_pending, pending)
        self.assertEqual(repeated_pending, pending)
        self.assertEqual(repeated, second)
        self.assertLess(second.index("## Unreleased"), second.index("## v0.4.0"))
        self.assertLess(second.index("## v0.4.0"), second.index("## v0.3.0"))
        self.assertLess(second.index("## v0.3.0"), second.index("## v0.2.0"))
        self.assertLess(second.index("## v0.2.0"), second.index("## v0.1.0"))
        self.assertEqual(second.count("## Unreleased"), 1)
        self.assertEqual(second.count("- Deferred capability with a"), 1)
        self.assertIn(
            "- Deferred capability with a\n  multiline explanation.",
            second,
        )

    def test_nonempty_unreleased_warning_names_each_pending_entry(self) -> None:
        warning = release.unreleased_warning(
            "0.3.0",
            ["Scalar equations bind grounded expressions.", "Recursive rules fail fast."],
        )

        self.assertEqual(
            warning,
            "warning: CHANGELOG.md Unreleased still contains 2 entries after "
            "scaffolding v0.3.0:\n"
            "  - Scalar equations bind grounded expressions.\n"
            "  - Recursive rules fail fast.\n"
            "Review whether they belong in v0.3.0.",
        )

    def test_empty_unreleased_is_silent(self) -> None:
        text = INTRO + "## Unreleased\n\n## v0.1.0 - 2026-01-01\n\n- First.\n"

        updated, pending = release.changelog_insert_entry_text(
            text, "0.2.0", "2026-02-01"
        )

        self.assertEqual(pending, [])
        self.assertIsNone(release.unreleased_warning("0.2.0", pending))
        self.assertLess(updated.index("## Unreleased"), updated.index("## v0.2.0"))

    def test_missing_or_duplicate_unreleased_fails_loudly(self) -> None:
        with self.assertRaisesRegex(ValueError, "exactly one"):
            release.changelog_insert_entry_text(INTRO, "0.2.0", "2026-02-01")
        with self.assertRaisesRegex(ValueError, "exactly one"):
            release.changelog_insert_entry_text(
                INTRO + "## Unreleased\n\n## Unreleased\n\n",
                "0.2.0",
                "2026-02-01",
            )


if __name__ == "__main__":
    unittest.main()
