#!/usr/bin/env python3
"""Fail implementation PRs that omit the versioned progress ledger.

Only file synchronization is machine-checked. Semantic correctness,
evidence freshness and affected task IDs still require human review.
"""
from __future__ import annotations

import re
import subprocess
import sys
from pathlib import PurePosixPath

LEDGER = "website/public/progress/tasks.json"
ROOT_FILES = {"Cargo.toml"}
SOURCE_DIRS = ("crates/", "packages/", "examples/", "fuzz/")
LANGUAGE_SUFFIXES = {".rs", ".aipo", ".c", ".h", ".cc", ".cpp", ".hpp", ".ts", ".tsx", ".js", ".mjs", ".json", ".toml"}
FIXTURE_SUFFIXES = LANGUAGE_SUFFIXES | {".code", ".stdout", ".stderr", ".aibc", ".wasm"}


def implementation_file(path: str) -> bool:
    if not path or path.startswith("/") or ".." in PurePosixPath(path).parts:
        return False
    if path in ROOT_FILES:
        return True
    suffix = PurePosixPath(path).suffix.lower()
    if path.startswith("docs/conformance/"):
        return suffix in FIXTURE_SUFFIXES
    return path.startswith(SOURCE_DIRS) and suffix in LANGUAGE_SUFFIXES


def requires_ledger(paths: list[str]) -> tuple[bool, list[str]]:
    changes = sorted({path for path in paths if implementation_file(path)})
    return bool(changes) and LEDGER not in paths, changes


def main(args: list[str]) -> int:
    if len(args) != 2 or any(not re.fullmatch(r"[0-9a-f]{40}", value) for value in args):
        print("Usage: progress_delta.py <base-sha> <head-sha>", file=sys.stderr)
        return 2
    base, head = args
    try:
        result = subprocess.run(
            ["git", "diff", "--name-only", "--diff-filter=ACMRT", f"{base}...{head}"],
            check=True, capture_output=True, text=True,
        )
    except subprocess.CalledProcessError as error:
        print(f"Cannot inspect PR diff: {error.stderr.strip()}", file=sys.stderr)
        return 2
    changed = result.stdout.splitlines()
    required, implementation = requires_ledger(changed)
    if required:
        print("FAIL: implementation files changed without website/public/progress/tasks.json", file=sys.stderr)
        print("Affected examples:", ", ".join(implementation[:12]), file=sys.stderr)
        print("Update relevant IDs/checkpoints/gates/evidence in the same PR, even if status stays unchanged.", file=sys.stderr)
        return 1
    if implementation:
        print(f"OK: progress ledger changed alongside {len(implementation)} implementation file(s).")
        print("NOTE: this policy validates file synchronization, not factual proof or task coverage.")
    else:
        print("OK: no behavioral implementation files in this PR; progress ledger not required.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
