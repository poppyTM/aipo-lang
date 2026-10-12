---
title: Workflow de implementação e evidência
---
# Workflow para contribuição orientada por evidências

## Classifique o requisito

| Estado | Significado | Ação |
| --- | --- | --- |
| Compliant | Implementação e testes satisfazem o contrato | Preservar |
| Partial | Núcleo existe mas falta integração | Corrigir delta |
| Experimental | Funciona em subset com limitações | Expor limite |
| Stub | API declarada sem funcionalidade real | Implementar ou retirar |
| Broken | Cenário reproduzível falha | Corrigir a causa |
| Duplicated | Duas fontes competem | Consolidar authority |
| Missing | Não há implementação | Planejar slice |
| Superseded | Contrato substituído | Migrar consumidores |

## Protocolo

1. Reproduza o comportamento e anexe comando/ambiente.
2. Localize a fonte normativa atual e registre conflitos.
3. Planeje o menor slice de mudança sem quebrar APIs não afetadas.
4. Adicione teste positivo, negativo e conformance do recurso.
5. Faça alteração em domínio responsável; não duplicar semântica em backends.
6. Execute gates pertinentes e registre *exatamente* o que correu.
7. Atualize referência, tutorial e status somente após provar suporte.
8. Publique diff, riscos, limitações e próximos passos.

## Evidências

- `SPEC`: seção da decisão vigente.
- `CODE`: símbolo e caminho da implementação.
- `TEST`: comando executado, SHA e resultado.
- `MANUAL`: cenário reproduzível e ambiente.
- `INFERENCE`: hipótese sem prova.

Nenhuma evidência antiga valida automaticamente um HEAD novo.

## Sincronização obrigatória do progresso

Toda implementação ou refatoração que altere funcionalidades, testes ou documentação deve localizar os processos afetados em [Progresso](/progress/) e atualizar `website/public/progress/tasks.json` no **mesmo PR**. Feche checkpoints somente com evidência rastreável, registre `not-run` explicitamente e reabra DONE em regressão. [Protocolo completo](/engineering/progress-protocol).


O gate automático `Aipo progress sync` rejeita PR de implementação que não atualize o JSON. Esse check só verifica a presença da alteração; a revisão humana deve confrontar os IDs, critérios e evidências com o código. [Regras do CI](/engineering/progress-protocol).
