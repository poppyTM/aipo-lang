# Aipo-lang × omniLua — Estudos e Auditoria Técnica

Este diretório abriga a documentação operacional e técnica de auditoria, reaproveitamento e evolução da **Aipo-lang** orientada pela referência do **omniLua** (conforme diretriz [`AIPO_OMNILUA_DIRETRIZ_TECNICA_CODE_AGENTS.md`](file:///home/raillen/Documentos/aipo-lang/AIPO_OMNILUA_DIRETRIZ_TECNICA_CODE_AGENTS.md)).

## 1. Princípio Fundamental

> **A Aipo não é um dialeto de Lua.** Nenhuma semântica, gramática ou filosofia da Aipo deve ser alterada para acomodar conveniências de Lua. O omniLua é utilizado estritamente como **fonte de técnicas de implementação de runtime, sandboxing, alocação e oráculos em Pure Rust (`unsafe_code = "forbid"`)**, submetido a rigorosos testes de evidência e paridade.

## 2. Commits de Referência Fixados

- **Aipo-lang:** `58f8bc15de13a5eea5834a18f62a6e74bae73349` (Branch: `feat/host-ahs-headless-conformance`)
- **omniLua:** `90dcf85a8cfb95305b77056af00a114fd0df30a1` (Local: `~/aipo-refs/omnilua`, origin: [ianm199/omnilua](https://github.com/ianm199/omnilua))
- **Licenças:**
  - Aipo: MIT License, Copyright (c) 2026 Aipo Contributors.
  - omniLua: MIT License, Copyright (c) 2026 lua-rs-port contributors (trabalho derivado de Lua 5.4.7 PUC-Rio, mantendo NOTICE correspondente).

## 3. Estrutura de Artefatos da Fase 0

| Arquivo | Descrição |
|---|---|
| [`AIPO_CURRENT_STATE.md`](file:///home/raillen/Documentos/aipo-lang/docs/studies/omnilua/AIPO_CURRENT_STATE.md) | Inventário comprovado dos subsistemas da Aipo (o que existe, o que é parcial e lacunas). |
| [`OMNILUA_SOURCE_MAP.md`](file:///home/raillen/Documentos/aipo-lang/docs/studies/omnilua/OMNILUA_SOURCE_MAP.md) | Mapeamento de crates, arquivos e mecanismos do omniLua relevantes para a Aipo. |
| [`BASELINE.md`](file:///home/raillen/Documentos/aipo-lang/docs/studies/omnilua/BASELINE.md) | Baseline congelado de testes, compilação e métricas da Aipo. |
| [`SEMANTIC_COMPATIBILITY.md`](file:///home/raillen/Documentos/aipo-lang/docs/studies/omnilua/SEMANTIC_COMPATIBILITY.md) | Matriz rigorosa de compatibilidade entre o modelo da Aipo e o modelo de Lua/omniLua. |
| [`OPPORTUNITIES.md`](file:///home/raillen/Documentos/aipo-lang/docs/studies/omnilua/OPPORTUNITIES.md) | Matriz de oportunidades categorizada por `COPY / ADAPT / INSPIRE / DEFER / REJECT`. |
| [`RISKS.md`](file:///home/raillen/Documentos/aipo-lang/docs/studies/omnilua/RISKS.md) | Mapeamento de ameaças, riscos de regressão e salvaguardas arquiteturais. |

## 4. Próximas Fases

- **Fase 1:** Treinamento de validação e infraestrutura de oráculos e canários.
- **Fase 2:** Instrumentação e hardening de fronteiras (leak canaries e contadores de alocação).
- **Fase 3:** Piloto de Sandboxing (Instruction Budget compartilhado uncatchable + Memory Ceiling amostrado).
- **Fase 4:** Piloto de Embedding Scoped (Empréstimos de host com lifetime explícito).
