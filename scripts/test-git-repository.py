#!/usr/bin/env python3
"""Offline boundary controls, including negative controls for the source census."""

from __future__ import annotations

import importlib.util
import io
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from contextlib import redirect_stderr, redirect_stdout
from unittest.mock import patch

sys.dont_write_bytecode = True

from git_repository import GitCheckout, GitCheckoutError
import release

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("git_census", ROOT / "scripts/check-git-calls.py")
census = importlib.util.module_from_spec(spec)
spec.loader.exec_module(census)


def fixture_git(root: Path, *args: str) -> str:
    env = os.environ.copy()
    for key in ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_COMMON_DIR"):
        env.pop(key, None)
    return subprocess.run(["git", "-C", str(root), *args], env=env, check=True,
                          capture_output=True, text=True).stdout.strip()


class BoundaryTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        fixture_git(self.root, "init", "--quiet")

    def test_valid_checkout_scrubs_overrides_on_probe_and_command(self) -> None:
        (self.root / "tracked").write_text("tracked\n")
        fixture_git(self.root, "add", "tracked")
        overrides = {key: str(self.root / "wrong") for key in (
            "GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_COMMON_DIR")}
        with patch.dict(os.environ, overrides):
            checkout = GitCheckout(self.root)
            self.assertEqual(checkout.run(["ls-files"], check=True).stdout.strip(), "tracked")
            self.assertEqual(checkout.run(["rev-parse", "--show-toplevel"]).stdout.strip(), str(self.root))

    def test_discovery_stops_at_jj_and_package_boundaries(self) -> None:
        for marker in (".jj", "Cargo.toml", "mix.exs", "package.json", "pyproject.toml", "go.mod"):
            desk = self.root / marker.replace(".", "_")
            corpus = desk / "docs"
            corpus.mkdir(parents=True)
            if marker == ".jj":
                (desk / marker).mkdir()
            else:
                (desk / marker).touch()
            with self.assertRaisesRegex(GitCheckoutError, "colocated checkout") as caught:
                GitCheckout.discover(corpus)
            self.assertIn(str(desk), str(caught.exception))

    def test_nested_plain_git_corpus_discovers_its_validated_boundary(self) -> None:
        corpus = self.root / "docs"
        corpus.mkdir()
        self.assertEqual(GitCheckout.discover(corpus).root, self.root)
        with self.assertRaises(GitCheckoutError):
            GitCheckout(corpus)

    def test_release_tag_requires_colocation_before_any_git_or_nix(self) -> None:
        with patch.object(release, "ROOT", self.root), patch.object(
            release.subprocess, "run", side_effect=AssertionError("must not launch")
        ), self.assertRaisesRegex(GitCheckoutError, ".git and .jj"):
            release.tag("0.27.0")

    def test_colocated_checkout_is_accepted(self) -> None:
        (self.root / ".jj").mkdir()
        self.assertEqual(GitCheckout(self.root, colocated=True).root, self.root)

    def test_shell_refuses_ancestor_and_scrubs_valid_checkout(self) -> None:
        helper = ROOT / "scripts/git-repository.sh"
        desk = self.root / "desk"
        desk.mkdir()
        env = os.environ.copy()
        env.update({key: str(self.root / "wrong") for key in (
            "GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_COMMON_DIR")})
        command = ['bash', '-c', 'source "$1"; require_git_root "$2"', '--', str(helper)]
        valid = subprocess.run([*command, str(self.root)], env=env, capture_output=True, text=True)
        self.assertEqual(valid.returncode, 0, valid.stderr)
        refused = subprocess.run([*command, str(desk)], env=env, capture_output=True, text=True)
        self.assertNotEqual(refused.returncode, 0)
        self.assertIn(str(desk), refused.stderr)
        self.assertIn("colocated checkout", refused.stderr)


class CensusTests(unittest.TestCase):
    def test_gate_exit_rejects_each_unblessed_language_launch(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            helpers = {
                "crates/anneal-core/src/repository.rs": 'fn git_command() { Command::new("git"); }',
                "xtask/src/atlas.rs": 'fn git() { Command::new("git"); }',
                "scripts/git_repository.py": 'def run():\n return ["git", "status"]',
                "scripts/git-repository.sh": 'git_at_root() {\n git status\n}',
            }
            for name, text in helpers.items():
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(text)
            with patch.object(census, "ROOT", root), redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
                self.assertEqual(census.main(), 0, "positive four-builder instrument")
                for name, source in {
                    "crates/bad.rs": 'fn bad() { Command::new("git"); }',
                    "scripts/bad.py": 'subprocess.run(["git", "status"])',
                    "scripts/bad.sh": 'git status',
                }.items():
                    path = root / name
                    path.write_text(source)
                    self.assertNotEqual(census.main(), 0, f"{name} must turn the real gate red")
                    path.unlink()

    def test_real_census_has_exactly_four_builders(self) -> None:
        allowed, refused = census.census(ROOT)
        self.assertEqual(refused, [])
        self.assertEqual(len(allowed), 4)

    def test_rust_test_attribute_does_not_hide_later_production(self) -> None:
        source = '''struct S { #[cfg(test)] field: usize }
#[cfg(test)] mod tests { fn fixture() { Command::new("git"); } }
fn production() { Command::new("git"); }
'''
        self.assertEqual(census.rust_calls("crates/adapter.rs", source), [(3, False)])

    def test_new_git_launches_in_all_three_languages_go_red(self) -> None:
        self.assertEqual(census.rust_calls("crates/adapter.rs", 'fn bad() { Command::new("git"); }'), [(1, False)])
        self.assertEqual(census.python_calls("scripts/bad.py", 'subprocess.run(["git", "status"])'), [(1, False)])
        self.assertEqual(census.shell_calls("scripts/bad.sh", 'ROOT="$(git rev-parse --show-toplevel)"'), [(1, False)])

    def test_blessed_file_does_not_bless_another_function(self) -> None:
        self.assertEqual(census.rust_calls("xtask/src/atlas.rs", 'fn bad() { Command::new("git"); }'), [(1, False)])
        self.assertEqual(census.python_calls("scripts/git_repository.py", 'def bad():\n return ["git", "status"]'), [(2, False)])
        self.assertEqual(census.shell_calls("scripts/git-repository.sh", 'bad() {\n git status\n}'), [(2, False)])

    def test_publisher_allows_jj_git_but_cannot_hide_a_raw_git_launch(self) -> None:
        self.assertEqual(census.shell_calls("scripts/jj-land.sh", 'jj git push --bookmark master'), [])
        self.assertEqual(census.shell_calls("scripts/jj-land.sh", 'jj git fetch; git status'), [(1, False)])


if __name__ == "__main__":
    unittest.main()
