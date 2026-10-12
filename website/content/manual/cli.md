---
title: "CLI: executar, conferir, testar e compilar"
description: "Referência de uso organizada por tarefas e comportamento do Aipo."
---

# CLI: executar, conferir, testar e compilar

Você pode utilizar o Aipo diretamente pela CLI gerada com Rust. O comando mais simples durante o desenvolvimento é `cargo run -q -p aipo-cli -- …` na raiz do repositório.

## Comandos do cotidiano

| O que quero fazer | Comando |
| --- | --- |
| Criar projeto | `aipo new demo` |
| Executar | `aipo run main.aipo` |
| Ver o plano sem executar | `aipo plan main.aipo` |
| Medir com limite | `aipo profile main.aipo --budget 100000 --json` |
| Recarregar ao editar | `aipo watch main.aipo` |
| Depurar | `aipo debug main.aipo` |
| Integrar editor | `aipo lsp` |
| Analisar sem executar | `aipo check main.aipo` |
| Formatar | `aipo fmt main.aipo` |
| Conferir formatação | `aipo fmt main.aipo --check` |
| Descobrir e rodar testes | `aipo test` |
| Compilar JavaScript | `aipo build main.aipo --target js --out dist/` |
| Compilar Wasm | `aipo build main.aipo --target wasm --out dist/` |
| Desassemblar | `aipo disasm main.aipo` |
| Auditar pacote | `aipo package audit ./my-package` |

A tabela apresenta a interface de comando documentada no [CLI Reference do repositório](https://github.com/poppyTM/aipo-lang/blob/main/docs/reference/cli.md).

## Exemplos de uso sem instalar globalmente

```bash
cargo build -p aipo-cli
cargo run -q -p aipo-cli -- check examples/06_variables_and_values.aipo
cargo run -q -p aipo-cli -- run examples/06_variables_and_values.aipo
cargo run -q -p aipo-cli -- fmt examples/06_variables_and_values.aipo --check
```

## Backends alternativos

`--engine=vm` é a VM de referência. `--engine=reg` escolhe Reg nativo quando todo o programa é suportado e verificado; os demais casos usam serviços canônicos com razão explícita. Use `aipo plan main.aipo` para consultar a escolha. O interpretador Reg nativo continua um subconjunto. `--wasm` e `--target wasm` envolvem o destino WebAssembly e suas restrições.

**Se aparecer um erro:** rode `check` primeiro. Um erro de parser, um contrato estático inválido e uma falha em tempo de execução pedem correções diferentes.

[Primeiros passos](/start/) · [Diagnósticos](/reference/diagnostics) · [Targets](/manual/backends).
