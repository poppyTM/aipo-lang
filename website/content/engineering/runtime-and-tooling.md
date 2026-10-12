---
title: Runtime e ferramentas — P07-G02
description: Guia de uso, motivos das mudanças, reimplementação, evidências e limites.
---

# Runtime e ferramentas: uso e reimplementação

A rodada P07-G02 implementa as cinco frentes de runtime, embedding, shell/perfis, performance e ferramentas. O código está em revisão; testes não foram executados por instrução explícita do usuário. O objetivo permanece DRAFT, sem merge ou release pública.

O [guia completo em português](https://github.com/poppyTM/aipo-lang/blob/feat/runtime-and-tooling-completion/docs/development/runtime-and-tooling-guide.md) contém os algoritmos, comandos, APIs Rust/C, migração de clientes, motivos e roteiro de reimplementação. A [evidência](https://github.com/poppyTM/aipo-lang/blob/feat/runtime-and-tooling-completion/docs/evidence/P07-G02/README.md) separa compilação, medições e limites. A [continuidade dos estudos](https://github.com/poppyTM/aipo-lang/blob/feat/runtime-and-tooling-completion/docs/studies/runtime-and-tooling-follow-up.md) relaciona as decisões a nove fontes pinadas.

## Mudanças e critérios abertos

| Frente | Mudança | Limite |
| --- | --- | --- |
| Correção | Verifier Reg, fatos de inicialização/escopos, revisão estrutural, `is` Wasm conservador | Conformance/fuzz/Miri/ASan não executados |
| Cobertura e embedding | Plano explícito, linker, budgets, C begin/pump/abort, limites/cache Wasm | Reg nativo continua subconjunto; nativos podem bloquear |
| Sessão e perfis | REPL sem replay, tasks persistentes, reload com migração/rollback, shell/web/embedded | Efeitos externos não são revertidos; embedded ainda usa `std` |
| Performance | Janelas vivas, corpos Rc, constantes/provas por módulo, benchmark pareado | Microbenchmark não mede toda a linguagem, cold start ou RAM |
| Ferramentas e distribuição | new/watch/profile/debug/plan/LSP, vendor offline, empacotamento/instalação | LSP básico; outras plataformas e releases não certificadas |

## Por que as mudanças foram necessárias

O verifier evita leitura de registradores sem inicialização e referências inválidas antes de efeitos guest. Revisões estruturais detectam remove+insert mesmo com comprimento final igual. Snapshot em lugar restaura aliases e células de tasks, enquanto o linker preserva offsets de código publicado.

No Wasm, um ramo antecipado antigo aceitava tipos físicos e impedia a checagem semântica posterior. O ramo foi removido. Aliases e nomes de tipo sombreados agora produzem diagnóstico de capacidade; não viram `true` ou `false` por coincidência de representação.

O plano canônico permite compartilhar serviços da linguagem. Não cria uma segunda semântica de closures/async no Reg. Perfis são escolhas de features; não linguagens diferentes nem portabilidade MCU certificada.

## Progresso e reimplementação

IDs afetados: A03, R02–R06, B02–B03, I01/I03/I05/I07 e Q01–Q04. Os checkpoints de inventário e gaps reconhecidos têm evidência de fonte; gates de testes continuam `not-run`. Consulte o [painel](/progress/) e o [protocolo](/engineering/progress-protocol).

Reimplemente na ordem: verifier/coleções, linker/snapshot/scheduler, Session/reload, embedding, ferramentas e distribuição. Preserve identidade, resultados/diagnósticos, ordem de efeitos, budgets e scopes de unwind. Use os casos de regressão existentes antes de ampliar suporte.

[Estado dos backends](/reference/status) · [CLI](/reference/cli) · [Embedding](/engineering/embedding).
