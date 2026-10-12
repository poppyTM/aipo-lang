---
title: Prompts de implementação e revisão
---
# Prompts por tarefa

Estes modelos são mensagens de orientação para humanos e agentes. Adapte o domínio e o contrato à tarefa concreta. **Nunca use um prompt como substituto de decisão, código ou teste.**

## Implementação de uma feature

> Leia o contrato canônico de [DOMÍNIO] e o código do branch atual. Produza uma matriz do que já existe, o que diverge, o que está ausente e o que é experimental. Implemente o menor delta coerente, preserve boundaries, adicione fixtures e execute gates pertinentes. Registre comandos de teste e status real; não promova uma proposta a recurso verificado.

## Auditoria de drift de sintaxe

> Compare ADR-001, SYNTAX.md, lexer/parser, sema, CLI e os exemplos publicados. Separe sintaxe-alvo de sintaxe implementada. Reproduza cada divergência em fixtures, não infira funcionalidade pela documentação. Relate impacto nas trilhas de aprendizado e na referência.

## Otimização do runtime

> Identifique baseline, alocações e custo de despacho no código atual. Compare pelo menos uma alternativa com invariantes de bytecode, erros e capacidades. Implemente mudança isolada, execute conformance, teste inputs hostis, registre mediana/variância. Se benchmark não foi executado, declare o limite.

## Auditoria documental

> Identifique quem usa a página, de quem é o contrato, se há conteúdo duplicado ou histórico e qual página deve ser autoridade. Atualize a fonte, preserve links externos e traduções, execute verificador de rotas. Não apague fixtures/evidências consumidas por outros sistemas.
