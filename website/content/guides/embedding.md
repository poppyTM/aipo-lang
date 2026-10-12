---
title: Embarcar Aipo numa aplicação
---
# Embarcar Aipo numa aplicação

Aipo pode ser usado como linguagem de scripting controlada por outra aplicação. Há mais de uma fronteira de integração: APIs Rust, ABI C, esquema de superfície de host (AHS) e adaptadores específicos.

## Contratos de segurança

O host define quais funções disponibiliza e quais capacidades realmente concede. Uma descrição de superfície não concede permissões e não instala implementações de funções.

## Caminho recomendado

1. Escolha a fronteira: Rust nativo, C ABI ou integração via processo/CLI.
2. Inspecione `crates/aipo-host` e, para C/C++, `crates/aipo-c-abi/include/aipo.h`.
3. Descreva a superfície autorizada, inclua limites de execução e políticas para handles.
4. Prove um caso pequeno com estado e falha, antes de integrar frameworks de UI ou jogos.
5. Confirme se o backend escolhido oferece a semântica utilizada.

O CLI aceita uma descrição AHS explicitamente por invocação:

```bash
aipo check app.aipo --ahs host.json
```

`host.json` não é fornecido automaticamente: precisa seguir o schema real do projeto.

Veja [Arquitetura de embedding](/engineering/embedding) e [Referência de CLI](/reference/cli).
