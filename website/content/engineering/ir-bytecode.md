---
title: IR, bytecode e verificação
---
# Representações intermediárias

Apoiar diversos destinos sem duplicar a semântica exige delimitar quais transformações acontecem antes de cada backend.

## Core IR

`aipo-ir` representa operações independentes do destino. Uma transformação precisa preservar ordem, efeitos observáveis, contratos e falhas. Não mova regras semânticas arbitrárias para emissores individuais.

## Bytecode

`aipo-bytecode` define formatos, opcodes, emissão e verificação para as VMs. Cada operando tem papel e limite. O verificador deve rejeitar índices de pool inválidos, jumps fora do módulo, frames inconsistentes e entradas malformadas antes de execução insegura.

## Register backend

O emissor de registradores foi corrigido na revisão de outubro para reportar instruções não representadas em vez de convertê-las silenciosamente. Ainda é experimental: compatibilidade com closures, async, hooks e outras construções é parcial.

## Evidência necessária

- Golden ou fixture reproduzível por opcode.
- Caso positivo e caso hostil de decode/verifier.
- Execução na VM de referência.
- Verificação de erro explícito para operação não suportada.
- Registro de baseline de performance quando se alterar despacho ou formato.

Veja [Runtime](/engineering/runtime), [Backends](/engineering/backends), [corpus original](https://github.com/poppy-team/aipo-lang/tree/main/docs/conformance) e [hardening](https://github.com/poppy-team/aipo-lang/blob/main/docs/development/runtime-hardening-guide.md).
