---
title: Falhas recuperáveis e contratos
---
# Falhas, faults e integridade

Uma falha recuperável representa um evento que o programa pode administrar, como não encontrar dados esperados. Um fault representa uma violação que pode exigir interromper a execução, como problemas de runtime ou de contrato.

## Por que distinguir?

Quando todos os erros são tratados como exceções indiferenciadas, é fácil esconder bugs e corrupção de estado. Aipo oferece `attempt ... failed` para tratar falhas suportadas, `or_else` para fallback e contratos para verificar expectativas.

## Rollback não significa transação universal

O journal de runtime executa restaurações conforme limites e fronteiras que precisam ser testados. Não deduza transações ACID de banco de dados, nem segurança multi-thread, de uma palavra como “atômico”.

A implementação deve explicitar quais objetos, alterações e backends são cobertos. Use [Contratos](/learn/20-contratos), [Invariantes](/learn/22-invariantes) e [Runtime](/engineering/runtime).
