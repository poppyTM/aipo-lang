---
title: Frontend, parser e semântica
---
# Frontend: transformar texto em significado

O frontend define onde o código-fonte deixa de ser uma sequência de caracteres e passa a ter estrutura e comportamento verificáveis.

## Etapas

1. **Source** conserva arquivos, offsets e mapeamento das posições.
2. **Lexer** identifica tokens, literais e construções lexicais inválidas.
3. **Parser / CST** produz estrutura com recuperação de erros, preservando informação suficiente para diagnósticos e ferramentas.
4. **AST e HIR** separam a superfície textual das construções semânticas utilizadas adiante.
5. **Sema** resolve escopos, contratos e regras de mutação; deve reportar erros estáticos sem depender da VM.

## Contratos não negociáveis

- Todo span de diagnóstico aponta a fonte original de forma estável.
- O parser não usa recursão ilimitada em texto hostil.
- Formas antigas não devem ser promovidas a canônicas por estarem presentes no lexer.
- Mudança de keyword, associatividade ou sintaxe exige decisão aprovada, fixture e atualização da referência.
- A comparação de `check` com execução é parte da qualidade: o journal identificou nomes de membros aceitos no check e recusados em runtime.

## Problemas de migração conhecidos

O [ADR-001](https://github.com/poppy-team/aipo-lang/blob/main/docs/decisions/adr-001-canonical-syntax.md) descreve remoção de `end`, `div`, `impl`, `self!` e `satisfy` e a adoção das formas aprovadas. [SYNTAX.md](https://github.com/poppy-team/aipo-lang/blob/main/SYNTAX.md) lista os deltas conhecidos com o parser na revisão correspondente.

O [journal de drift](https://github.com/poppy-team/aipo-lang/blob/main/docs/journal/2026-10-05-syntax-drift.md) encontrou divergências de campos tipados, importação e membros. Antes de corrigir, reproduza no HEAD e confira se ainda existem.

## Definição de pronto

Um ajuste de frontend inclui teste positivo, teste negativo, diagnóstico acessível e, se mudar comportamento público, atualização de spec/guia. Só depois disso se altera a documentação do livro como “implementado”.
