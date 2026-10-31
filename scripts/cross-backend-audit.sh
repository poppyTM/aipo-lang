#!/usr/bin/env bash
# Adversarial cross-backend sweep.
#
# The conformance corpus proves the backends agree on programs that pass. It says
# nothing about the cases where a backend quietly disagrees: a contract that one
# engine accepts and another rejects, a mutation that rolls back on one and not
# the other, a data structure that grows past its block. This sweep runs small,
# targeted programs through the stack VM, the register engine and Wasm and
# compares stdout *and* exit status.
#
# Add a case by dropping a `.aipo` file in the directory below. Each one should
# be small enough that a failure names one behaviour.
#
# Usage: scripts/cross-backend-audit.sh
set -uo pipefail
cd "$(dirname "$0")/.."

AIPO=${AIPO:-./target/debug/aipo}
CASES=${CASES:-"scripts/audit-cases"}
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

if [ ! -x "$AIPO" ]; then
  echo "cross-backend-audit: $AIPO not found; build it first (cargo build -p aipo-cli)" >&2
  exit 2
fi
if [ ! -d "$CASES" ]; then
  echo "cross-backend-audit: no case directory at $CASES" >&2
  exit 2
fi

checked=0
differed=0

for src in "$CASES"/*.aipo; do
  [ -f "$src" ] || continue
  base=$(basename "$src" .aipo)
  checked=$((checked + 1))

  vm=$("$AIPO" run "$src" 2>&1); vm_code=$?
  reg=$("$AIPO" run "$src" --engine=reg 2>&1); reg_code=$?
  wasm=$("$AIPO" run "$src" --wasm 2>&1); wasm_code=$?
  js=$("$AIPO" build "$src" --target js --out "$WORK/$base" >/dev/null 2>&1 \
    && (cd "$WORK/$base" && node app.js 2>&1))
  js_code=$?

  # Two backends agree when they both succeed with the same stdout, or when they
  # both fail. Fault text is engine-specific by design, so a shared failure counts
  # as agreement; what must not diverge is whether the program runs at all.
  agrees() {
    local code_a=$1 out_a=$2 code_b=$3 out_b=$4
    if [ "$code_a" -eq 0 ] && [ "$code_b" -eq 0 ]; then
      [ "$out_a" = "$out_b" ]
    else
      [ "$code_a" -ne 0 ] && [ "$code_b" -ne 0 ]
    fi
  }

  case_diff=""
  agrees "$vm_code" "$vm" "$reg_code" "$reg" || case_diff="$case_diff reg"
  agrees "$vm_code" "$vm" "$wasm_code" "$wasm" || case_diff="$case_diff wasm"
  agrees "$vm_code" "$vm" "$js_code" "$js" || case_diff="$case_diff js"

  if [ -n "$case_diff" ]; then
    differed=$((differed + 1))
    printf 'DIVERGES %s ->%s\n' "$base" "$case_diff"
    printf '  vm   (%d): %s\n' "$vm_code" "$(printf '%s' "$vm" | head -1)"
    case "$case_diff" in *reg*) printf '  reg  (%d): %s\n' "$reg_code" "$(printf '%s' "$reg" | head -1)";; esac
    case "$case_diff" in *wasm*) printf '  wasm (%d): %s\n' "$wasm_code" "$(printf '%s' "$wasm" | head -1)";; esac
    case "$case_diff" in *js*) printf '  js   (%d): %s\n' "$js_code" "$(printf '%s' "$js" | head -1)";; esac
  fi
done

printf '\nchecked=%d diverged=%d\n' "$checked" "$differed"
[ "$differed" -eq 0 ]
