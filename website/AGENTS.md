# Aipo website — scope for coding agents

This folder is the new editorial home for Aipo documentation. Its contents must **not** silently override the repository's canonical language decisions or implementation. Read `content/engineering/agents/reading.md` and `content/engineering/decisions/conflicts.md` first.

## Editing contract
- Reader tutorials: `content/learn`, guided by `content/_meta/chapters.json`.
- Task-oriented: `content/guides`.
- Reference facts: `content/reference`.
- Architecture: `content/engineering`.
- Original repository docs and `docs/conformance`: preserve until downstream consumers have migrated.
- Every example must distinguish proposed target syntax from implemented surface.
- Only claim tests passed when executed on the current commit with logged commands/results.
- Accessibility: keyboard navigation, contrast, readable code, reduced motion, cognitive load.
- Run `npm run check` and `npm run build` in `website`; execute CLI example gates when appropriate.

Do not duplicate the whole Prumo workforce. Follow [the source catalog](https://github.com/poppy-lat/prumo/tree/main/src/prumo/resources/workforce) selectively with task-appropriate permissions.

## Protocolo de progresso (obrigatório)

Antes de qualquer slice, ler [progresso](content/progress/index.md), [protocolo](content/engineering/progress-protocol.md) e [auditoria legado](content/engineering/legacy-audit.md). No mesmo PR, atualizar `public/progress/tasks.json` (IDs afetados, checkpoints, gates, evidência e datas). Nunca apresentar teste antigo ou presença de crate como nova validação. Rodar `npm run check` e `npm run build` a partir de `website/`. É vedado excluir `docs/` antes de cumprir os gates de paridade e consumidores descritos na auditoria.


**CI de sincronização:** o workflow `.github/workflows/progress-sync.yml` verifica PRs que mudam código/fixtures e exige `website/public/progress/tasks.json` no mesmo diff. Documentar IDs afetados e evidências; apenas alterar o JSON para contornar o gate não conclui o trabalho.

## Documentação centrada na pessoa

A entrada pública deve seguir **Começar → Manual → Exemplos → Referência**. Não colocar implementação, status interno ou diretivas de agentes na navegação do leitor comum. O manual vive em `content/manual/`, com uma pergunta principal por página; exemplos completos de `content/examples/` devem reproduzir exatamente `examples/*.aipo`. A referência de APIs é derivada do registro Rust (não de especulação de LLM). Não apagar o corpus de conformidade. A revisão editorial deve cumprir `content/engineering/accessibility.md`: leitura linear, títulos previsíveis, navegação curta, teclado, reduced motion, preferência de foco/texto maior e ausência de distrações.

Gates locais no diretório `website`: `npm run check` e `npm run build`; a verificação `check:usage` confere paridade textual dos 26 exemplos. Ela não executa programas Aipo, não certifica WCAG e não substitui validação com leitor de tela.
