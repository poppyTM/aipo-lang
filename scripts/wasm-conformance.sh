#!/usr/bin/env bash
# Wasm conformance sweep: compiles and runs every paired fixture in
# docs/conformance/programs through the Wasm backend and diffs stdout against
# the committed `.stdout`. Prints one line per fixture: PASS / DIFF / ERROR.
set -uo pipefail
cd "$(dirname "$0")/.."

AIPO=${AIPO:-./target/debug/aipo}
OUT=$(mktemp -d)
pass=0; diffn=0; errn=0

for src in docs/conformance/programs/*.aipo; do
  base=$(basename "$src" .aipo)
  expected="docs/conformance/programs/$base.stdout"
  [ -f "$expected" ] || continue
  "$AIPO" run "$src" --wasm >"$OUT/$base.out" 2>"$OUT/$base.err"
  code=$?
  if [ $code -ne 0 ]; then
    printf 'ERROR %s :: %s\n' "$base" "$(head -1 "$OUT/$base.err")"
    errn=$((errn+1))
  elif ! diff -q "$OUT/$base.out" "$expected" >/dev/null; then
    printf 'DIFF  %s\n' "$base"
    diffn=$((diffn+1))
  else
    pass=$((pass+1))
  fi
done

printf '\npass=%d diff=%d error=%d total=%d\n' "$pass" "$diffn" "$errn" "$((pass+diffn+errn))"
[ "${VERBOSE:-0}" = "1" ] && { echo; echo "artifacts: $OUT"; } || rm -rf "$OUT"
