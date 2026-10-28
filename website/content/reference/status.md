---
title: Estado dos backends
description: Como entender cobertura e maturidade sem misturar código existente com teste aprovado.
---

# Compatibilidade e limites

O Aipo possui implementações reais para diferentes destinos. **Não existe uma certificação universal de paridade** apenas porque os crates estão presentes.

## Entenda a diferença

| Motor | Implementação identificada | Medição de 2026-10-27 |
| --- | --- | --- |
| **VM de pilha** | `aipo-vm`: valor, chamadas e execução | 36/36 programas de conformance e 29/29 diagnósticos |
| **Perfil Reg** | Verifier e escolha explícita entre Reg nativo e VM canônica | 20/36 nativos, 16/36 com fallback explícito; saída igual em 36/36 |
| **JavaScript** | `aipo-js`: emissão ESM e runtime shim | 36/36 programas e 23 exemplos com saída igual à VM |
| **WebAssembly** | `aipo-wasm`: HIR, emissor e runner | 16/36 programas de conformance e 5/26 exemplos compilam no subset atual; predicados, acesso e mutações de List já são suportados; falhas emitem `AIPO_COMPILE_WASM_UNSUPPORTED` |
| **C ABI** | `aipo-c-abi`, begin/pump/abort, linker e harness Rust/C | Suíte Rust, harness C nativo e sincronização header↔exports verdes; consumidor externo por plataforma continua pendente |

A [matriz de suporte por backend](https://github.com/poppyTM/aipo-lang/blob/main/docs/architecture/backend-support-matrix.md) registra os comandos e os limites. Números medidos não são certificação universal.

## Limitações registradas

A auditoria [P07-G01](https://github.com/poppyTM/aipo-lang/blob/main/docs/evidence/P07-G01-runtime-hardening.md) documenta partes da RegVM ainda sem paridade completa: métodos, closures/upvalues, async, hooks, invariantes, journal e host. Ela também registra verificações de compilação e testes **não executados na rodada descrita**.

A presença de uma suite de testes não é prova de que ela passou na última revisão. A reconciliação de 2026-10-27 executou o workspace completo (1.130 testes), conformance VM/Reg, Poppy, eGUI, game host, validadores do portal e build VitePress. Consulte o [painel de progresso](/progress/) para ver critérios e evidências, sem interpretar seus percentuais como a porcentagem de linguagem pronta.

## Rodada P07-G02

O [guia de implementação](/engineering/runtime-and-tooling) distingue verifier, sessões/tasks persistentes, rollback guest, C cooperativo, provas/limites Wasm, ferramentas e distribuição. A reconciliação de 2026-10-27 executou conformance e a suíte do workspace; a matriz de MSRV 1.96 em runner dedicado continua pendente. Reg nativo, LSP básico e perfil embedded com `std` têm limites explícitos.

## Como testar sua necessidade

1. Escolha um [programa completo](/examples/) que use o recurso.
2. Execute primeiro na VM de referência.
3. Compile ou rode no segundo destino, usando as flags adequadas.
4. Compare resultados e erros; anote o SHA, o ambiente e os comandos.
5. Se houver divergência, abra um bug e não anuncie paridade geral.

## Código e testes que ajudam

- [Conformance de programas](https://github.com/poppyTM/aipo-lang/tree/main/docs/conformance/programs).
- [Testes diferenciais da CLI](https://github.com/poppyTM/aipo-lang/tree/main/crates/aipo-cli/tests).
- [RegVM](https://github.com/poppyTM/aipo-lang/blob/main/crates/aipo-vm/src/reg_vm.rs).
- [Wasm](https://github.com/poppyTM/aipo-lang/tree/main/crates/aipo-wasm).

[Escolher backend](/manual/backends) · [Comandos de build](/reference/cli).
