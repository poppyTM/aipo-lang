---
title: Stdlib, CLI e pacotes
---
# Stdlib e fronteiras de bibliotecas

A linguagem precisa separar semântica intrínseca de funções adicionadas pela biblioteca padrão. Isso evita aumentar gramática e runtime por necessidades de UI, jogos ou HTTP.

## Ownership

- `aipo-stdlib`: funções, métodos de coleções e módulos nativos.
- `aipo-runtime`: módulos em execução e registro de nativos.
- `aipo-package`: identidades, manifestos, lockfiles, resolução e cache.
- `aipo-cli`: comandos, flags, diagnóstico e integração entre camadas.
- `packages/`: experimentos e pacotes escritos em Aipo, sujeitos à compatibilidade da gramática atual.

## Princípios

1. Uma assinatura pública deve ter referência verificável.
2. Nenhum alias novo deve ser introduzido por conveniência sem decisão de linguagem.
3. Acesso a arquivos, ambiente e tempo pode depender de capacidade do host.
4. Dependências remotas devem permanecer pinadas e auditáveis.
5. Um lockfile íntegro não significa que o código remoto é benigno.
6. O CLI deve produzir saídas e códigos de erro consistentes com sua referência.

## Drift conhecido

O journal de 5 de outubro registrou erros de parser em `aipo.ui`, depois corrigidos naquele pacote, e diferenças de formatação em outros pacotes. Essas medições são históricas; reproduza os comandos no commit atual antes de afirmar que continuam.

## Documento de API

A [referência resumida](/reference/stdlib) serve humanos. O ideal técnico é gerar uma referência detalhada a partir de um inventário testável de assinaturas/arity/capabilities, sem digitar manualmente a mesma verdade em três lugares.

Consulte [módulos](/guides/modules), [CLI](/reference/cli) e [pacotes históricos](https://github.com/poppy-team/aipo-lang/tree/main/docs/packages).
