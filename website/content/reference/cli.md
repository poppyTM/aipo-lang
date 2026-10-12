---
title: Referência da CLI
description: Comandos e opções do Aipo com exemplos reproduzíveis.
---

# CLI do Aipo

A CLI executa, analisa, formata, testa e gera artefatos. Os comandos abaixo são documentados em [`docs/reference/cli.md`](https://github.com/poppyTM/aipo-lang/blob/main/docs/reference/cli.md) e mantidos em `crates/aipo-cli`.

## Primeiro uso

```bash
cargo build -p aipo-cli
cargo run -q -p aipo-cli -- run examples/06_variables_and_values.aipo
```

## Comandos comuns

| Comando | Resultado |
| --- | --- |
| `new demo` | Cria um pacote local |
| `run file.aipo` | Compila e executa |
| `plan file.aipo` | Mostra o plano sem executar |
| `profile file.aipo --budget 100000 --json` | Mede execução e consumo |
| `debug file.aipo` | Depura por instruções |
| `watch file.aipo` | Recarrega preservando a última geração válida |
| `lsp` | Inicia protocolo de editor via stdio |
| `check file.aipo` | Analisa sem executar |
| `test [path] --filter pattern` | Descobre e executa testes selecionados |
| `fmt file.aipo [--check]` | Formata / verifica formatação |
| `build file.aipo --target js --out dist/` | Gera bundle ESM |
| `build file.aipo --target wasm --out dist/` | Gera binário WebAssembly |
| `disasm file.aipo` | Mostra o bytecode desassemblado |
| `disasm file.aibc` | Inspeciona bytecode compilado |
| `package lock DIR` | Resolve e escreve lockfile |
| `package audit DIR` | Audita o lockfile sem regravar |
| `package vendor DIR --out DEST` | Exporta pacote e dependências locais offline |
| `package cache verify DIR` | Verifica entradas de cache |
| `package cache prune DIR --lock LOCK [--apply]` | Limpeza; simulação por padrão |

## Executar destinos diferentes

- `--engine=vm`: VM principal, referência para comparar resultados.
- `--engine=reg`: plano de execução que escolhe Reg nativo verificado ou VM canônica, com motivo explícito. Consulte `plan` antes de medir.
- `--wasm`: seleciona execução Wasm quando disponível.
- `--package-cache DIR`: fornece cache de pacotes sem presumir busca na rede.
- `--ahs FILE`: fornece schema de API de host para análise; **não concede capabilities sozinho**.
- `--host=headless-test`: instala perfil de host de conformance explicitamente.

## Dependências GitHub

O sistema aceita dependências pinadas em commit e fetch explícito, sujeito à feature Rust `github-http`. A execução offline é o padrão para leitura de lock/cache. Confira as opções completas no [CLI Reference original](https://github.com/poppyTM/aipo-lang/blob/main/docs/reference/cli.md), pois há argumentos de cache, paths e credenciais que não cabem em uma linha de consulta.

## Códigos de saída

`0` normalmente indica sucesso; `1` representa erro de linguagem/testes; `2` indica uso inválido da CLI. Para capturar a causa, preserve também a mensagem de diagnóstico emitida.

[Guia mais simples](/manual/cli) · [Como resolver erros](/guides/troubleshooting).
