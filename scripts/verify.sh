#!/usr/bin/env bash
# Re-lock every local Aipo package whose sources changed, then run the
# verification gates. The package digest covers source files, so any edit
# under packages/*/src invalidates aipo.lock until it is regenerated.
#
# `--all` widens the scope to the whole workspace. The default stays narrow only
# to keep the inner loop fast; every crate in the workspace is expected to pass,
# including `aipo-c-abi`, whose P0 boundary tests were fixed and are green now.
set -euo pipefail

cd "$(dirname "$0")/.."

SCOPE=(-p aipo-game-host -p aipo-cli)
if [[ "${1:-}" == "--all" ]]; then
  SCOPE=(--workspace)
fi

echo "==> Re-locking local Aipo packages"
for manifest in packages/*/aipo.toml; do
  pkg_dir="$(dirname "$manifest")"
  cargo run -q -p aipo-cli --bin aipo -- package lock "$pkg_dir" >/dev/null
  echo "    locked $pkg_dir"
done

echo "==> cargo fmt"
cargo fmt --all
cargo fmt --all --check

echo "==> cargo clippy"
cargo clippy --all-targets "${SCOPE[@]}" -- -D warnings

echo "==> cargo test"
cargo test "${SCOPE[@]}"

# The canonical examples of the `lang-aipo` skill are what an assistant copies from,
# so a drift there propagates into generated code. The skill's own linter is only
# lexical; this runs the compiler over every example (`check` + `test`/`run`).
echo "==> skill examples (lang-aipo)"
cargo build -q -p aipo-cli --bin aipo
AIPO_BIN="${PWD}/target/debug/aipo" bash skills/lang-aipo/scripts/verify.sh

echo "==> docs build"
npm run --silent docs:build >/dev/null

echo "OK"
