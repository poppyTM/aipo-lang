---
title: "Testes, diagnósticos e conformance"
description: "Referência de uso organizada por tarefas e comportamento do Aipo."
---

# Testes, diagnósticos e conformance

Há duas perguntas diferentes: **meu programa funciona?** e **a linguagem implementa o contrato corretamente?** O Aipo possui ferramental para ambas.

## Teste de programas

O comando `aipo test` descobre testes isolados e permite filtrar pelo nome ou etiqueta. A sintaxe `#!test` marca funções de teste conforme o contrato da linguagem.

```bash
cargo run -q -p aipo-cli -- test
```

## Conferência rápida de um arquivo

```bash
cargo run -q -p aipo-cli -- check examples/06_variables_and_values.aipo
```

O comando de checagem não executa o programa. Um diagnóstico de lexer ou parser explica erro de escrita; um diagnóstico semântico pode indicar nome desconhecido, parâmetro incompatível ou regra de tipo.

## Como a linguagem é verificada

O repositório tem um corpus real em `docs/conformance/` com programas `.aipo`, `.stdout`, diagnósticos `.code` e integração com testes Rust. É **uma dependência ativa de desenvolvimento**, não documentação descartável.

```bash
cargo test --workspace --locked
```

A execução acima é recomendada para contribuidores. **Não afirma que foi executada nesta revisão editorial.** Revisões dos backends demandam também testes diferenciais e negativos.

[Entender erros](/reference/diagnostics) · [Corpus de conformance](https://github.com/poppyTM/aipo-lang/tree/main/docs/conformance).
