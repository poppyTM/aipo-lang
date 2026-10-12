---
title: Modelo de execução e portabilidade
---
# Uma linguagem, diferentes caminhos de execução

Código-fonte passa por tokenização, parsing, análise e representações intermediárias. O caminho final depende do destino: VM de pilha, VM de registradores, JavaScript ou WebAssembly.

## O que é portabilidade?

Não basta emitir um arquivo. O programa precisa preservar as propriedades relevantes: ordem de resultados, erros, contratos, limites, APIs acessíveis e diagnósticos quando prometidos.

A Stack VM serve de referência. O emissor de registradores continua experimental; JS e Wasm devem ser comparados com o corpus e com as opções da build.

## Onde ficam as decisões?

[Arquitetura](/engineering/architecture) explica as fronteiras, [Backends](/engineering/backends) descreve os detalhes e a [matriz de suporte](/reference/status) resume os estados.
