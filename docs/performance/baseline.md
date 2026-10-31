# Baseline de Performance do Aipo

**Status:** registro (artefato gerado, não é um gate)
**Escopo:** baselines de wall-clock para estágios do frontend, comandos, workloads da VM, o backend JS e a suíte cross-language separada
**Ambiente:** Linux x86_64 (4 CPU, 5 GiB RAM), rustc 1.98.1, Node v24.18.0, `--release`, runner compartilhado (ruidoso — veja a coluna MAD)
**Registrado em:** 2026-09-20, `cargo run --release -p aipo-bench` (7 amostras, mediana/MAD), git a8709881
**Re-medido em:** 2026-10-31, mesma máquina e método, git ad7aaaa — ver "Rodada de 2026-10-31" abaixo

Estes números são baselines para comparação futura em um runner dedicado, **não** limiares de aprovação/reprovação. Para o protocolo multi-runtime, as amostras brutas, os checksums e as regras de justiça, veja `docs/performance/cross-language.md`.

## Estágios do frontend (mediana)

| estágio | 858 B | 9 KiB | 99 KiB | 1023 KiB |
|---|---|---|---|---|
| bytecode/compile+verify | 34.12µs | 347.30µs | 3.91ms | 45.94ms |
| formatter/format | 50.16µs | 561.38µs | 9.55ms | 78.44ms |
| hir/lower | 24.52µs | 224.03µs | 4.18ms | 41.71ms |
| ir/lower | 19.97µs | 239.95µs | 4.27ms | 39.82ms |
| lexer/tokenize | 24.13µs | 275.36µs | 5.36ms | 58.56ms |
| sema/check | 14.65µs | 105.30µs | 2.43ms | 24.32ms |
| source/new | 1.90µs | 15.45µs | 179.48µs | 2.23ms |
| syntax/parse | 77.95µs | 861.32µs | 11.42ms | 102.74ms |

## Comandos e workloads da VM (mediana)

| workload | entrada | mediana | MAD |
|---|---|---|---|
| cmd/check | 858 B | 188.04µs | 7.59µs |
| cmd/run | 858 B | 349.31µs | 6.43µs |
| cmd/build-emit | 858 B | 483.31µs | 4.75µs |
| cmd/fmt-check | 858 B | 94.61µs | 2.92µs |
| vm/int-arith | 43 B | 217.06µs | 1.62µs |
| vm/float-arith | 46 B | 206.48µs | 1.16µs |
| vm/call-overhead | 61 B | 223.84µs | 625ns |
| vm/recursion | 75 B | 13.79ms | 315.07µs |
| vm/closure-shared-var | 94 B | 274.68µs | 12.55µs |
| vm/global-lookup | 45 B | 218.07µs | 7.66µs |
| vm/list-build-iter | 74 B | 503.85µs | 33.07µs |
| vm/dict-insert-lookup | 78 B | 226.62µs | 2.02µs |
| vm/string-concat | 39 B | 477.75µs | 2.80µs |
| vm/interpolation | 43 B | 222.32µs | 2.16µs |
| vm/bytes-alloc | 49 B | 77.50µs | 611ns |
| vm/pipeline | 40 B | 142.71µs | 389ns |
| vm/failure-propagate | 112 B | 175.88µs | 180ns |
| vm/attempt | 68 B | 106.75µs | 262ns |
| vm/contracts | 71 B | 179.25µs | 1.07µs |
| vm/invariant-commit | 167 B | 212.84µs | 1.34µs |

## Backend JS (frontend + emit + spawn do node + execução)

| workload | entrada | mediana | MAD |
|---|---|---|---|
| js/emit | 858 B | 288.64µs | 13.93µs |
| js/emit | 9 KiB | 4.00ms | 178.16µs |
| js/emit | 99 KiB | 57.60ms | 4.30ms |
| js/emit | 1023 KiB | 576.11ms | 16.64ms |
| js/hello | 17 B | 57.64ms | 5.16ms |
| js/integrated | 858 B | 72.06ms | 6.68ms |

O spawn do processo Node domina os workloads pequenos (~50–70 ms no total). A medição cross-language separa Aipo VM in-process, Aipo CLI/VM e Aipo→JavaScript; comparações de steady-state ainda exigem runner dedicado.

## Escalonamento (construção/iteração de list, inserção em dict, N=200..1600)

Todas as razões ficam próximas de 2× a cada dobra neste runner — comportamento linear; os veredictos reportam o ruído com honestidade. Razões brutas:

- `list-build: N=200..1600 ratios [1.73x, 2.17x, 1.66x] verdict=noisy — rerun on dedicated runner`
- `list-iterate: N=200..1600 ratios [1.88x, 2.07x, 1.86x] verdict=noisy — rerun on dedicated runner`
- `dict-insert: N=200..1600 ratios [1.75x, 1.87x, 2.02x] verdict=noisy — rerun on dedicated runner`

## Achado notável durante este gauntlet

O parsing de format-strings escalava 68× a cada 10× de entrada (medidos 23.4 ms para 400 f-strings em 8.4 KiB). Causa raiz: padding `" ".repeat(offset)` por placeholder mais rescans completos. Corrigido fazendo o parsing de slices sem padding e deslocando os spans de volta (`crates/aipo-syntax/src/shift.rs`, adendo do ADP-005): 1.77 ms depois, custo proporcional ao offset eliminado, snapshots do corpus idênticos byte a byte.

## Rodada de 2026-10-31

Duas correções de hot path moveram os estágios mais quentes. Mediana em 99 KiB
(7 amostras), mesma máquina ruidosa:

| estágio | antes (2026-09-20) | agora | fator |
|---|---:|---:|---:|
| lexer/tokenize | 5.36ms | 1.07ms | 5.0× |
| syntax/parse | 11.42ms | 4.12ms | 2.8× |
| js/emit | 42.9ms | 1.58ms | 27× |

O que mudou:

- **Parser** deixou de clonar o `TokenKind` inteiro para ler um payload e de novo
  para consumi-lo; identificadores e literais agora custam uma alocação, não
  duas. Sessenta e um sítios que só descartavam o token passaram a mover apenas
  o cursor. Em 1023 KiB o parse caiu de 102.74ms para 47.95ms.
- **Lexer** decodifica o code point do cursor uma vez e o mantém ao lado do
  offset; `peek` virou leitura de campo em vez de re-fatiar e re-decodificar
  UTF-8. Em 1023 KiB o tokenize caiu de 58.56ms para 11.09ms.
- **Emissor JS** escreve o JSON do módulo direto no buffer em vez de montar uma
  árvore `serde_json::Value` por instrução (48 macros `json!`, um mapa e uma
  `String` por campo). Em 1023 KiB caiu de 78.44ms para 15.41ms; a saída é
  semanticamente idêntica (a ordem das chaves segue o wire, não o `BTreeMap`).

`ir/lower` (118ms em 1023 KiB) é trabalho proporcional ao tamanho do programa,
não um comportamento quadrático: medido em 21 KiB / 43 KiB / 88 KiB o custo é
linear (0.00s / 0.01s / 0.02s em `aipo check`).

### Onde ainda há custo, e por quê

- **`syntax/parse` é o maior estágio absoluto** (47.95ms em 1 MiB, 21.9 MB/s).
  O laço agora faz trabalho real: cada nó da AST é alocado e cada expressão
  passa pelo Pratt parser. Reduzir mais exigiria arena de nós ou reutilização de
  buffers, o que é uma mudança de representação, não uma correção de hot path.
- **`vm/recursion`** (fib(20), 4.63ms) é o custo por instrução do interpretador
  de bytecode: `step` faz uma checagem de iteração, um contador de métricas e um
  `match` de opcode por instrução. O `Value` já tem 16 bytes e a chamada de
  frame não aloca; o limite restante é o dispatch interpretado em si. Trocar por
  dispatch computado ou threading é uma decisão de arquitetura da VM que a
  doutrina de performance pede para justificar com dado, não com intuição.
