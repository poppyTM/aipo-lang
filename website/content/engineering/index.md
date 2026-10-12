---
title: Engenharia Aipo
---
# Engenharia da linguagem

Esta seção é para quem implementa, revisa, testa ou integra o compilador e os runtimes. **Não é o percurso recomendado para aprender a programar em Aipo**. Comece pelo [Livro da Linguagem](/learn/) se quiser usá-la.

## Ordem de leitura sugerida

1. [Arquitetura e ownership](/engineering/architecture)
2. [Domínio diretamente envolvido](/engineering/frontend)
3. [Decisões canônicas e conflitos](/engineering/decisions/)
4. [Implementação e evidências](/engineering/quality)
5. [Orientação para code agents](/engineering/agents/)

## Domínios técnicos

| Domínio | Responsabilidade |
| --- | --- |
| [Frontend](/engineering/frontend) | Fonte, lexer, CST, AST, HIR, semântica |
| [IR e bytecode](/engineering/ir-bytecode) | Representações intermediárias e verificadores |
| [Runtime](/engineering/runtime) | Valores, pilha, registradores, execução e falhas |
| [Backends](/engineering/backends) | Emissão JavaScript e WebAssembly |
| [Stdlib e pacotes](/engineering/stdlib-packages) | Superfície pública e resolução de dependências |
| [Host e C ABI](/engineering/embedding) | Embedding, capacidades, handles e interop |
| [Qualidade](/engineering/quality) | Conformance, segurança e CI |
| [Acessibilidade](/engineering/accessibility) | Sintaxe, diagnósticos e documentação |
| [Estudos](/engineering/studies) | Referências técnicas, hipóteses e benchmarks |
| [Guia P07-G02](/engineering/runtime-and-tooling) | Mudanças, motivos, uso, reimplementação e limites |

Documentos aqui descrevem o **estado encontrado e os contratos pretendidos separadamente**. Sempre declare commit, provas e limites ao afirmar que algo está pronto.

## Progresso e migração

- [Painel de progresso da implementação](/progress/): checkpoints, gates, limitações e evidências.
- [Protocolo obrigatório de atualização](/engineering/progress-protocol): como registrar mudança, regressão e validação.
- [Auditoria do legado](/engineering/legacy-audit): critérios para consolidar e excluir documentação antiga com segurança.
