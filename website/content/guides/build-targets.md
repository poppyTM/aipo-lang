---
title: Gerar JavaScript e WebAssembly
---
# Gerar JavaScript e WebAssembly

O Aipo possui destinos de execução diferentes. A semântica efetiva depende da cobertura do destino e das flags de build.

## JavaScript

```bash
cargo run -q -p aipo-cli -- build examples/06_variables_and_values.aipo --target js --out dist/
```

O comando emite arquivos JavaScript e artefatos de runtime necessários ao destino. Confira os caminhos e o formato no `--help` do binário instalado.

## WebAssembly

```bash
cargo run -q -p aipo-cli -- build examples/06_variables_and_values.aipo --target wasm --out dist/
```

Wasm requer recursos e dependências específicos; a presença de um compilador Wasm não implica equivalência automática de toda a biblioteca padrão.

## Verificar compatibilidade

- Escolha um programa pequeno que já exista no corpus.
- Compare comportamento de VM e destino gerado.
- Anote revisão e flags.
- Não generalize a partir de um único resultado.

Consulte [Estado dos backends](/reference/status) e [Implementação dos backends](/engineering/backends).
