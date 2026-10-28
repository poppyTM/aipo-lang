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

Não use a existência da flag `--engine=reg` como prova de paridade. O emissor gera `AIPO_COMPILE_REG_UNSUPPORTED` quando encontra uma operação não coberta; o plano canônico usa a VM com motivo explícito. Em 2026-10-27, 20/36 programas de conformance rodaram nativamente em Reg e 16/36 usaram fallback, com saída igual à VM nos 36.

## Matriz medida

A [matriz de suporte por backend](https://github.com/poppyTM/aipo-lang/blob/main/docs/architecture/backend-support-matrix.md) registra os números de VM, Reg, JavaScript, Wasm, C ABI, Poppy, eGUI e game host, com comandos de reprodução e limites.

## Matriz mínima para cada recurso

| Dimensão | VM | RegVM | JS | Wasm |
| --- | --- | --- | --- | --- |
| Parse e sema | Compartilhado | Compartilhado | Compartilhado | Compartilhado |
| Execução | Referência | Experimental | Requer corpus | Requer corpus |
| Falhas, contratos e async | Testes centrais | Parcial | A comprovar por caso | A comprovar por caso |
| Capacidades do host | Dependente do host | Dependente do host | Dependente do ambiente | Dependente do ambiente |

A matriz é qualitativa. Não implica “passou nos testes” nesta revisão.

Consulte [Guia de build](/guides/build-targets), [Status](/reference/status) e [corpus](https://github.com/poppy-team/aipo-lang/tree/main/docs/conformance).
