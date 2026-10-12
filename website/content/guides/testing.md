---
title: Testar programas Aipo
---
# Testar programas Aipo

## Fluxo recomendado

```bash
cargo run -q -p aipo-cli -- check examples/06_variables_and_values.aipo
cargo run -q -p aipo-cli -- test
cargo test --workspace
```

`check` analisa um programa sem executá-lo; `test` executa a suíte de arquivos Aipo identificados; `cargo test` valida a implementação Rust. São níveis diferentes e não se substituem.

## Fixtures de conformidade

O [corpus existente](https://github.com/poppy-team/aipo-lang/tree/main/docs/conformance) guarda entradas `.aipo`, saídas `.stdout` e códigos de diagnóstico `.code`. Para corrigir uma semântica:

1. Reproduza com a menor entrada possível.
2. Defina a saída esperada ou o código de erro.
3. Acrescente uma fixture que falhe antes da correção.
4. Faça a alteração no domínio responsável.
5. Reexecute o conjunto afetado e somente então atualize o registro de evidências.

Não marque um teste como executado porque ele foi escrito. O merge de runtime de 9 de outubro de 2026 registrou expressamente ausência de execução de gates na reconstrução correspondente.
