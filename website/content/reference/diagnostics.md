---
title: Diagnósticos e erros
---
# Diagnósticos

Aipo distingue erros de uso da CLI, erros lexicais e sintáticos, problemas de semântica, falhas recuperáveis e faults de execução.

## Etapas

| Origem | O que significa |
| --- | --- |
| Lexer | O texto não forma um token ou literal válido |
| Parser | A sequência de tokens não forma uma construção válida |
| Sema | Contrato, mutação, nome ou importação incompatível |
| Emitter | O backend não consegue representar um recurso |
| Runtime | A execução encontrou falha ou fault |
| CLI | Flag, arquivo, parâmetro ou uso inválido |

## Casos encontrados em auditoria

- `AIPO_SEM_EXPORT_UNKNOWN`: exportação de símbolo não resolvido na situação descrita no journal.
- `AIPO_COMPILE_REG_UNSUPPORTED`: instrução não suportada pelo emissor de registradores.
- `AIPO_RT_FAILURE_UNCAUGHT`: falha recuperável não tratada no topo.
- `AIPO_LEX_INVALID_NUMBER`: literal numérico inválido.

**Fonte de autoridade dos códigos atuais:** [catálogo antigo](https://github.com/poppy-team/aipo-lang/blob/main/docs/diagnostics/catalog.md) e código de `aipo-diagnostics`. Não invente códigos novos com base no prefixo.

Para reproduzir: [Resolver erros](/guides/troubleshooting).
