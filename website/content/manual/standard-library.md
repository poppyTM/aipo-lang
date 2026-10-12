---
title: "Biblioteca padrão: escolha pelo objetivo"
description: "Referência de uso organizada por tarefas e comportamento do Aipo."
---

# Biblioteca padrão: escolha pelo objetivo

A biblioteca do Aipo é mais ampla do que uma lista mínima de `print` e `math`. No código Rust atual, `crates/aipo-stdlib/src/lib.rs` registra módulos e funções nativas para trabalhar com coleções, texto, arquivos, tarefas, segurança de host e dados estruturados.

**Escolha o que pretende fazer**, depois abra a seção de API. Este manual separa nomes da biblioteca de APIs opcionais do host.

## Texto e coleções

| Quero… | Comece por |
| --- | --- |
| Limpar e transformar texto | `string`, métodos de `String` |
| Mapear/filtrar itens | métodos de `List` |
| Consultar por chave | `Dict` |
| Evitar duplicatas | `Set` |
| Avaliar quando necessário | `Sequence` |

[Aprender texto](/manual/text) · [Aprender coleções](/manual/collections) · [Exemplos](/examples/).

## Matemática e dados

| Objetivo | Módulo |
| --- | --- |
| Trigonometria, funções matemáticas | `math` |
| Gerador determinístico com sementes | `random` |
| Leitura e geração de JSON | `json` |
| Conversão entre textos e bytes | `encoding` |
| Bytes, endianess e varints | `binary` |
| Operações de expressão regular (feature opcional) | `regex` |

## Paths, URLs e tempo

| Objetivo | Módulo |
| --- | --- |
| Normalização e composição de caminho | `path` |
| Analisar/compor URLs | `url` |
| Datas civis e relógios | `time` |
| Conversões e intervalos | `Duration` |

## Testes e observabilidade

| Objetivo | Módulo |
| --- | --- |
| Funções de teste e assertivas | `testing`, `expect` |
| Logs estruturados | `log` |
| Tarefas cooperativas | `task` |

## Serviços que dependem do host

| Objetivo | Módulo | Regra |
| --- | --- | --- |
| Ler variáveis de ambiente | `env` | Capability explícita |
| Ler arquivos de forma assíncrona | `fs` | FilesystemSource e permissão |
| Comandos de shell (quando integrados pelo host) | `sh` | Nunca presumir autorização implícita |
| Entrada e saída | `io` | Capacidades podem variar por host |

**Importante:** `env` e `fs` podem estar registrados mesmo quando a capability foi negada. A presença da função não é permissão de leitura. Regex pode exigir a feature Rust `regex`.

Para uma listagem de APIs ainda mais extensa, consulte a [referência antiga detalhada](https://github.com/poppyTM/aipo-lang/blob/main/docs/manual/stdlib.md) e [o código de registro atual](https://github.com/poppyTM/aipo-lang/blob/main/crates/aipo-stdlib/src/lib.rs). A implementação é a referência para descobrir se uma chamada permanece disponível.

**Pratique:** comece pelo [exemplo de transformações](/examples/15-pipelines-and-trailing-blocks) ou [strings](/examples/09-strings-unicode-and-formatting).
