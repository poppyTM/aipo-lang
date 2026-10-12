#!/usr/bin/env bash
# studies/fetch-refs.sh — baixa as linguagens de referência para FORA do repo.
#
# Uso:
#   ./studies/fetch-refs.sh                  # clona/atualiza em ~/aipo-refs
#   AIPO_REFS_DIR=/mnt/dados/refs ./studies/fetch-refs.sh
#
# Saída: $REFS_DIR/refs.lock.json com repo + branch + SHA pinado.
# Política: nada aqui entra no git do aipo-lang (fica fora da árvore).
set -euo pipefail

REFS_DIR="${AIPO_REFS_DIR:-$HOME/aipo-refs}"
mkdir -p "$REFS_DIR"

# name|url|branch|license|tier
REFS=(
  "lua|https://github.com/lua/lua.git|master|MIT (COPYRIGHT)|A"
  "luajit|https://github.com/LuaJIT/LuaJIT.git|v2.1|MIT/X custom|A"
  "wren|https://github.com/wren-lang/wren.git|main|MIT|A"
  "quickjs|https://github.com/quickjs-ng/quickjs.git|master|MIT|A"
  "luau|https://github.com/luau-lang/luau.git|master|MIT|B"
  "janet|https://github.com/janet-lang/janet.git|master|MIT|B"
  "wasm3|https://github.com/wasm3/wasm3.git|main|MIT|C"
  "wamr|https://github.com/wasm-micro-runtime/wasm-micro-runtime.git|main|Apache-2.0|C"
  "wasmi|https://github.com/wasmi-labs/wasmi.git|main|Apache-2.0|C"
)

LOCK="$REFS_DIR/refs.lock.json"
{
  printf '{\n  "pinned_at": "%s",\n  "refs_dir": "%s",\n  "refs": [\n' \
    "$(date -u +%FT%TZ)" "$REFS_DIR"
  first=1
  for entry in "${REFS[@]}"; do
    IFS='|' read -r name url branch license tier <<< "$entry"
    if [ -d "$REFS_DIR/$name/.git" ]; then
      echo "== $name: atualizando ($branch) ==" >&2
      git -C "$REFS_DIR/$name" fetch --depth 1 origin "$branch"
      git -C "$REFS_DIR/$name" checkout -q FETCH_HEAD
    else
      echo "== $name: clonando $url ($branch) ==" >&2
      git clone --depth 1 --branch "$branch" "$url" "$REFS_DIR/$name"
    fi
    sha="$(git -C "$REFS_DIR/$name" rev-parse HEAD)"
    [ "$first" -eq 1 ] || printf ',\n'
    first=0
    printf '    {"name": "%s", "url": "%s", "branch": "%s", "sha": "%s", "license": "%s", "tier": "%s"}' \
      "$name" "$url" "$branch" "$sha" "$license" "$tier"
  done
  printf '\n  ]\n}\n'
} > "$LOCK"

echo "OK — lock em $LOCK"
