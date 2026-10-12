---
title: Progresso de implementação
description: Estado auditável por processo, critérios de aceite, evidência e validação da linguagem Aipo.
---

# Progresso da implementação

Acompanhe o que existe, o que ainda precisa ser demonstrado e o que falta implementar ou certificar. Este painel segue o **modelo de acompanhamento do Petunia3D**, adaptado ao repositório Rust e à documentação VitePress do Aipo.

**O painel não é uma estimativa de quanto da linguagem está pronta.** O percentual de cada processo mede apenas critérios de aceite comprovados — não tempo restante, linhas implementadas ou maturidade global do produto.

[Regras obrigatórias para manter o progresso](/engineering/progress-protocol) · [Auditoria da documentação antiga](/engineering/legacy-audit) · [Matriz de suporte](/reference/status)

<ProgressBoard />

## Interpretação dos dados

- **TODO:** nenhum checkpoint daquele processo foi concluído.
- **IN PROGRESS (025%):** um em quatro checkpoints tem evidência reconhecida; gates restantes continuam abertos.
- **DONE:** todos os checkpoints concluídos, as evidências vinculadas e todos os gates aplicáveis em estado `pass`.

O status é explícito no JSON versionado. O site calcula o percentual, agrupa por etapa e filtra os processos; **não lê a implementação em tempo real**. A base desta primeira auditoria é o commit `7ab1538d1c1f2dde67e01527649c9a7eebd2ed4c`. A revisão P07-G01 registra testes não executados; por isso a inspeção da existência de código não fecha automaticamente os processos de runtime.

## Como sugerir correções

Localize o ID do processo, confira as evidências e indique exatamente qual critério precisa mudar. Mudanças de código e mudanças de progresso pertencem ao **mesmo PR**. Um novo resultado de testes só é válido quando registra comando, ambiente, SHA e resultado verificável.

[Consultar o protocolo completo](/engineering/progress-protocol).
