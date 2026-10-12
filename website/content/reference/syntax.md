---
title: Sintaxe da implementação
description: Formas praticáveis, exemplos reais e diferenças com propostas normativas.
---

# Sintaxe do Aipo

Esta referência distingue **o código mantido nos exemplos do repositório** das propostas normativas para versões futuras. Isso evita aprender uma forma que seu compilador ainda não aceita.

## Elementos básicos

| Para fazer… | Escreva |
| --- | --- |
| Valor fixo | `let name = "ana"` |
| Estado mutável | `var count = 0` |
| Atualizar estado | `count = count + 1` |
| Função | `fn f(x) { return x }` |
| Função com default | `fn greet(name, prefix = "olá") { ... }` |
| Condição | `if ready { ... } else { ... }` |
| Condição que retorna valor | `if ready then a else b` |
| Repetição contada | `repeat 4 as i { ... }` |
| Percorrer lista | `each item in items { ... }` |
| Lista | `[1, 2, 3]` |
| Dicionário | `{ "name": "ana" }` |
| Instância de struct | `Task{ title: "parser" }` |
| Método de tipo | `Task:finish(var self) { ... }` |
| Fall back de falha | `parse(text) or_else fallback` |
| Propagar falha | `parse(text)?` |
| Interpolação | `f"Olá {name}"` |
| Diretiva | `#!test`, `#!satisfies Greeter` |

Os snippets nesta tabela são **formas de referência**, não programas completos independentes. Veja os [exemplos reais](/examples/) para executar cada caso.

## Operadores e expressões

- Aritmética: `+`, `-`, `*`, `/`, `//` (divisão inteira).
- Comparação: `==`, `!=`, `<`, `<=`, `>`, `>=`.
- Lógica: `and`, `or`, `not`.
- Range: `0..10`; slices: `xs[0..3]`.
- Pipeline: `value |> func`.
- Cópia funcional: `instance with { field: value }`.
- Padrões: `enum`, `match`, `when` e guardas.

[Manual de operadores](/manual/operators) · [Enums e padrões](/manual/enums-patterns).

## Sintaxe alvo e pontos de atenção

O arquivo [`SYNTAX.md`](https://github.com/poppyTM/aipo-lang/blob/main/SYNTAX.md) define uma **proposta normativa de V1** e declara explicitamente que ela **não foi integralmente implementada**. Ele descreve mudanças em relação a formas históricas como `end`, `div`, `impl`, `satisfy` e `self!`.

Os exemplos mantidos no repositório contêm as formas atualmente usadas, incluindo blocos `{ }`, `Tipo:metodo`, `var self` e diretivas `#!`. Não tente substituir automaticamente todos os arquivos antigos sem conferir os testes do parser.

**Fonte para verificar:** [fixtures de conformance](https://github.com/poppyTM/aipo-lang/tree/main/docs/conformance/programs), [exemplos reais](/examples/) e [parser Rust](https://github.com/poppyTM/aipo-lang/tree/main/crates/aipo-syntax).

[Manual](/manual/) · [Compatibilidade](/reference/status).
