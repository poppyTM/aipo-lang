# Migration ledger — old docs to website/

The new website is **canonical for navigation**, not a replacement for semantically authoritative implementation facts until conflicts are adjudicated.

| Old path | New destination | Decision |
| --- | --- | --- |
| docs/getting-started | website/content/learn + guides | Re-authored, examples still need gate |
| docs/manual | learn + concepts + reference | Split by cognitive prerequisites |
| docs/canon | website/content/engineering/decisions + existing canon | Preserve source, no blanket override |
| docs/architecture | website/content/engineering | Reorganize by crate/domain boundaries |
| docs/adp + docs/decisions | engineering/decisions | Do not flatten conflicting statuses |
| docs/trajectory + docs/waves | archive + engineering | Historical delivery, not current capability |
| docs/evidence + docs/journal | archive + evidence links | Immutable audit trail; avoid deleting |
| docs/llm + docs/PRUMO.md | engineering/agents | Lean progressive loading |
| docs/studies | engineering/studies | Keep pinned research files and citations |
| docs/en | website/content/en | New translation is partial; old remains |
| docs/conformance | Unchanged | Executable corpus and golden files |

## Exit criteria before deleting or redirecting old documents

1. Link/consumer audit, including GitHub Actions, Prumo, scripts, and open PRs.
2. Authoritative decision per semantic topic, with supersession table.
3. Examples compiled/executed for documented backend and recorded output.
4. Full content parity of reference pages (including API signatures).
5. English translation reviewed or explicitly marked partial.
6. CI succeeds for build, links, MD, code examples, and library tests.
7. 301/compatibility mapping for published URLs before switching deployment.

## Remaining obligations

The current branch delivers a full new navigable structure and editorial re-authoring; it does **not** certify every example, automate full stdlib API extraction, migrate every historical technical paragraph, or deploy the site. Those claims require build and conformance runs, plus explicit approval for superseding conflicting language contracts.

## Auditoria de escopo e exclusão (2026-10-09)

A [auditoria detalhada](content/engineering/legacy-audit.md) registra 485 arquivos em `docs/`: 175 de conformidade, 105 na seção inglesa, 35 em canon, 24 em evidências, 28 em decisões/adp e demais domínios. A migração **não foi concluída** por equivalência: `docs/` permanece intocado nesta entrega.

Para cada arquivo candidato a remoção, registrar: origem; nova rota; consumidor; autoridade e supersession; cobertura de API/exemplos; estado de tradução; redirect; execução de CI; decisão `preserve|migrated|superseded|safe-to-delete`. Não remover arquivos sem todos os gates. Progresso da migração: [D02](content/progress/index.md).

A página de [progresso](content/progress/index.md) e o [protocolo](content/engineering/progress-protocol.md) tornam essa obrigação verificável em PRs posteriores. A publicação do portal via Vercel foi efetuada anteriormente; esta entrega não altera o projeto de deploy.
