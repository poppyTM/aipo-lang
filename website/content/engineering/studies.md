---
title: Estudos e referências de runtimes
---
# Estudos de implementação

O Aipo possui estudos de [VMs](https://github.com/poppy-team/aipo-lang/blob/main/docs/studies/reference-vms.md), [runtimes](https://github.com/poppy-team/aipo-lang/blob/main/docs/studies/reference-runtimes.md) e [lições aplicadas](https://github.com/poppy-team/aipo-lang/blob/main/docs/studies/lessons-for-aipo.md). A revisão de outubro também incluiu [guia para LLMs](https://github.com/poppy-team/aipo-lang/blob/main/docs/studies/llm-study-guide.md).

## Como tratar referências externas

Um design bem-sucedido em Wren, QuickJS, Lua, LuaJIT, Luau, Janet ou motores Wasm **não pode ser importado automaticamente** para Aipo. É necessário identificar a propriedade que se deseja preservar, os custos de integração e o contrato testável.

## Fluxo

1. Pergunta técnica concreta: qual problema medido precisa de solução?
2. Fonte upstream pinada por commit, arquivo e símbolo.
3. Comparação com a implementação atual, com fatos separados de hipóteses.
4. Proposta de modificação limitada a um domínio.
5. Benchmark e conformance antes/depois.
6. Registro de resultado, rejeição ou pendência.

## Tópicos prioritários

- Lifetime de closures, frames e upvalues.
- Limites de memória, budgets, verificação de bytecode hostil.
- Valores compactos e custos de alocação.
- Startup e embedding do perfil mínimo.
- Semântica compartilhada entre VM, JS e Wasm.

**Nota:** a leitura dos upstreams na reconstrução de outubro não incluiu compilações nem benchmarks desses runtimes. Não use os documentos como resultados de medição de velocidade.
