---
title: Backends JavaScript e Wasm
---
# Emissão e portabilidade

Aipo possui múltiplos caminhos de compilação e execução. Cada backend representa um contrato de compatibilidade que deve ser medido com o corpus.

## JavaScript (`aipo-js`)

O backend gera artefatos ESM e um runtime shim com semântica compatível onde demonstrada. Testes diferenciais precisam comparar valores, erros, ordem observável e limites relevantes. O código gerado deve conservar mapeamento para fonte quando disponível.

## WebAssembly (`aipo-wasm`)

O compilador Wasm atual recebe HIR diretamente segundo a documentação de hardening, enquanto JavaScript parte de Core IR. Essa diferença arquitetural importa na manutenção de passes e otimizações. O runner com Wasmtime é dependente de features.

## Register VM

Não use a existência da flag `--engine=reg` como prova de paridade. O emissor precisa gerar erro `AIPO_COMPILE_REG_UNSUPPORTED` com orientação útil quando encontrar uma operação não coberta.

## Matriz mínima para cada recurso

| Dimensão | VM | RegVM | JS | Wasm |
| --- | --- | --- | --- | --- |
| Parse e sema | Compartilhado | Compartilhado | Compartilhado | Compartilhado |
| Execução | Referência | Experimental | Requer corpus | Requer corpus |
| Falhas, contratos e async | Testes centrais | Parcial | A comprovar por caso | A comprovar por caso |
| Capacidades do host | Dependente do host | Dependente do host | Dependente do ambiente | Dependente do ambiente |

A matriz é qualitativa. Não implica “passou nos testes” nesta revisão.

Consulte [Guia de build](/guides/build-targets), [Status](/reference/status) e [corpus](https://github.com/poppy-team/aipo-lang/tree/main/docs/conformance).
