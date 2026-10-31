#!/usr/bin/env bash
# Contract audit: properties of the frontend and the formatter that no unit test
# asserts directly, because they are statements about a whole corpus rather than
# about one function.
#
#  1. Formatter idempotence — formatting an already-formatted file must not change
#     it. A formatter that oscillates makes `aipo fmt --check` unusable.
#  2. Formatter semantic fidelity — a formatted program must run to the same
#     stdout as its original. Formatting is presentation; changing output is a
#     formatter bug, not a style choice.
#
# Usage: scripts/contract-audit.sh [--verbose]
set -uo pipefail
cd "$(dirname "$0")/.."

AIPO=${AIPO:-./target/debug/aipo}
VERBOSE=${VERBOSE:-0}
[ "${1:-}" = "--verbose" ] && VERBOSE=1

if [ ! -x "$AIPO" ]; then
  echo "contract-audit: $AIPO not found; build it first (cargo build -p aipo-cli)" >&2
  exit 2
fi

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

fail_idem=0
fail_sem=0
checked=0

for src in docs/conformance/programs/*.aipo examples/*.aipo; do
  base=$(basename "$src" .aipo)
  checked=$((checked + 1))

  # 1. Format twice; the second pass must be a no-op.
  cp "$src" "$WORK/once.aipo"
  "$AIPO" fmt "$WORK/once.aipo" >/dev/null 2>&1
  cp "$WORK/once.aipo" "$WORK/twice.aipo"
  "$AIPO" fmt "$WORK/twice.aipo" >/dev/null 2>&1
  if ! cmp -s "$WORK/once.aipo" "$WORK/twice.aipo"; then
    echo "NOT-IDEMPOTENT $base"
    fail_idem=$((fail_idem + 1))
  fi

  # 2. A paired fixture must still produce its committed stdout after formatting.
  expected="docs/conformance/programs/$base.stdout"
  [ -f "$expected" ] || continue
  actual=$("$AIPO" run "$WORK/once.aipo" 2>&1)
  if [ "$actual" != "$(cat "$expected")" ]; then
    echo "SEMANTIC-DRIFT $base"
    fail_sem=$((fail_sem + 1))
  fi
done

printf '\nchecked=%d not-idempotent=%d semantic-drift=%d\n' \
  "$checked" "$fail_idem" "$fail_sem"

[ "$VERBOSE" = "1" ] && echo "(work dir removed on exit)"
[ $((fail_idem + fail_sem)) -eq 0 ]
