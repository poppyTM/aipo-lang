---
title: Auditoria e migração do legado
description: Inventário de documentos antigos e critérios de remoção segura.
---

# Auditoria da documentação legada

**Decisão de 2026-10-09:** manter `docs/` nesta rodada. O novo site é a entrada editorial principal, mas a migração **não possui paridade semântica total**. Excluir os arquivos agora eliminaria testes, decisões, provas e traduções ainda sem substituição.

[Protocolo de progresso](/engineering/progress-protocol) · [Histórico](/archive/) · [Decisões conflitantes](/engineering/decisions/conflicts) · [Progresso D02](/progress/#D02)

## Inventário de referência

Inventário Git da `main`, baseline `7ab1538d1c1f2dde67e01527649c9a7eebd2ed4c`, em 2026-10-09. Não é estimativa baseada no número de arquivos visíveis no site.

| Diretório/coleção legada | Arquivos observados | Resultado da auditoria | Ação |
| --- | ---: | --- | --- |
| `docs/conformance/` | 175 | Corpus executável, diagnósticos, snapshots, fixtures de ABI | **Preservar**, mover só com mudança explícita do runner |
| `docs/en/` | 105 | Muitas páginas não migraram para o novo website, que tem EN parcial | **Preservar** |
| `docs/canon/` | 35 | Mistura de normas da linguagem e decisões históricas, incluindo Odin | Preservar até adjudicar validade/supersessão |
| `docs/evidence/` | 24 | Evidências de metas, P07 e resultados históricos | Preservar rastreabilidade imutável |
| `docs/decisions/` e `docs/adp/` | 28 (17 + 11) | Contratos normativos e conflitos de estado | Preservar até mapa de autoridade por assunto |
| `docs/architecture/` | 12 | Descrição detalhada da arquitetura e critérios que o portal resume | Migrar por domínio com revisão humana |
| `docs/studies/` | 7 | Pesquisa com referências e propostas que não são implementação | Preservar citações e histórico |
| Demais arquivos em `docs/` | 99 | Manuais, modelos de produto, segurança, operações, pacotes, governança, CI e outras seções | Avaliar individualmente |
| **Total** | **485** | 485 arquivos de conteúdo e suporte, não 485 páginas independentes do portal | **Sem exclusão em lote** |

O novo `website/` possui 70 arquivos no diretório `content/` no snapshot inspecionado, com **26 capítulos de aprendizagem**. Uma página resumida não substitui automaticamente vários contratos profundos, nem um tutorial substitui fixture testável.

## Matriz de decisão para cada arquivo antigo

Antes de remover qualquer caminho, exigir os sete registros:

1. **Consumidor:** localizar referências em crates Rust, GitHub Actions, scripts, Prumo, PRs, README, links públicos e exemplos.
2. **Autoridade:** classificar `canonical`, `historical`, `superseded`, `test-fixture` ou `research`; resolver conflitos ADP/ADR sem assumir vitória do mais novo.
3. **Destino equivalente:** página `website/content/` com todas as definições, assinaturas, exemplos, limites e decisões vigentes.
4. **Prova:** snippets validados por backend e testes de consumidor reexecutados quando aplicável.
5. **Idiomas:** tradução preservada, migrada ou formalmente declarada parcial; não prometer paridade internacional inexistente.
6. **Compatibilidade:** redirecionamento ou mapa explícito dos links antigos, inclusive anchors publicados no GitHub.
7. **CI e revisão:** alteração atômica do consumidor, validação de conteúdo e aprovação técnica da remoção.

Um resultado negativo em qualquer item significa **preservar** e registrar o bloqueio em `D02`.

## Coleções que não devem ser apagadas como “docs antigas”

O nome do diretório não define função. `docs/conformance/` contém entradas consumidas por `crates/aipo-cli/tests/conformance.rs`, incluindo extensões `.aipo`, `.stdout`, `.code` e especificações de host. Remover sem migrar paths quebrará fluxos técnicos.

`docs/evidence/` contém resultados históricos com revisão e limites, como [P07-G01](https://github.com/poppyTM/aipo-lang/blob/main/docs/evidence/P07-G01-runtime-hardening.md); **não reescrever o passado como resultado atual**. `docs/adp`, `docs/decisions` e `docs/canon` retêm a origem das decisões, inclusive quando uma regra foi alterada.

Até haver substituição completa, `docs/` continua como **arquivo técnico consultável**, enquanto `website/content/` atua como ponto único de navegação e síntese por público.

## Plano de consolidação

| Etapa | Artefato exigido | Estado inicial |
| --- | --- | --- |
| Identificar consumidores e referências de todos os caminhos | Inventário de links e testes por arquivo | Pendente |
| Avaliar autoridade de canon/ADP/ADR | Tabela vigente/superseded por contrato | Parcial |
| Substituir referência de API completa | Páginas técnicas verificadas por backend | Pendente |
| Validar exemplos e tradução | Testes e revisão PT/EN | Pendente |
| Criar redirects, atualizar consumidores e medir quebra de links | Relatório de compatibilidade + CI | Pendente |
| Excluir por lotes pequenos e reversíveis | PR próprio, diff, teste e backup Git | **Não autorizado sem gates** |

## Atualização contínua

Cada documento migrado deve acrescentar uma linha ao `website/MIGRATION.md`: origem, destino, decisão normativa, consumidores alterados, gates, idioma, revisão e status `preserve`, `migrated`, `superseded` ou `safe-to-delete`. Só a última classificação, depois dos sete gates, autoriza a exclusão efetiva.

Esta auditoria é estática. **Não foram reexecutadas suites Rust, benchmarks nem testes de migração de consumidores** nesta inspeção.
