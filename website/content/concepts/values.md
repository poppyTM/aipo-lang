---
title: Valores e mutabilidade
---
# Valores, vínculos e mutação

Uma variável é o nome pelo qual um programa acessa um valor. Em Aipo, `let` comunica que o vínculo não será reatribuído; `var` comunica intenção de atualização.

## Por que imutabilidade por padrão?

Em funções maiores, mutabilidade irrestrita aumenta a quantidade de estados possíveis. Quando dados não são reatribuídos, o leitor precisa considerar menos mudanças possíveis.

**Imutabilidade do vínculo não significa automaticamente imutabilidade profunda de todos os dados referenciados.** Coleções, estruturas e handles de host têm regras próprias: consulte a semântica de cada tipo.

## Mutabilidade de estruturas

Campos de `struct` e receptores de método seguem contratos específicos. O `var self` explicita uma operação que altera o receptor na sintaxe-alvo. Para cada mudança de semântica, a implementação deve possuir testes que cubram escopo, aliasing e rollback.

Exercício de leitura: [Valores e variáveis](/learn/02-variaveis) e [Métodos](/learn/14-metodos).
