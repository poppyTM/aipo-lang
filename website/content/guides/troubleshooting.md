---
title: Resolver erros
---
# Resolver erros sem adivinhar

Quando um programa não funciona, comece por identificar em qual fase falhou.

| Sinal | Onde investigar |
| --- | --- |
| Token, número ou string inesperados | Lexer e normalização de fonte |
| Bloco, declaração ou expressão inválida | Parser / sintaxe implementada |
| Nome desconhecido ou contrato incompatível | Análise semântica e módulos |
| Instrução de backend não suportada | Emissor de destino e matriz de suporte |
| Falha de execução | Runtime, handles, capability, dados e orçamento |

## Procedimento

1. Guarde o arquivo e o comando exatos.
2. Rode `aipo check arquivo.aipo`.
3. Rode na Stack VM de referência antes de testar outros destinos.
4. Reduza a entrada ao menor caso que reproduz.
5. Compare o comportamento com o [corpus de conformidade](https://github.com/poppy-team/aipo-lang/tree/main/docs/conformance).
6. Se uma forma estiver no [ADR-001](https://github.com/poppy-team/aipo-lang/blob/main/docs/decisions/adr-001-canonical-syntax.md) mas não no parser, registre divergência; não disfarce alterando o exemplo.

Consulte o [catálogo de diagnósticos](/reference/diagnostics) e o [registro de conflitos](/engineering/decisions/conflicts).
