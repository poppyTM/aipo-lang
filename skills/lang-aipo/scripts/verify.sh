#!/usr/bin/env bash
#
# Gate dos exemplos canônicos da skill `lang-aipo`.
#
# O linter lexical pega uma classe de alucinação (tokens que não existem), mas NÃO
# pega drift contra a gramática real nem contra a biblioteca padrão real: os
# exemplos já ensinaram `import io`, `sys.args()`, `fn main()` e `return match …`,
# nenhum dos quais o compilador aceita. Por isso este script roda o **compilador
# de verdade** em cada exemplo:
#
#   - `aipo check`  → frontend, semântica e verificação de bytecode;
#   - `aipo test`   → exemplos com `#!test` (testes isolados);
#   - `aipo run`    → os demais exemplos, cujo topo do módulo é o programa.
#
# Compilador ausente é FALHA, não skip: skip é exatamente como o drift passou
# despercebido até aqui. Quando o binário não está no PATH, aponte AIPO_BIN.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SKILL_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
EXAMPLES="${SKILL_ROOT}/examples"
AIPO_BIN="${AIPO_BIN:-aipo}"

if ! command -v "${AIPO_BIN}" >/dev/null 2>&1; then
    echo "erro: compilador '${AIPO_BIN}' não encontrado no PATH." >&2
    echo "      instale-o ou defina AIPO_BIN=/caminho/para/aipo" >&2
    exit 1
fi

echo "=== Linter lexical (tokens proibidos) ==="
python3 "${SCRIPT_DIR}/lint_aipo.py" "${EXAMPLES}"

fail=0

echo
echo "=== Compilador (${AIPO_BIN}): check ==="
for example in "${EXAMPLES}"/*.aipo; do
    name="$(basename "${example}")"
    if out="$("${AIPO_BIN}" check "${example}" 2>&1)"; then
        printf '  ok      %s\n' "${name}"
    else
        printf '  FALHOU  %s\n' "${name}"
        printf '%s\n' "${out}" >&2
        fail=1
    fi
done

echo
echo "=== Compilador (${AIPO_BIN}): execução ==="
for example in "${EXAMPLES}"/*.aipo; do
    name="$(basename "${example}")"
    if grep -q '#!test' "${example}"; then
        subcommand="test"
    else
        subcommand="run"
    fi
    if out="$("${AIPO_BIN}" "${subcommand}" "${example}" 2>&1)"; then
        printf '  ok      %s (%s)\n' "${name}" "${subcommand}"
    else
        printf '  FALHOU  %s (%s)\n' "${name}" "${subcommand}"
        printf '%s\n' "${out}" >&2
        fail=1
    fi
done

echo
if [ "${fail}" -ne 0 ]; then
    echo "Exemplos canônicos NÃO estão em conformidade com Aipo V1." >&2
    exit 1
fi
echo "Todos os exemplos compilam e executam em conformidade com Aipo V1."
