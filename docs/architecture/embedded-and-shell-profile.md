# Perfis nativo, shell, web e embedded

**Status:** implementação e limites · **Atualização:** P07-G02, 2026-10-09.
**Fontes:** `Cargo.toml`, `crates/aipo-cli/Cargo.toml`, `aipo-bytecode` e `aipo-vm`.

Este documento substitui a especificação anterior que misturava metas de MCU, números de memória/startup não medidos e APIs hipotéticas com o runtime entregue. Consulte o [guia de uso/reimplementação](../development/runtime-and-tooling-guide.md) e a [evidência](../evidence/P07-G02/README.md).

## Perfis de build efetivos

| Seleção em `aipo-cli` | Componentes opcionais | Wasmtime | Biblioteca padrão Rust |
|---|---|---|---|
| Features default | Formatter, JS, emissor Wasm, regex, Unicode extra e runner | Incluído | `std` |
| `--no-default-features --features profile-shell` | Formatter, regex e Unicode extra | Ausente | `std` |
| `--no-default-features --features profile-web` | JS e emissor Wasm | Ausente; adicionar `wasmtime-runner` para runner | `std` |
| `--no-default-features --features profile-embedded` | Nenhum emissor/JIT opcional | Ausente | `std` |
| Mesma seleção embedded com `--profile nano` | LTO, size optimization, strip e panic abort do Cargo | Ausente | `std` |

Aliases não removem defaults sozinhos. Não foram criados crates por perfil. `embedded` significa uma configuração nativa menor, não um porte `no_std`. A CLI mínima ainda registra serviços de SO da stdlib; hosts podem usar as bibliotecas e registrar somente seus próprios serviços. O MSRV é Rust 1.96, alinhado ao grafo Wasmtime/Cranelift do lockfile.

## Um frontend, planos de execução explícitos

A VM de pilha é a referência de semântica: valores, contratos, métodos, captures, scheduler, invariantes e falhas. O RegEmitter compila seu subconjunto estritamente; `ExecutionPlan` seleciona Reg nativo quando todo o IR é representável e verificado, ou bytecode canônico com uma razão explícita.

Isso permite que `aipo run --engine=reg` use a superfície canônica sem fingir que o interpretador Reg possui todas as capacidades. `aipo plan` mostra a decisão sem executar. A API `analyze_to_reg_module` continua estrita. Contextos de host que exigem a VM também selecionam o plano canônico.

JS e Wasm continuam emissores próprios. O Wasm não representa toda a dinâmica de `Value`; `is` com tipo semântico desconhecido é rejeitado com diagnóstico. Não há runtime WAMR/wasmi embutido nesta entrega.

## Representação, memória e registradores

`Value` continua sendo enum Rust com variantes gerenciadas por Rc/RefCell. Seu tamanho depende do alvo; `layout_probe` deve medir o build concreto. Não há promessa atual de 16 bytes, NaN boxing ou RAM mínima fixa.

`List`, `Set` e `Bytes` usam `Collection<T>` com revisão estrutural; Dict usa sua revisão. Guards verificam a revisão independentemente de comprimento. Elementos existentes podem ser substituídos sem alterar a estrutura. Snapshot guest restaura alocações em lugar para preservar aliases.

O RegVM mantém 256 registradores, limite de profundidade e uma arena reservada. Os valores gerenciados atuais continuam em Rc; a arena não é o allocator de todos os valores e não fornece desalocação global O(1). Chamadas compiladas movem/restauram somente a janela viva do caller/callee. Entrada raw conserva todos os registradores do host.

O encoding real é definido em `crates/aipo-bytecode/src/instruction.rs`:

| Campo | Largura | Papel |
|---|---|---|
| Opcode | 7 bits | Até 128 códigos |
| A | 8 bits | Registrador 0–255 |
| B | 9 bits | Operando conforme opcode; registradores continuam limitados a 255 |
| C | 8 bits | Operando conforme opcode |
| Bx | 17 bits | Pool/índice unsigned conforme opcode |
| sBx | 17 bits com bias 65536 | Imediato/salto relativo |

O verifier distingue registrador de índice/imediato. Não é correto interpretar todo B como registrador ou anunciar C com 9 bits.

## Sessão, shell e reload

`aipo-sh` usa Session persistente, entrada multilinha, histórico JSONL e comandos de completion/load/reload/reset. Compila e executa somente statements novos. O perfil Reg interativo usa explicitamente a sessão canônica.

O linker conserva índices publicados e reloca unidades novas. Reload remove declarações do arquivo, inicializa candidata, preserva globals compatíveis, oferece migração e valida layouts retidos. Snapshot restaura definições/heap se a candidata falhar. Histórico e watcher têm limites de tamanho/profundidade.

Rollback não desfaz arquivos, rede, processos ou recursos externos do host. A shell oferece os serviços existentes de `sh`; não implementa controle de jobs POSIX completo, redirecionamento universal de descritores ou pipelines entre processos como consequência de `|>`. O operador continua seguindo o canon.

## Embedding e limites

O C ABI reside em `aipo-c-abi` e expõe os nomes de `include/aipo.h`, não as APIs hipotéticas do documento anterior. Há carga/chamada síncrona, begin/pump/abort cooperativos, handles, capacidades e budgets. Reg raw possui verificação, orçamento e diagnóstico próprio. Strings/Bytes atravessam C como snapshots com release explícito.

Rust usa `Vm`, `RegVm`, `Session` e `ExecutionPlan`. `Value` é `!Send`; cada runtime pertence a uma thread. Reentrada C durante callback é recusada. Pump limita quanta guest, não a duração de callback nativo.

Wasm permite fuel, limite por memória linear e limite de output bufferizado, com engine/cache reutilizáveis e Store separado por execução. Não há limite global de todo o processo JIT ou de todos os allocators Rust.

## Medição e próximos portes

[P07-G02](../evidence/P07-G02/README.md) contém benchmark de chamadas Reg contra a base pinada, dados brutos e reprodução. Não mede cold start, Flash, RSS de todos os perfis ou runtimes externos. Não se deve reaproveitar números de outro layout, versão Lua ou máquina como números atuais da Aipo.

Um porte para MCU exige separar serviços dependentes de SO, projetar allocator/heap limitado, tratar ciclos, definir HAL, validar layout/alinhamento, permitir execução em Flash e medir RAM/Flash/startup em alvo concreto. `nano` com `panic=abort` não fornece recuperação de panic por unwind.

A matriz de compilação multiplataforma e o workflow de artifacts estão implementados; seu sucesso deve ser observado antes de certificar plataformas. Os testes desta rodada foram explicitamente dispensados pelo usuário. A implementação não declara os gates Prumo concluídos.
