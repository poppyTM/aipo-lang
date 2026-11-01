# Matriz de suporte por backend

**Status:** evidência operacional, não certificação universal.
**Data da medição:** 2026-10-31.
**Fonte:** `aipo plan`, `aipo check --wasm`, `aipo run --engine=vm|reg`, `cargo test --workspace` e corpus `docs/conformance/`.

## Como ler

- **Nativo** significa que o backend escolhido executou o programa sem fallback.
- **Fallback** significa que o plano selecionou a VM canônica com motivo explícito.
- **Saída igual** significa stdout idêntico ao `.stdout` commitado.
- A matriz mede o corpus e os exemplos deste repositório; não promete cobertura de toda a linguagem.

## Programas de conformance

| Backend | Cobertura nativa | Fallback | Saída igual | Observação |
| --- | ---: | ---: | ---: | --- |
| Stack VM | 36/36 | 0 | 36/36 | Referência de semântica |
| RegVM | 20/36 | 16/36 | 36/36 | Fallback: métodos de ordem superior, handlers de falha e IR não representável |
| JavaScript | 36/36 | 0 | 36/36 | Bundle ESM; requer Node com ESM |
| WebAssembly | 29/36 | — | 29/36 | Subset; falhas de compilação emitem `AIPO_COMPILE_WASM_UNSUPPORTED`. Cobre `init`/`invariant` com rollback de mutação, módulo `string` completo (incluindo normalização NFC, case folding e trim Unicode), `Dict` posicional, `await`/`task.spawn`/`task.all` e contratos `T?`. Limites (declarados, não silenciosos): `fn` aninhado ou closure que captura variável do frame externo e combinadores async que exigem agendador. Cada um é recusado com `AIPO_COMPILE_WASM_UNSUPPORTED` e a receita `use --engine=vm`, em vez de produzir saída diferente da VM |

## Exemplos executáveis

| Backend | Cobertura | Saída igual | Observação |
| --- | ---: | ---: | --- |
| Stack VM | 23/23 com `.stdout` | 23/23 | 3 exemplos de host não têm snapshot |
| RegVM | 10/23 nativos; 13/23 fallback | 23/23 | Fallback explícito preserva a saída |
| JavaScript | 23/23 | 23/23 | O build grava `package.json` com `"type": "module"` |
| WebAssembly | 25/26 | — | Programas de host e closure com captura ainda não compilam; NFC agora é suportado |

## Outros subsistemas

| Área | Evidência | Limite |
| --- | --- | --- |
| C ABI | Testes Rust + harness C nativo + `header_sync` (36 exports) | Validar consumidor externo em cada plataforma |
| Pacotes | Testes de resolver, lock, cache, vendor e GitHub pinado | Sem registry público |
| Host ABI/Poppy | 14 testes Poppy verdes | Capabilities continuam deny-by-default |
| eGUI | 9 testes verdes | Adaptador headless, sem janela de SO |
| Game host | 43 testes de bridge + 3 de áudio | Executado em 2026-10-27 com `libasound2-dev` instalada no Linux |

## Reprodução

```sh
cargo test --locked
cargo test -p aipo-poppy
cargo test -p aipo-egui
target/debug/aipo plan docs/conformance/programs/01_hello.aipo
target/debug/aipo check examples/01_fizzbuzz.aipo --wasm
target/debug/aipo run docs/conformance/programs/01_hello.aipo --engine=reg
```

O plano Reg só seleciona o backend nativo quando o emissor e o verifier aceitam o IR e a
construção não depende de serviços canônicos ausentes. Quando há fallback, `aipo plan`
mostra o motivo. Não use a existência da flag como prova de paridade.
