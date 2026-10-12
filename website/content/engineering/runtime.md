---
title: Runtime e máquinas virtuais
---
# Runtime: valores, frames e execução

O runtime materializa os contratos do Aipo: valores, chamadas, closures, estado mutável, tratamento de falhas, módulos, orçamentos e fronteiras com o host.

## Stack VM

A Stack VM é o caminho de execução de referência do projeto. Estado de pilha, frames, closures e upvalues precisam ter lifetimes claros. Alterações de semântica de valor não devem ser duplicadas por emissor.

## Register VM experimental

A RegVM busca um modelo de execução diferente, mas não tem paridade integral. A revisão de 9 de outubro abordou:
- retorno e restauração de caller e handlers;
- operandos de bytecode e jumps;
- delegação de aritmética e igualdade às regras compartilhadas de `Value`;
- budgets de instruções;
- iteração de dicionários e guardas;
- rejeição explícita de operações não suportadas.

O [guia de reimplementação](https://github.com/poppy-team/aipo-lang/blob/main/docs/development/runtime-hardening-guide.md) contém detalhes de código e limites, sem substituir os contratos de especificação.

## Falhas e rollback

Distinga `fail` recuperável, falha não tratada, fault e orçamento excedido. O journal de rollback não equivale a uma transação universal de sistema externo. Teste as fronteiras de mutação realmente protegidas.

## Segurança do host

Handles geracionais, verificação de escopo, capacidades, orçamentos de instrução, limites de alocação e cancelamento devem ser tratados como limites concretos de execução. A ausência de threads na VM não prova que todo objeto externamente acessível seja seguro.

## Critérios de aceitação

Uma alteração de runtime exige fixture no corpus, execução nas variantes afetadas, teste de entradas hostis quando aplicável e benchmark antes/depois quando a motivação for performance. **Os gates da reconstrução P07-G01 não foram executados**, segundo seu registro de evidência. Veja [Qualidade](/engineering/quality).
