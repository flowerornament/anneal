"""Validated direct-Git checkout boundary for repository instruments.

jj-only desks must never inherit an ancestor's repository. Product code uses
RepositoryContext; these dependency-free scripts share this boundary instead.
"""

from __future__ import annotations

import os
import subprocess
from pathlib import Path


class GitCheckoutError(RuntimeError):
    """The requested root does not identify its own Git checkout."""


class GitCheckout:
    def __init__(self, root: Path, *, colocated: bool = False) -> None:
        self.root = root.resolve()
        hint = (
            f"refusing Git at {self.root}; use the colocated checkout "
            f"(expected {'.git and .jj' if colocated else '.git'} at {self.root})"
        )
        if not (self.root / ".git").exists() or (
            colocated and not (self.root / ".jj").exists()
        ):
            raise GitCheckoutError(hint)
        probe = self.run(["rev-parse", "--show-toplevel"])
        if probe.returncode or Path(probe.stdout.strip()).resolve() != self.root:
            raise GitCheckoutError(f"{hint}; reported toplevel: {probe.stdout.strip()!r}")

    @classmethod
    def discover(cls, path: Path) -> GitCheckout:
        path = path.resolve()
        if path.is_file():
            path = path.parent
        for boundary in (path, *path.parents):
            if any((boundary / marker).exists() for marker in (
                ".git", ".jj", "Cargo.toml", "mix.exs", "package.json", "pyproject.toml", "go.mod"
            )):
                return cls(boundary)
        raise GitCheckoutError(f"no Git checkout boundary for {path}; use the colocated checkout")

    def run(self, args: list[str], **kwargs) -> subprocess.CompletedProcess[str]:
        env = os.environ.copy()
        for key in ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_COMMON_DIR"):
            env.pop(key, None)
        env["GIT_OPTIONAL_LOCKS"] = "0"
        options = {"check": False, "capture_output": True, "text": True, "timeout": 30}
        options.update(kwargs)
        return subprocess.run(["git", "-C", str(self.root), *args], cwd=self.root, env=env, **options)
