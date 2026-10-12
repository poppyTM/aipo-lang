---
title: Biblioteca padrão
description: Índice completo de APIs implementadas e regras de capacidades.
---

# Biblioteca padrão

Procure uma função **pelo problema que quer resolver**, não pela ordem em que foi implementada. O [manual por objetivo](/manual/standard-library) explica para que serve cada módulo. Aqui ficam as funções extraídas do registro real em Rust.

## Módulos disponíveis no código

- [**collections** — Coleções: List, Dict, Set e Sequence](/reference/api/collections)
- [**string** — Texto, Unicode e formatação](/reference/api/string)
- [**math** — Matemática](/reference/api/math)
- [**json** — Leitura e escrita JSON](/reference/api/json)
- [**random** — Aleatoriedade e sementes](/reference/api/random)
- [**binary** — Leitura e escrita binária](/reference/api/binary)
- [**encoding** — Base64, hex e UTF-8](/reference/api/encoding)
- [**path** — Paths e manipulação de diretórios](/reference/api/path)
- [**url** — URLs](/reference/api/url)
- [**regex** — Expressões regulares (opcional)](/reference/api/regex)
- [**time** — Datas e relógios](/reference/api/time)
- [**task** — Tarefas cooperativas](/reference/api/task)
- [**testing** — Asserções e testes](/reference/api/testing)
- [**log** — Logs estruturados](/reference/api/log)
- [**sh** — Shell e execução (host)](/reference/api/sh)
- [**io** — Entrada/saída](/reference/api/io)
- [**env** — Variáveis de ambiente (capability)](/reference/api/env)
- [**fs** — Sistema de arquivos (capability)](/reference/api/fs)

## Globals e conversões

A prelude também fornece funções como `len`, `copy`, `same`, `some`, `Int`, `Float`, `Byte` e `fail`. `Bytes` fornece buffers binários. Consulte as implementações em [`aipo-stdlib/src/lib.rs`](https://github.com/poppyTM/aipo-lang/blob/main/crates/aipo-stdlib/src/lib.rs).

## Cuidados fundamentais

- **Registrada ≠ disponível:** uma API de `env`, `fs` ou relógio pode negar acesso se o host não concedeu permissão.
- **Backend importa:** módulos e tipos podem ter cobertura diferente na VM, RegVM, JS e Wasm.
- **Feature opcional importa:** a biblioteca regex depende de uma feature Rust; não presuma que está sempre compilada.
- **API da VM vs API do host:** controles gráficos de `aipo-game-host` não são funções disponíveis por padrão no CLI.

As páginas acima listam nomes encontrados no código-fonte. A versão atual deste portal **não afirma ter executado todas as chamadas**. Para assinaturas rigorosas e exceções, abra o arquivo Rust vinculado em cada entrada.

[Começar com a stdlib](/manual/standard-library) · [Compatibilidade](/reference/status) · [Exemplos reais](/examples/).
