#!/usr/bin/env python3
"""Fetch pinned study sources outside Aipo; --verify performs offline inspection."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent


def git(path, *args):
    result = subprocess.run(["git", "-C", str(path), *args], check=True,
                            text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    return result.stdout.strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dest", type=Path, default=Path(os.environ.get("AIPO_REFS_DIR", str(Path.home() / "aipo-refs"))))
    parser.add_argument("--verify", action="store_true", help="Inspect existing HEADs, origins and lock without fetching")
    args = parser.parse_args()
    if args.dest.is_symlink():
        raise ValueError("reference destination must not be a symlink")
    destination = args.dest.resolve()
    if destination == ROOT or ROOT in destination.parents:
        raise ValueError("reference clones must stay outside the Aipo repository")
    manifest = json.loads((ROOT / "studies/refs.json").read_text())
    refs = manifest["refs"]
    names = set()
    for ref in refs:
        if not re.fullmatch(r"[a-z][a-z0-9_-]*", ref["name"]) or ref["name"] in names:
            raise ValueError("invalid or duplicate reference name")
        if not re.fullmatch(r"[0-9a-f]{40}", ref["sha"]):
            raise ValueError("references require full lowercase commit SHAs")
        if not re.fullmatch(r"https://github\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+\.git", ref["url"]):
            raise ValueError("references require explicit public GitHub URLs")
        names.add(ref["name"])
    lock = destination / "refs.lock.json"
    if args.verify:
        if json.loads(lock.read_text()) != manifest:
            raise ValueError("reference lock differs from the committed manifest")
    else:
        destination.mkdir(parents=True, exist_ok=True)
    for ref in refs:
        clone = destination / ref["name"]
        if clone.is_symlink():
            raise ValueError(f"{ref['name']}: clone must not be a symlink")
        if not clone.exists():
            if args.verify:
                raise ValueError(f"{ref['name']}: clone is missing")
            clone.mkdir()
            git(clone, "init", "--quiet")
            git(clone, "remote", "add", "origin", ref["url"])
        if git(clone, "remote", "get-url", "origin") != ref["url"]:
            raise ValueError(f"{ref['name']}: unexpected origin; refusing to overwrite")
        if git(clone, "status", "--porcelain"):
            raise ValueError(f"{ref['name']}: working tree is dirty; refusing to overwrite")
        if not args.verify:
            git(clone, "fetch", "--depth=1", "origin", ref["sha"])
            git(clone, "checkout", "--quiet", "--detach", ref["sha"])
        if git(clone, "rev-parse", "HEAD") != ref["sha"]:
            raise ValueError(f"{ref['name']}: HEAD does not match the pinned commit")
        if git(clone, "status", "--porcelain"):
            raise ValueError(f"{ref['name']}: working tree is not clean")
        print(f"{ref['name']}: {ref['sha']}", flush=True)
    if not args.verify:
        temporary = lock.with_suffix(".json.tmp")
        temporary.write_text(json.dumps(manifest, indent=2) + "\n")
        temporary.replace(lock)
    print("Reference lock and sources match the manifest.")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        print(f"Reference retrieval failed: {error}", file=sys.stderr)
        if isinstance(error, subprocess.CalledProcessError):
            print(error.stderr[:1200], file=sys.stderr)
        sys.exit(1)
