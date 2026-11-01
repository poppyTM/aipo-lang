---
title: API string
---

# string

Módulo nativo de manipulação de texto. Strings são UTF-8 normalizadas em NFC; as operações contam **pontos de código** (não bytes), salvo `byte_len`.

Fonte: [crates/aipo-stdlib/src/string.rs](https://github.com/poppyTM/aipo-lang/blob/main/crates/aipo-stdlib/src/string.rs).

## Consulta e inspeção

| Função | Assinatura | Descrição |
| --- | --- | --- |
| `string.len(text)` | `String -> Int` | Conta pontos de código (NFC). |
| `string.byte_len(text)` | `String -> Int` | Comprimento em bytes UTF-8. |
| `string.contains(text, needle)` | `(String, String) -> Bool` | Substring presente. |
| `string.starts_with(text, prefix)` | `(String, String) -> Bool` | Começa com prefixo. |
| `string.ends_with(text, suffix)` | `(String, String) -> Bool` | Termina com sufixo. |
| `string.find(text, needle)` | `(String, String) -> Int?` | Índice do primeiro match (ponto de código), ou `none`. |
| `string.graphemes(text)` | `String -> List<String>` | Segmentos de grafema estendido. |
| `string.words(text)` | `String -> List<String>` | Palavras. |
| `string.lines(text)` | `String -> List<String>` | Linhas. |

## Transformação

| Função | Assinatura | Descrição |
| --- | --- | --- |
| `string.lower(text)` | `String -> String` | Minúsculas (NFC na saída). |
| `string.upper(text)` | `String -> String` | Maiúsculas (NFC na saída). |
| `string.capitalize(text)` | `String -> String` | Primeiro caractere caled, restante minúsculo (NFC). |
| `string.casefold(text)` | `String -> String` | Case folding Unicode. |
| `string.reverse(text)` | `String -> String` | Reverte grafemas (NFC na saída). |
| `string.trim(text)` | `String -> String` | Remove espaços Unicode nas pontas. |

## Estrutura

| Função | Assinatura | Descrição |
| --- | --- | --- |
| `string.split(text, sep)` | `(String, String) -> List<String>` | Divide por separador não vazio; campos vazios preservados. |
| `string.join(sep, list)` | `(String, List<String>) -> String` | Junta lista com separador (ordem canônica: separador primeiro). |
| `string.replace(text, from, to)` | `(String, String, String) -> String` | Substitui todas as ocorrências não sobrepostas; `from` não pode ser vazio. |
| `string.slice(text, start, end)` | `(String, Int, Int) -> String` | Substring por índices de caractere `[start, end)`. |
| `string.format(template, values)` | `(String, Dict<String, V>) -> String` | Substitui placeholders `{nome}` pelos valores do dicionário, convertidos textualmente. |

## Backends

- `string.len`, `string.byte_len`, `string.find` (índice de ponto de código), `string.lower`, `string.upper`, `string.trim`, `string.split`, `string.join`, `string.replace`, `string.slice`, `string.format`, `string.reverse`, `string.capitalize` e `string.contains`/`starts_with`/`ends_with` são suportados no WebAssembly (NFC aplicada nas boundaries de construção).
- `string.format` aceita valores `String`, `Int`, `Float` e `Bool` no WebAssembly; outros tipos dependem do backend.
- `string.graphemes`, `string.words`, `string.lines`, `string.casefold` dependem da feature `unicode` e do backend.

[Escolher pelo objetivo](/manual/standard-library) · [Todos os módulos](/reference/stdlib) · [Texto e Unicode](/manual/text).
