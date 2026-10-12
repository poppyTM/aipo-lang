---
title: Qualidade e conformidade
---
# Estratégia de qualidade

Documentação, implementação e testes devem evoluir juntos, mas não são a mesma evidência. Adicionar um teste não implica executá-lo.

## Níveis de verificação

| Nível | Exemplo | Prova |
| --- | --- | --- |
| Markdown | Links, metadata, sintaxe do site | `npm run check` |
| Build do site | VitePress e assets | `npm run build` |
| Conformance Aipo | Fixture `.aipo` e `.stdout`/`.code` | Runner e saída |
| Rust | `cargo test --workspace` | Relatório e commit |
| Diferencial | VM x JS x Wasm x RegVM | Corpus por destino |
| Segurança | Entradas hostis, orçamento, ABI | Casos negativos e testes |
| Performance | Baseline reproduzível | Ambiente, amostra, mediana, variância |

## Gates recomendados de documentação

- Rotas, tradução declarada e links válidos.
- Um documento canônico por responsabilidade.
- Snippets publicados como verificados somente após execução.
- Metadados de origem, versão, backend e expectativa para exemplos.
- Alterações de API sincronizadas com referência e guia correspondente.
- Relatórios de CI não mascaram falhas com `|| true`.

## Referências preexistentes

- [Conformance](https://github.com/poppy-team/aipo-lang/blob/main/docs/conformance/README.md).
- [Estratégia de teste](https://github.com/poppy-team/aipo-lang/blob/main/docs/development/testing-strategy.md).
- [Evidência da revisão P07-G01](https://github.com/poppy-team/aipo-lang/blob/main/docs/evidence/P07-G01-runtime-hardening.md).

**Limite desta entrega:** a documentação foi criada em uma branch separada. Não reivindicar execução da suite Rust, conformance ou build do website até esses comandos serem efetivamente executados.


## Contrato de progresso nos PRs

Além de checar `website/`, a CI executa `Aipo progress sync` para alterações de código, contratos e fixtures. O script `website/scripts/progress_delta.py` exige o diff do JSON do progresso no mesmo PR. Não equivale a execução dos testes Rust nem transforma evidência histórica em resultado atual. [Protocolo](/engineering/progress-protocol).
