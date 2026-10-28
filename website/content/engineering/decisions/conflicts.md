---
title: Conflitos de autoridade identificados
---
# Conflitos documentais a reconciliar

Este registro conserva **evidências de inconsistência editorial**, não decide sem autorização questões de semântica de linguagem.

## Autoridade vigente por tema

| Tema | Fonte canônica | Estado |
| --- | --- | --- |
| Estado da implementação | `PROJECT_STATE.md` + `.ai/goals/` | P07-G01/G02/G03 DONE; release v0.1.0 publicada |
| Sintaxe implementada | `SYNTAX.md` + testes do parser | Normativo para a superfície atual; `fixed` é legado aceito |
| Arquitetura e crates | `docs/architecture/crates.md` + `docs/crates/crate-contracts.md` | Derivado de `cargo metadata` |
| Conformance | `docs/conformance/README.md` + fixtures | Executável; 36 programas e 29 diagnósticos |
| Diagnósticos | `docs/diagnostics/catalog.md` + `aipo-diagnostics` | Códigos estáveis |
| Progresso e evidência | `website/public/progress/tasks.json` | Painel auditável; baseline revisado em 2026-10-27 |
| Documentos legados | `docs/en/`, `docs/journal/`, `docs/evidence/` | Históricos; não certificam o HEAD |
| Release | `docs/adp/ADP-008-v0.1.0-language-release.md` + `PROJECT_STATE.md` | `v0.1.0` publicada em 2026-10-28 |

## ADP-003 — Execução de scripts não confiáveis

- `docs/decisions/adp-003.md`: “Aprovado & Implementado”.
- `docs/adp/ADP-003-execution-budgets.md`: política unificada de memória/interrupção/sinais ainda rascunho, com APIs parciais de orçamento implementadas.

**Conciliação necessária:** distinguir budget de instruções já existente das garantias completas de sandbox. Não vender um orçamento parcial como contenção universal.

## ADP-004 — Unicode e identificadores

- `docs/decisions/adp-004.md`: normalização NFC de identificadores aprovada/implementada.
- `docs/adp/ADP-004-unicode-identifier-policy.md`: documento de proposta relata ausência de NFC automático no source loader, regras de caracteres existentes e questões de segurança abertas.

**Conciliação necessária:** comparar lexer, normalização e testes no HEAD; separar normalização de strings de identificadores e decidir qual contrato é realmente canônico.

## ADR-001 — Nova sintaxe x parser

O parser atual removeu `end`, `div`, `self!`, `impl` e `satisfy`; `Tipo:metodo`, `var self`, `{ }` e `#!` são a superfície canônica. `fixed` continua aceito como forma legada. A matriz completa está em [`SYNTAX.md`](https://github.com/poppyTM/aipo-lang/blob/main/SYNTAX.md) §16 e é fixada pelos testes do parser.

**Pendência:** remover definitivamente `fixed` exige atualizar fixtures de conformance e exemplos; não fazer isso sem um ADP e uma migração única.

## Exemplos e conteúdo introdutório

`docs/getting-started/first-program.md`, `docs/examples/index.md` e `examples/` usam formas e níveis de complexidade distintos. Um tutorial de cinco minutos apresenta contratos e invariantes antes de variáveis simples.

**Conciliação necessária:** novos capítulos em `website/content/learn` e exemplos com outputs verificados. Antigos arquivos continuam preservados, porém deixam de definir a ordem de aprendizado.

## Estado de verificação

A reconciliação de 2026-10-27 executou o workspace completo (1.130 testes), conformance VM/Reg, Poppy, eGUI, game host, validadores estáticos do portal e build VitePress. Código disponível, teste executado e release certificada continuam sendo estados diferentes.

## Política de resolução

Para cada conflito: (1) reproduzir código/teste no HEAD; (2) identificar norma aprovada e escopo; (3) propor conciliação; (4) obter decisão; (5) atualizar fonte canônica; (6) ajustar docs derivadas, fixtures e redirecionamentos; (7) registrar supersession. Recência sozinha não dá autoridade.
