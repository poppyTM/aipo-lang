---
title: Decisões arquiteturais
---
# Registro de decisões

Aipo usa ADPs e ADRs para registrar o *porquê* de contratos e mudanças. O registro não deve competir com a especificação nem com o estado do compilador.

## Fontes

- [ADPs originais](https://github.com/poppy-team/aipo-lang/tree/main/docs/adp) — conteúdo detalhado, propostas e questões em aberto.
- [Resumos de decisões](https://github.com/poppy-team/aipo-lang/tree/main/docs/decisions) — índice público que pode divergir do documento completo.
- [ADR-001: sintaxe-alvo](https://github.com/poppy-team/aipo-lang/blob/main/docs/decisions/adr-001-canonical-syntax.md).
- [SYNTAX.md](https://github.com/poppy-team/aipo-lang/blob/main/SYNTAX.md) — documentação da sintaxe-alvo e diferenças.
- [Mapa de autoridade legado](https://github.com/poppy-team/aipo-lang/blob/main/docs/language/authority-map.md).

## Estado de uma decisão

- **Proposed:** problema identificado; alternativas em discussão.
- **Accepted:** escolha normativa aprovada; pode não estar implementada.
- **In migration:** há diferença entre contrato e código.
- **Implemented, unverified:** alteração existe; gates relevantes não foram executados ou documentados.
- **Verified:** evidências no commit correto sustentam o comportamento.
- **Superseded:** substituída por decisão explícita, mantida para rastreabilidade.

Um documento não pode ser declarado simultaneamente `Accepted` e `Verified` apenas porque existe um teste.

## Conflitos

Veja o [registro de conflitos](/engineering/decisions/conflicts) antes de promover o conteúdo de `docs/canon` a manual de uso. A normalização definitiva dos ADPs originais precisa reconciliar seus escopos sem apagar histórico.
