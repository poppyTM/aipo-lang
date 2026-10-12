---
title: "Diretivas, testes e expressões modernas"
description: Manual atualizado com fontes da linguagem Aipo.
---

# Diretivas, testes e expressões modernas

O Aipo aceita diretivas com prefixo `#!`. Elas não são comentários: orientam o compilador ou os mecanismos de teste.

## Diretivas mais úteis

| Diretiva | Para quê |
| --- | --- |
| `#!test` | Identificar uma função de teste |
| `#!test[tag]` | Atribuir uma etiqueta ao teste |
| `#!satisfies Interface` | Declarar a intenção de conformidade estrutural |
| `#!deprecated` | Marcar um elemento obsoleto |
| `#!todo` | Registrar ação pendente no próprio código |

O grau de suporte a cada forma de parâmetro da diretiva depende da implementação e da versão do frontend. Antes de produzir uma biblioteca pública, consulte [testes reais de diretivas](https://github.com/poppyTM/aipo-lang/blob/main/crates/aipo-cli/tests/directive_tests.rs).

## Funções e padrões modernos

Além de `fn`, a implementação contém testes para lambdas `=>`, funções locais recursivas, combinações de `with`, expressões condicionais `if … then … else …` e novos padrões de `match`. Consulte os exemplos e fixtures deste manual, em vez de tentar combinar todas essas formas de uma vez.

## Para manter a leitura clara

Uma feature nova não deve obrigar você a aprender dez recursos ao mesmo tempo. Comece por um exemplo pequeno, execute a VM de referência e aumente a complexidade apenas quando seu programa exigir.

[Usar testes](/manual/testing) · [Enums e padrões](/manual/enums-patterns) · [Operadores](/manual/operators).
