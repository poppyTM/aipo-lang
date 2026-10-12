---
title: Histórico e migração documental
---
# Histórico preservado

O novo website substitui a **navegação editorial** antiga por Livro, Guias, Referência, Conceitos e Engenharia. O conteúdo anterior continua em `docs/` e em arquivos da raiz do repositório para evitar perda de decisões, fixtures, registros de auditoria e rastreabilidade.

## Onde encontrar o legado

| Coleção | Uso legítimo |
| --- | --- |
| [docs/canon](https://github.com/poppy-team/aipo-lang/tree/main/docs/canon) | Especificações originais e capítulos históricos (incluindo fase Odin) |
| [docs/adp](https://github.com/poppy-team/aipo-lang/tree/main/docs/adp) | Decisões detalhadas |
| [docs/evidence](https://github.com/poppy-team/aipo-lang/tree/main/docs/evidence) | Evidência de metas e entregas |
| [docs/journal](https://github.com/poppy-team/aipo-lang/tree/main/docs/journal) | Auditorias e decisões em contexto |
| [docs/studies](https://github.com/poppy-team/aipo-lang/tree/main/docs/studies) | Referências técnicas e síntese |
| [docs/conformance](https://github.com/poppy-team/aipo-lang/tree/main/docs/conformance) | Corpus ativo de testes; **não apagar** |
| [docs/en](https://github.com/poppy-team/aipo-lang/tree/main/docs/en) | Traduções do site anterior |

## Como consolidar sem perder informação

1. Identificar consumidor do arquivo (links, build, testes, Prumo, scripts).
2. Selecionar a página normativa que vai substituí-lo.
3. Copiar somente regras realmente vigentes e registrar a origem.
4. Resolver conflitos com decisões explícitas.
5. Publicar redirecionamento/aviso de supersession.
6. Remover arquivos somente quando o caminho antigo deixar de ser consumido ou tiver rota de compatibilidade.

Até concluir essas etapas, o legado é histórico preservado — **não autoridade automática da superfície nova**. [Conflitos identificados](/engineering/decisions/conflicts).

## Auditoria atual

A [auditoria de 2026-10-09](/engineering/legacy-audit) identificou 485 arquivos legados e explica por que o corpus de conformidade, a referência inglesa, as decisões e os registros de evidência ainda precisam ser preservados. Acompanhe os critérios da migração no [processo D02](/progress/#D02).
