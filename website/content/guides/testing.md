---
title: Testar programas Aipo
---
# Testar programas Aipo

## Fluxo recomendado

```bash
cargo run -q -p aipo-cli -- check examples/06_variables_and_values.aipo
cargo run -q -p aipo-cli -- test packages/aipo-ui
cargo test --workspace
```

`check` analisa um programa sem executá-lo; `test <dir>` executa a suíte Aipo daquele diretório; `cargo test` valida a implementação Rust. São níveis diferentes e não se substituem.

## Fixtures de conformidade

O [corpus existente](https://github.com/poppyTM/aipo-lang/tree/main/docs/conformance) guarda entradas `.aipo`, saídas `.stdout` e códigos de diagnóstico `.code`. Para corrigir uma semântica:

1. Reproduza com a menor entrada possível.
2. Defina a saída esperada ou o código de erro.
3. Acrescente uma fixture que falhe antes da correção.
4. Faça a alteração no domínio responsável.
5. Reexecute o conjunto afetado e somente então atualize o registro de evidências.

Não marque um teste como executado porque ele foi escrito. A reconciliação de 2026-10-27 executou o corpus VM/Reg, o workspace completo (1.130 testes), Poppy, eGUI, game host e build VitePress.
