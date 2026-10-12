---
title: Manual do Aipo
description: Aprenda os recursos da linguagem por necessidade, com exemplos da implementação.
---

# Manual do Aipo

Encontre o recurso que precisa, aprenda com **um exemplo curto** e abra um programa completo quando quiser explorar. Você não precisa conhecer o compilador para usar a linguagem.

**Primeira vez aqui?** Comece por [instalação e primeiro programa](/start/). Se você já programa, escolha um assunto abaixo.

## 1. Primeiros conceitos

- [Variáveis e valores](/manual/variables): `let`, `var`, tipos e `none`
- [Texto e Unicode](/manual/text): strings, interpolação e métodos
- [Números e Bytes](/manual/numbers): aritmética, conversão e buffers
- [Listas e dicionários](/manual/collections): coleções, filtros e slices
- [Condições e laços](/manual/control-flow): `if`, `each`, `repeat`, `match`
- [Funções](/manual/functions): argumentos, defaults, closures

## 2. Construir programas

- [Structs e métodos](/manual/structs): campos, inicialização e `var self`
- [Interfaces e contratos](/manual/interfaces): comportamento e tipos opcionais
- [Falhas e rollback](/manual/failures): `fail`, `attempt`, `or_else`
- [Módulos e pacotes](/manual/modules-packages): `import`, `export`, cache e lockfiles
- [Enums e padrões](/manual/enums-patterns): variantes, desestruturação e guardas
- [Atualizar structs com `with`](/manual/updates): cópias imutáveis

## 3. Recursos para aprofundar

- [Operadores e expressões](/manual/operators): `?`, `|>`, `with`, literais
- [Async e tarefas](/manual/async): `async fn`, `await`, combinadores
- [Diretivas e testes](/manual/directives): `#!test`, `#!satisfies`, diagnósticos
- [Biblioteca padrão](/manual/standard-library): mapa por função dos módulos

## 4. Usar em um projeto

- [CLI](/manual/cli): executar, testar, formatar e compilar
- [JavaScript, VM e Wasm](/manual/backends): escolher destino com clareza
- [Testar programas](/manual/testing): testes e conformance
- [Jogos, interfaces e hosts](/manual/hosts): scripting integrado a aplicações

## Prefere aprender vendo um programa?

[Exemplos completos](/examples/) apresenta 26 programas reais do repositório, incluindo aplicações de dados, contratos, projetos pequenos, Snake e integração gráfica. Também existe o [livro sequencial](/learn/), mantido como uma rota opcional de aprendizagem.

**Atenção à sintaxe:** o Aipo está evoluindo. O manual dá prioridade às formas usadas no código e nas fixtures, e separa os alvos normativos futuros. [Ver o que é compatível com cada backend](/reference/status).
