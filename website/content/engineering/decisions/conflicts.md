---
title: Conflitos de autoridade identificados
---
# Conflitos documentais a reconciliar

Este registro conserva **evidências de inconsistência editorial**, não decide sem autorização questões de semântica de linguagem.

## ADP-003 — Execução de scripts não confiáveis

- `docs/decisions/adp-003.md`: “Aprovado & Implementado”.
- `docs/adp/ADP-003-execution-budgets.md`: política unificada de memória/interrupção/sinais ainda rascunho, com APIs parciais de orçamento implementadas.

**Conciliação necessária:** distinguir budget de instruções já existente das garantias completas de sandbox. Não vender um orçamento parcial como contenção universal.

## ADP-004 — Unicode e identificadores

- `docs/decisions/adp-004.md`: normalização NFC de identificadores aprovada/implementada.
- `docs/adp/ADP-004-unicode-identifier-policy.md`: documento de proposta relata ausência de NFC automático no source loader, regras de caracteres existentes e questões de segurança abertas.

**Conciliação necessária:** comparar lexer, normalização e testes no HEAD; separar normalização de strings de identificadores e decidir qual contrato é realmente canônico.

## ADR-001 — Nova sintaxe x parser

A sintaxe aprovada em `docs/decisions/adr-001-canonical-syntax.md` difere do lexer/parser histórico (end/div/impl/satisfy/self!). `SYNTAX.md` declara explicitamente que a superfície nova é alvo. O manual público contém exemplos de diferentes gerações.

**Conciliação necessária:** matriz de features por versão/backend e migração dos exemplos acompanhada de conformance.

## Exemplos e conteúdo introdutório

`docs/getting-started/first-program.md`, `docs/examples/index.md` e `examples/` usam formas e níveis de complexidade distintos. Um tutorial de cinco minutos apresenta contratos e invariantes antes de variáveis simples.

**Conciliação necessária:** novos capítulos em `website/content/learn` e exemplos com outputs verificados. Antigos arquivos continuam preservados, porém deixam de definir a ordem de aprendizado.

## Estado de verificação

O merge de runtime P07-G01 entregou implementação e documentação, mas registrou testes não executados. O portal deve diferenciar “código disponível” de “testes verificados”.

## Política de resolução

Para cada conflito: (1) reproduzir código/teste no HEAD; (2) identificar norma aprovada e escopo; (3) propor conciliação; (4) obter decisão; (5) atualizar fonte canônica; (6) ajustar docs derivadas, fixtures e redirecionamentos; (7) registrar supersession. Recência sozinha não dá autoridade.
