# Aipo Cross-Language Benchmarks

**Status:** implemented; manual performance evidence, not a blocking gate
**Scope:** portable workloads for Aipo CLI/VM, Aipo VM in-process, Aipo→JavaScript/Node, Lua, LuaJIT, Wren, Luau, CPython, PyPy, Ruby, JavaScript/Node and Rust native
**Related:** `docs/performance/baseline.md`, `docs/testing/ci-tiers.md`, `crates/aipo-bench/`

## Objetivo

Comparar custo de workloads equivalentes sem esconder a diferença entre interpretador, JIT, VM e código nativo. Rust nativo é um controle algorítmico; seu processo usa o mesmo binário do runner e inclui o startup desse harness, portanto não representa o startup de um binário nativo mínimo.

Aipo aparece em quatro linhas:

- **Aipo Wasm JIT:** compilação direta de HIR para WebAssembly e execução via Cranelift/Wasmtime com Host ABI `aipo_host` (`aipo run --wasm`).
- **Aipo VM in-process:** compilação fora do timer; execução, criação da VM e registro da stdlib dentro do timer.
- **Aipo CLI/VM:** processo `aipo run`, incluindo startup e compilação do fonte na Stack VM clássica.
- **Aipo→JavaScript/Node:** bundle emitido uma vez antes do timer; execução do Node é medida separadamente.

## Como executar

Prepare os binários release:

```bash
cargo build --release -p aipo-cli -p aipo-bench
```

Execute a comparação completa:

```bash
target/release/aipo-bench --compare \
  --compare-json target/cross-language.json
```

Modos úteis:

```bash
# três amostras, para desenvolvimento
target/release/aipo-bench --compare --compare-quick

# workload e linguagens específicas; N é explícito por workload
target/release/aipo-bench --compare --compare-n arithmetic=100000,collections=50000 \
  --compare-languages aipo-vm,lua,python,rust

# workload específico para A/B isolado
target/release/aipo-bench --compare --compare-workloads collections \
  --compare-languages aipo-vm --compare-runs 15

# caminho explícito do binário Aipo
target/release/aipo-bench --compare --aipo-bin target/release/aipo

# primeira onda: Wren e Luau locais; PyPy é opcional
PATH="$Wren_BIN_DIR:$LUAU_BIN_DIR:$PATH" \
  target/release/aipo-bench --compare \
  --compare-languages wren,luau,pypy \
  --compare-workloads arithmetic,collections,strings,recursion,startup \
  --compare-runs 1 --compare-n arithmetic=32,collections=32,strings=8 \
  --compare-resources
```

Quando `--aipo-bin` ou `AIPO_BIN` é usado, o runner registra o hash e o profile inferido do caminho; um binário customizado permanece sob responsabilidade de quem o selecionou.

### Runner pareado dedicado

Para medições locais de A/B, use dois binários release já construídos e o runner `scripts/perf/paired.sh`. Ele fixa a CPU, alterna a ordem baseline/candidate e não altera o estado do Git:

```bash
scripts/perf/paired.sh \
  --baseline target/release/aipo-bench-before \
  --candidate target/release/aipo-bench-after \
  --workloads arithmetic,collections,fields,recursion \
  --rounds 15 --pairs 3 --cpu 0 \
  --output-dir target/paired
```

O script exige `taskset` por padrão; `--allow-unpinned` só deve ser usado em machines sem suporte a taskset. Os relatórios e um `manifest.txt` com hashes, workloads, rounds e CPU ficam em `target/paired`.

O runner detecta `lua`, `luajit`, `wren_cli`/`wren`, `luau`, `python3`, `pypy3`/`pypy`, `ruby` e `node`. Runtimes externos ausentes são marcados como skipped. Aipo CLI/VM, Aipo→JavaScript/Node e Rust nativo são required; sem o binário Aipo ou sem Node, a falha aparece no relatório e o comando retorna erro.

## Workloads

Todos os programas imprimem exatamente `checksum:<valor>`. O runner rejeita saída ausente, divergente ou com exit code diferente de zero.

| Workload | N padrão | O que mede |
|---|---:|---|
| `arithmetic` | 200000 | loop com chamada de função e aritmética |
| `collections` | 100000 | construção e iteração de lista |
| `fields` | 100000 | seis campos, mutação e chamadas de método |
| `strings` | 5000 | concatenação ASCII incremental |
| `recursion` | fixo | Fibonacci(24) |
| `startup` | fixo | processo mínimo, runtime initialization e cache possivelmente aquecido |

`--compare-quick` remove `startup` e usa três amostras. A execução normal usa cinco amostras. `--compare-n` usa pares `workload=N`, com N entre 1 e 1.000.000; isso evita aplicar N de aritmética ao workload quadrático de strings. `--compare-workloads` seleciona uma lista de workloads para A/B isolado. `--compare-runs` aceita de 1 a 31.

## Primeira onda de referências

A matriz adiciona três referências sem transformar grupos diferentes em uma comparação única:

- **Wren 0.4.0 + Wren CLI 0.4.0**: VM compacta, bytecode e embedding; o CLI recebe o script e os argumentos diretamente.
- **Luau 0.739**: VM derivada de Lua, com interpretador e código nativo; o CLI usa `-a` para separar os argumentos do programa.
- **PyPy 8.0.0, Python 3.12**: mesma linguagem Python com runtime JIT/GC; ausente no PATH, aparece como `skipped`.

As fontes são mantidas em `/home/raillen/Documentos/Projetos/aipo-reference-runtime/` e registradas em `SOURCES.json`. Elas são referências read-only, não dependências do build da Aipo. O runner resolve o executável pelo `PATH`; o checkout pinado não prova, sozinho, qual binário foi executado. Por isso, execuções locais devem registrar `PATH`, hash do binário e versão reportada.

Wren não fornece um CLI oficial no checkout principal da VM. O CLI separado é a fonte do executável usado para esta bateria. O build do CLI 0.4.0 encontrou uma colisão de nome `write` com headers libc atuais; a execução local usou uma árvore temporária com renomeação de compatibilidade, sem modificar o checkout pinado. Luau não aceita `--version`; o runner usa `-h` para confirmar disponibilidade e registra `luau CLI (no --version flag)`.

Mapa inicial de leitura:

| Aipo | Referências |
|---|---|
| `crates/aipo-vm/src/vm/dispatch.rs` | `lua/lvm.c`, `cpython/Python/ceval.c`, `ruby/vm_exec.c`, `node/deps/v8/src/interpreter/interpreter.cc` |
| `crates/aipo-vm/src/vm/call.rs` | `ruby/vm_method.c`, `luau/VM/src/lvmexecute.cpp`, `wren/src/vm/wren_vm.c` |
| `crates/aipo-vm/src/value.rs` | `lua/lobject.c`, `wren/src/vm/wren_value.c`, `luau/VM/src/lobject.cpp` |
| `crates/aipo-bytecode/src/emitter.rs` | `luau/Compiler/src/Compiler.cpp`, `wren/src/vm/wren_compiler.c`, `cpython/Compile` |

A leitura serve para formar hipóteses. Nenhum código de referência deve ser copiado sem verificar licença, contrato de runtime e paridade de semântica.

A contagem de alocações ficou explicitamente fora do primeiro resultado porque a Aipo ainda não possui um contrato único de allocator/GC contabilizável; `docs/adp/ADP-003-execution-budgets.md` permanece draft. O próximo spike deve definir escopo, unidade e custo antes de escolher `Rc`, arena ou tracing GC.

Ordem recomendada para os próximos experimentos de fields: (1) cache monomórfico de slot por site, já mantido; (2) cache do frame base, já mantido; (3) cópia de entrada de `SetField` em structs unguarded, já mantida; (4) alocações de `BoundMethod` ainda pendentes, mas sem novo pool de nomes até haver runner dedicado; (5) opcodes `Call0..Call4`/`GetLocal8`. Layout denso de structs, metadata de método cacheada, pool de nomes e despacho de método sem alocar foram testados e revertidos por ausência de ganho demonstrado. Cada etapa precisa de A/B próprio; não introduzir JIT, NaN-boxing ou GC custom antes de evidência.

### Piso de ruído do runner

Este host entrega um piso de ruído de aproximadamente ±5% nos workloads de `fields` e `arithmetic` em condições normais, e uma dispersão muito pior quando há contenção: o mesmo binário já chegou a variar 2,5x entre execuções da mesma configuração, com load average de 7,3 em 4 cores. Consequências práticas:

- Diferença menor que ±5% não é evidência de nada, mesmo com checksums idênticos e MAD pequeno por execução.
- Um único lote pareado não decide uma mudança. Mudanças candidatas precisam de pelo menos dois lotes, idealmente com `arithmetic` como workload de controle, porque ele não toca o caminho alterado e portanto mede deriva do ambiente.
- `arithmetic` não usa structs nem despacho de método, então serve como controle de deriva para experimentos de fields.
- Movimentar campos em structs grandes altera offsets e alinhamento de cache o suficiente para virar de +7% a neutro sem qualquer mudança semântica. Posição de campo é parte do resultado experimental e deve ser reportada junto do delta.
- Um lote com MAD pequeno ainda pode estar contaminado pela deriva acumulada ao longo da sessão; comparar o candidato com o baseline medido no mesmo lote continua obrigatório.
- Quando o host está disputado, wall-clock não decide nada. `scripts/perf/cpu_ab.py` mede tempo de CPU do processo filho (`RUSAGE_CHILDREN`) em vez de wall-clock: sob contenção o wall-clock infla por um fator arbitrário, enquanto o tempo de CPU continua proporcional ao trabalho efetivamente feito, porque tempo removido pelo escalonador não é cobrado. Ainda assim, com dispersão de 2,5x dentro do mesmo binário, o número mínimo de muitas repetições continua sendo o estimador mais confiável, e a conclusão correta é "inconclusivo" quando os intervalos se sobrepõem.

## O que o relatório contém

O relatório JSON usa o schema 5; a entrada identifica `manifest_schema` separadamente.

Cada resultado inclui:

- sample brute de cada execução;
- median e MAD;
- mínimo, p95 e máximo;
- checksum esperado e observado;
- operações e operações por segundo;
- versão do runtime;
- `N`, modo, setup de preparação e setup de runtime quando aplicável;
- métricas internas de instruções, calls, fields, cache de fields, globals e constantes para Aipo VM;
- para Aipo VM, timing sem instrumentação e uma execução separada, não cronometrada, para coletar métricas;
- `resources` com `peak_rss_bytes`, quando disponível, em bytes, obtido por amostragem Linux de `/proc` somente com `--compare-resources`; `allocation_count` e `allocated_bytes` permanecem nulos até existir um adapter específico por runtime;
- `field_cache_hits` e `field_cache_misses` contam lookups de slot em `GetField` e `SetField`; por isso os hits podem ser maiores que `field_lookups`, que registra apenas leituras;
- ambiente, perfil do build, dirty state do Git, CPU/OS e número de CPUs disponíveis;
- hash do binário do runner e hash/profile do binário Aipo quando selecionado.

Exemplo de saída:

```text
Aipo CLI/VM  287.11ms MAD=2.28ms p95=289.39ms checksum=59999500000
Lua           4.33ms MAD=233.35us p95=4.56ms checksum=59999500000
Rust native   1.49ms MAD=32.69us p95=1.79ms checksum=59999500000
```

## Política de fairness

- O mesmo algoritmo, as mesmas operações lógicas e o mesmo `N` são usados em todas as linguagens; o Rust nativo também constrói e percorre a lista e a string.
- O workload `fields` preserva a mesma recurrence e o mesmo checksum, mas usa structs, classes, tables ou `__slots__` conforme o modelo de cada runtime; compare-o como hotspot de lookup, não como ranking de object layout.
- O resultado é verificado por checksum; isso reduz a chance de trabalho morto, mas não é uma prova independente de que cada operação foi executada.
- Uma warmup não entra nas samples.
- `process` e `in-process` não devem ser comparados como a mesma métrica.
- Startup de interpretador permanece incluído nos resultados de processo.
- Compilação/bundle Aipo→JS é reportada como setup, não misturada com execução.
- Aipo VM in-process não inclui startup de processo e é sempre rotulado separadamente.
- Rust nativo é compilado fora do timer e não representa a mesma classe de runtime; a linha de startup inclui o harness `aipo-bench` e serve apenas como controle do runner.
- O p95 de uma rodada curta pode ser igual ao máximo; use mais samples em um runner dedicado antes de interpretar esse número.

## Limitações

- O resultado depende de CPU, sistema, versionamento e estado térmico.
- Python/PyPy, Ruby, Lua/LuaJIT, Wren, Luau e Node possuem Garbage Collectors, dispatch, recompilação e otimizações diferentes. O modo de execução deve ser lido junto com `kind`, `mode`, `includes_startup` e `includes_compile`.
- Os benchmarks registram `peak_rss_bytes` por amostragem Linux em uma execução separada quando `--compare-resources` está ativo; a amostragem pode perder o pico de processos muito curtos. `allocation_count` e `allocated_bytes` ainda não são comparáveis entre runtimes e permanecem nulos quando não há adapter nativo. I/O, startup de editor e engine integration continuam fora do escopo.
- `N` grande reduz o efeito de startup, mas pode medir regimes de memória diferentes.
- Os fixtures são versionados e confiáveis; o runner não é um sandbox para código arbitrário. Processos externos têm deadline de 120 s e output limitado a 1 MiB; a captura da VM também é limitada a 1 MiB, mas a VM in-process não possui budget de instruções para código arbitrário.
- Uma execução local não é baseline para regressão. Use um runner dedicado, pin de CPU e histórico de pelo menos três relatórios.

## Resultado local final

A tabela histórica abaixo cobre os cinco workloads originais; o workload `fields` é reportado separadamente na primeira onda.

Uma rodada completa de cinco samples em Linux x86_64, Intel Core i7-3632QM, execução sequencial, 4 CPUs disponíveis, Rust 1.98.1, Node 24.18.0, Python 3.14.6, Ruby 4.0.6, Lua 5.5.1 e LuaJIT 2.1 produziu estes median:

| Workload | Aipo CLI/VM | Aipo VM in-process | Aipo→JS | Lua | LuaJIT | Python | Ruby | JS | Rust |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `arithmetic` | 251.04ms | 239.12ms | 572.05ms | 10.68ms | 5.49ms | 57.33ms | 99.18ms | 57.02ms | 5.51ms |
| `collections` | 194.68ms | 180.20ms | 1118.91ms | 10.62ms | 5.50ms | 41.67ms | 94.90ms | 62.60ms | 5.64ms |
| `strings` | 10.62ms | 5.45ms | 109.41ms | 10.57ms | 10.65ms | 25.87ms | 87.60ms | 52.13ms | 5.50ms |
| `recursion` | 85.09ms | 79.00ms | 269.51ms | 10.62ms | 5.57ms | 31.07ms | 92.09ms | 50.32ms | 5.51ms |
| `startup` | 5.50ms | 0.29ms | 63.83ms | 5.53ms | 5.52ms | 21.05ms | 85.65ms | 48.40ms | 5.55ms |

A última alteração de implementação remove `Vec` temporárias de natives e conversões; a medição subsequente em `target/cross-language-final-current.json` foi feita sob carga elevada do runner e por isso não substitui a tabela acima como baseline comparável.

## Resultado local da primeira onda

Rodada release com cinco samples, execução sequencial, Wren 0.4.0 e Luau 0.739. Os números são median de processo e incluem startup/compilação; não são baseline de regressão.

| Workload | Wren | Luau |
|---|---:|---:|
| `arithmetic` | 41.37ms | 22.27ms |
| `collections` | 37.38ms | 11.61ms |
| `fields` | 120.35ms | 63.15ms |
| `strings` | 84.00ms | 21.44ms |
| `recursion` | 22.36ms | 16.07ms |
| `startup` | 6.55ms | 7.25ms |

No workload `fields`, a mesma rodada mediu Aipo VM em 1.430,92ms, contra 120,35ms do Wren e 63,15ms do Luau. Essa diferença é um **alerta para hotspot de fields/métodos**, não um veredito universal: os três runtimes têm modelos de objeto diferentes. O relatório detalhado ficou em `target/fields-release.json`.

PyPy não estava disponível no `PATH` e foi registrado como `skipped`. Uma rodada completa com todos os runtimes disponíveis produziu 66 resultados, 6 skipped e 0 failures. O relatório da primeira onda ficou em `target/first-wave-release.json`; a matriz completa ficou em `target/cross-language-first-wave-full.json`. Ambos contêm samples, RSS quando solicitado e proveniência.

## A/B do cache monomórfico de fields

Experimento de 15 samples, Aipo VM in-process, `N=100000`, mesmo workload e mesmo checksum:

| Variante | Median | MAD |
|---|---:|---:|
| Antes | 1.001,77ms | 39,22ms |
| Cache por site | 718,20ms | 30,54ms |

Redução median observada: **28,31%**. O cache produziu 2.399.992 hits e 8 misses no metrics pass. Relatórios: `target/fields-cache-before.json` e `target/fields-cache-after.json`. Esse resultado é direcional, não gate, porque o runner é compartilhado.

## Experimento rejeitado: cache de metadata de métodos

Um cache por site/tipo para `GetField` de métodos foi implementado temporariamente e medido contra a mesma workload `fields`. Ele registrou 199.998 hits e 2 misses, mas não apresentou ganho reproduzível no runner compartilhado:

| Variante | Median (31 samples, CPU 0) |
|---|---:|
| Sem cache de métodos | 971,58ms |
| Com cache de métodos | 1.131,79ms |

Uma rodada sequencial menor também ficou pior (1.141,64ms sem cache contra 1.599,96ms com cache). Como os valores foram afetados por carga elevada, a conclusão conservadora é ausência de benefício demonstrado, não regressão universal. O protótipo foi revertido; o cache monomórfico de fields e o cache do frame base permanecem como otimizações mantidas. Relatórios: `target/fields-method-disabled-pinned.json` e `target/fields-method-enabled-paired.json`.

## Experimento rejeitado: pool de nomes de métodos

Um protótipo trocou o nome de `BoundMethodData` para `Rc<str>` e manteve nomes qualificados em slots por site. A intenção era eliminar `format!` e cópias de nomes, mas o A/B não mostrou ganho reproduzível: em uma rodada adjacenta de 31 samples, `fields` ficou em 602,49ms contra 605,07ms do baseline, enquanto `collections` piorou de 178,82ms para 192,71ms. Uma segunda variante com `Owned`/`Shared` também não resolveu: `fields` 635,72ms contra 630,59ms e `collections` 190,71ms contra 179,45ms. A carga do runner variou entre execuções, então esses números são apenas uma rejeição exploratória, não uma regressão universal. O pool e a mudança de layout de `BoundMethodData` foram revertidos; a próxima tentativa deve evitar API-breaking sem um A/B em runner dedicado. Relatórios: `target/qualified-name-candidate-31.json`, `target/qualified-name-baseline-31b.json` e `target/bound-name-enum-candidate-31.json`.

## Experimento rejeitado: cache de flags `fixed` por slot

O `SetField` registrado passou a carregar um `Vec<bool>` de flags `fixed` por tipo e usar um setter com a flag já resolvida, evitando `HashSet<String>::contains` por escrita. O A/B pareado (3 pares, 15 samples) piorou o median dos três pares: `arithmetic` +5,34%, `fields` +11,32% e `recursion` +144,36%. Checksums e métricas continuaram idênticos, mas o custo do lookup adicional superou a economia; a alteração foi revertida. Relatórios: `target/fixed-field-paired/`.

## Experimento rejeitado: entry de cache com guardedness

O cache por site foi estendido para guardar também `type_is_guarded`, eliminando as duas buscas de HashMap no `SetField`. O A/B pareado (3 pares, 15 samples) não mostrou ganho: `arithmetic` +6,32%, `fields` +1,40% e `recursion` +2,13% no median dos três pares. Checksums e métricas permaneceram idênticos, mas a extensão foi revertida. Relatórios: `target/field-entry-paired/`.

## Experimento rejeitado: layout denso de `StructInstance`

`StructInstance` deixou de guardar `Vec<(String, Value)>` e passou a guardar `field_names: Vec<String>` ao lado de `fields: Vec<Value>`, eliminando o tuple por slot. A intenção era tornar o acesso a campo um índice direto em `Vec<Value>` e reduzir o tamanho do valor armazenado.

O A/B pareado (3 pares, 15 samples, CPU 0) mostrou o candidato mais lento em `fields` nos três pares: +3,73%, +7,39% e +24,23%. No median dos três pares, `arithmetic` ficou +33,87%, `fields` +22,66% e `recursion` -57,76%. A carga do runner foi elevada durante a medição (load average 2,50 a 3,80 em 4 cores, com o agente de código consumindo ~86% de CPU), o que explica a dispersão em `arithmetic` e `recursion`; ainda assim, a direção em `fields` foi consistente nos três pares e é a evidência relevante, já que essa é a workload que exercita o caminho alterado. Checksums permaneceram idênticos em todos os pares.

A causa provável é estrutural: separar nomes e valores troca uma alocação por instância de struct por duas, e cada acesso a campo passa a tocar duas regiões de memória em vez de uma. Como o fixture `fields` constroi instâncias repetidamente, a alocação extra domina o ganho esperado do acesso indexado.

O experimento também era API-breaking: `fields` é público e os oito arquivos consumidores (`value.rs`, `vm/dispatch.rs`, `host.rs` e cinco módulos da stdlib) tiveram de mudar. Como não houve ganho, o protótipo foi revertido e a API pública permanece com `Vec<(String, Value)>`. Relatórios: `target/dense-layout-paired/`.

## Experimento rejeitado: despacho de método sem alocar

A inspeção do caminho quente encontrou um custo real que os benchmarks não isolatingam. No fixture `fields` há 200.000 binds de método (`advance` e `score`), e cada um fazia quatro alocações de `String`: um clone do `type_name` do receiver, dois clones para montar a chave do mapa e um `format!("{type_name}.{field_name}")`. O padrão correto já existia no próprio repositório: `method_natives` tem um gêmeo aninhado (`method_natives_by_type`) que faz lookup com `&str` sem alocar, mas `struct_methods`, o mapa de métodos de usuário e o caminho quente do `fields`, nunca recebeu esse tratamento.

O protótipo aplicou o mesmo padrão: `struct_methods_by_type` aninhado por tipo, nome qualificado formatado uma vez no registro em vez de a cada bind, e `lookup_struct_method(&str, &str)` reaproveitando `lookup_method_native`. O nome da instância só era clonado no caminho de erro. Semanticamente a mudança era neutra: 56 suítes passaram, incluindo o differential VM↔JS, e os checksums permaneceram idênticos.

Mesmo assim, o ganho não pôde ser demonstrado. Foram quatro lotes pareados, mudando apenas a posição do novo campo na struct `Vm`:

| Lote | Posição do novo campo | Δ `fields` | Δ `arithmetic` |
|---|---|---:|---:|
| 1 (5 pares) | inline, no meio | -5,57% | +1,72% |
| 2 (3 pares) | inline, no meio | -3,70% | +1,13% |
| 3 (3 pares) | no fim da struct | +1,05% | -0,02% |
| 4 (5 pares) | boxed, no meio | +4,54% | +6,80% |

A mesma mudança lógica, com o mesmo mecanismo, oscilou de -5,6% a +4,5% apenas por mover um campo. Isso indica que o piso de ruído do runner é de aproximadamente ±5% neste host e que diferenças dentro dessa faixa não são evidência. O primeiro lote, que sugeria um ganho convincente de -5,57% com 4 de 4 pares limpos favoring o candidato, foi em boa parte sorte de um lote silencioso; lotes posteriores com ruído controlado o refutaram. O `arithmetic` funciona como workload de controle, pois não tem structs nem despacho de método, e por isso mede deriva do ambiente em vez do efeito da mudança.

Pelo exposto, o protótipo foi revertido. Além da ausência de ganho demonstrado, a mudança exigia duplicar o estado de registro: `struct_methods` é `pub`, então qualquer escrita futura fora de `register_struct_method` desincronizaria o mapa sombra e quebraria o despacho de método silenciosamente, sem que haja ganho de velocidade que pague esse risco.

## Diagnóstico: custo constante por opcode e a taxa do `VmFault` de 72 bytes

Medindo o custo por opcode em três programas de calibração com o mesmo número de iterações, e usando a contagem determinística de instruções como denominador, o custo por opcode é praticamente constante e independente da operação executada:

| Programa | Ops/iter | ns por opcode |
|---|---:|---:|
| `i = i + 1` (locais) | ~9 | ~111 |
| `s = s + i; i = i + 1` | ~14 | ~98 |
| `arithmetic` (chamadas e globals) | 28 | ~100 |

Um custo constante de cerca de 100ns por opcode, que não muda com a mistura de operações, indica que o custo está no laço de despacho e não em nenhum handler. Ablação descartou a contabilidade de métricas como causa: desligar `metrics_enabled` alterou o resultado em apenas 1% a 2%.

A causa identificada está no topo de `Vm::step`, que executa para **todo** opcode:

```rust
// antes
let opcode = OpCode::try_from(opcode_byte).map_err(|b| VmFault::CorruptedBytecode {
    offset: self.ip - 1,
    reason: format!("unknown opcode 0x{b:02x}"),
})?;
```

`OpCode::try_from` devolve `Result<OpCode, u8>`, ou seja 2 bytes, mas o `.map_err` reconstrói o resultado como `Result<OpCode, VmFault>`. `VmFault` tem 72 bytes, porque várias variantes carregam `String` alocado inline. Portanto cada opcode executado construía e destruía um `Result` de 72 bytes, e o closure que faz a conversão capturava `self`, o que obriga o compilador a manter `ip` em memória. Some-se a isso que todo `Result<_, VmFault>` de `push`, `pop` e `read_u16` também tem 72 bytes, o mesmo custo aparece várias vezes por opcode.

A correção troca o `.map_err` por um `match` que só constrói a falha no ramo frio, mantendo offset e mensagem idênticos:

```rust
let opcode = match OpCode::try_from(opcode_byte) {
    Ok(opcode) => opcode,
    Err(byte) => return Err(VmFault::CorruptedBytecode { /* idêntico ao anterior */ }.into()),
};
```

O caminho feliz passa a ser um `match` sobre 2 bytes, sem `Result` largo, sem closure que captura `self` e sem `format!`. O teste `test_unknown_opcode_reports_the_offset_of_the_bad_byte` fixa o erro do ramo frio, e as 56 suítes do workspace passam com contagem de instruções e checksums idênticos.

**O resultado da medição é nulo.** Com tempo de CPU do processo filho, 24 repetições alternadas por workload `arithmetic`, o resultado foi:

| | min | median |
|---|---:|---:|
| antes | 3,019s | 5,474s |
| depois | 3,014s | 5,359s |
| delta | -0,16% | -2,09% |

O número que decide é o mínimo, porque é o estado sem disputa: se a mudança removesse trabalho real por opcode, o mínimo cairia de forma mensurável. Não caiu. A explicação provável é que o LLVM já afundava a construção da falha para fora do caminho feliz, mantendo apenas o `match` de 2 bytes em tempo de execução, de modo que o desperdício existia no código-fonte mas nunca chegou a ser executado. As duas distribuições se sobrepõem quase por completo (antes 3,02s–7,53s, depois 3,01s–8,52s).

A alteração foi mantida, não por ganho de velocidade, que não existe, mas por dois motivos que não dependem de benchmark: ela deixa a construção da falha no ramo frio explícita no código, em vez de depender de o otimizador mover o caminho de erro para fora, e o teste novo fixa o contrato de `unknown opcode`, que antes não tinha cobertura. Registrar isso como otimização de desempenho seria errado; é uma mudança de robustez e clareza com resultado medido nulo.

A mesma hipótese do compilador provavelmente se aplica a `push`, `pop` e `read_u16`: o tamanho de `VmFault` é um problema real de hygiene e de ergonomia, e vale a pena reduzir por API, mas é plausível que o caminho de erro frio já seja removido em tempo de execução. Portanto **encolher `VmFault` não deve ser tratado como otimização de desempenho sem medir**. O comando para medir em host ocioso é:

```sh
python3 scripts/perf/cpu_ab.py <binario-baseline> <binario-candidato> --workloads arithmetic --rounds 10 --reps 24
```

## A/B do cache do frame base

A VM mantém em `Vm::frame_base` o `stack_base` do frame ativo para que `GetLocal`, `SetLocal` e `JumpIfSetLocal` não consultem `frames.last()` a cada acesso. O valor é atualizado em `run`, push/pop de frames, handlers de falha, `invoke` e troca de tasks. Um teste de regressão cobre retorno de chamada aninhada e restauração do frame externo.

A/B pareado, Aipo VM in-process, CPU 0, 31 samples, mesma workload e checksums:

| Workload | Sem cache no caminho quente | Com `frame_base` | Delta |
|---|---:|---:|---:|
| `arithmetic` | 276,23ms | 265,98ms | -3,71% |
| `fields` | 736,02ms | 687,05ms | -6,65% |
| `recursion` | 97,72ms | 93,65ms | -4,17% |

Métricas internas e checksums permaneceram bit a bit idênticos. Relatórios: `target/frame-base-baseline-31.json` e `target/frame-base-candidate-31.json`. O resultado é direcional, não gate, porque o runner é compartilhado. Uma tentativa adicional de forçar `#[inline]` em helpers quentes não trouxe ganho reproduzível e foi revertida.

## Redução de cópias em `SetField`

O valor de entrada de uma atribuição só é necessário para o journal quando o tipo é guarded. O caminho agora verifica `type_is_guarded` antes de clonar o valor anterior; structs sem `invariant()` mantêm a mesma mutação, sem a cópia de entrada. No fixture `fields`, isso elimina 600.000 clones de `Value` (seis escritas × 100.000 iterações) de forma determinística. Um A/B posterior com 31 samples não mostrou ganho consistente de wall-clock (`fields`: 608,31ms no baseline contra 612,08ms com a mudança; `collections`: 184,11ms contra 178,36ms). Portanto, a alteração é mantida como redução determinística de trabalho, não como claim de velocidade. Os testes de rollback de structs guarded e a suíte differential continuam verdes.

## Experimento rejeitado: cache de tipos guarded

Um `HashSet<String>` foi criado para substituir as duas buscas em `type_is_guarded`. O runner pareado (3 pares, 15 samples por execução) mostrou o candidato mais lento no median dos três pares: `arithmetic` +6,58%, `fields` +1,63% e `recursion` +3,36%. Checksums e métricas permaneceram idênticos, mas não houve benefício; o cache foi revertido. Relatórios: `target/guarded-cache-paired/`.


## Resultado A/B isolado do cache de constantes do shim

Em uma execução sequencial de três amostras apenas para `Aipo→JavaScript/Node`, o cache preguiçoso de constantes por instrução reduziu o median em aproximadamente `26%` (`arithmetic`), `11%` (`collections`) e `17%` (`recursion`). Os relatórios estão em `target/js-constant-cache-base.json` e `target/js-constant-cache-after.json`; o resultado é direcional porque o runner é compartilhado.

Observações: Aipo CLI/VM e Aipo VM in-process ficaram atrás das linguagens comparadas nos workloads computacionais; Aipo→JavaScript continua sendo o caminho mais caro, enquanto o caso de strings evidencia o custo de concatenação incremental. O startup da VM in-process é medido sem startup de processo. Rust nativo executa as mesmas operações lógicas, mas permanece apenas como controle de limite inferior.

Na comparação local A/B de três amostras contra `target/perf-phase1.json`, o Aipo VM reduziu o median em `12,3%` (`arithmetic`), `10,1%` (`collections`), `6,0%` (`strings`) e `6,2%` (`recursion`); Aipo→JavaScript variou de `-10,4%` a `+2,8%`. Esse resultado é direcional, não um gate, porque o runner é compartilhado.

## Substrato WebAssembly JIT (ADP-013 / Marco 6)

Com a introdução da crate `aipo-wasm` e a integração nativa ao CLI (`aipo run --wasm`), os programas Aipo são compilados diretamente do HIR para bytecode WebAssembly e executados via JIT machine code nativo pelo Cranelift/Wasmtime. 

O runtime WebAssembly elimina a sobrecarga de despacho da Stack VM tradicional (`match` de opcodes, `Vm::step`, `Value` medido em 16 bytes neste alvo com boxing e verificação de limites dinâmicos de pilha), operando diretamente com primitivas Wasm (`i64`, `f64`, `i32`) e layout linear de memória para structs.

### Bateria comparativa (`aipo-bench --compare`)

Execução de 7 rounds comparando o novo runtime WebAssembly JIT (`aipo-wasm`) com a Stack VM anterior (`aipo` e `aipo-vm`) e runtimes de mercado. Evidência registrada em `target/bench_wasm_comparison.json`.

#### 1. Aritmética (`arithmetic`, N=200.000, checksum: `59999500000`)

Laço intensivo de chamadas de função aritméticas (`step(i) = i * 3 - 1`).

| Runtime | Tipo de Execução | Mediana (ms) | Mínimo (ms) | Speedup vs VM | Razão vs Wasm |
|---|---|---:|---:|---:|---:|
| **Rust native** | Nativo compilado (`rustc -O`) | 5,51ms | 5,46ms | 48,5x | 0,35x |
| **LuaJIT** | JIT tracing nativo | 5,57ms | 5,43ms | 48,0x | 0,35x |
| **Lua 5.4** | Bytecode VM em C | 10,77ms | 10,58ms | 24,8x | 0,68x |
| **Aipo Wasm JIT** | **WebAssembly JIT (Cranelift)** | **15,72ms** | **10,53ms** | **17,0x – 25,1x** | **1,00x** |
| **JavaScript (Node.js)** | V8 JIT | 63,44ms | 56,89ms | 4,2x | 4,03x |
| **Python (CPython 3.12)** | Bytecode interpretador em C | 69,59ms | 62,86ms | 3,8x | 4,43x |
| **Ruby (CRuby 3.x)** | YARV interpretador | 108,14ms | 105,87ms | 2,5x | 6,88x |
| **Aipo VM in-process** | Stack VM Rust (sem startup) | 231,64ms | 229,61ms | 1,15x | 14,73x |
| **Aipo CLI/VM** | Stack VM Rust (com startup) | 267,34ms | 244,91ms | 1,00x (base) | 17,00x |
| **Aipo → JS / Node** | Transpilação JS sobre Node | 707,36ms | 603,04ms | 0,38x | 44,99x |

**Destaques:**
- **Aipo Wasm JIT é 17x a 25x mais rápido** que o Aipo CLI na Stack VM clássica.
- **Supera Node.js por 4.0x**, **CPython por 4.4x** e **CRuby por 6.9x**.
- Empata com o interpretador C do Lua 5.4 (10,53ms vs 10,58ms no mínimo).
- Fica a apenas **1,9x** do teto absoluto de Rust compilado nativamente.

#### 2. Recursão profunda (`recursion`, Fibonacci(24), checksum: `46368`)

Chamadas recursivas puras com 92.735 invocações de função e controle de ativações de frame.

| Runtime | Tipo de Execução | Mediana (ms) | Mínimo (ms) | Speedup vs VM | Razão vs Wasm |
|---|---|---:|---:|---:|---:|
| **Rust native** | Nativo compilado (`rustc -O`) | 5,50ms | 5,45ms | 17,9x | 0,48x |
| **LuaJIT** | JIT tracing nativo | 5,85ms | 5,68ms | 16,8x | 0,52x |
| **Aipo Wasm JIT** | **WebAssembly JIT (Cranelift)** | **11,35ms** | **10,60ms** | **8,65x** | **1,00x** |
| **Lua 5.4** | Bytecode VM em C | 16,22ms | 11,12ms | 6,05x | 1,43x |
| **Python (CPython 3.12)** | Bytecode interpretador em C | 31,89ms | 30,26ms | 3,08x | 2,81x |
| **JavaScript (Node.js)** | V8 JIT | 59,66ms | 58,38ms | 1,65x | 5,26x |
| **Aipo VM in-process** | Stack VM Rust (sem startup) | 77,68ms | 77,16ms | 1,26x | 6,85x |
| **Aipo CLI/VM** | Stack VM Rust (com startup) | 98,15ms | 86,64ms | 1,00x (base) | 8,65x |
| **Ruby (CRuby 3.x)** | YARV interpretador | 98,17ms | 93,55ms | 1,00x | 8,65x |
| **Aipo → JS / Node** | Transpilação JS sobre Node | 307,74ms | 287,11ms | 0,32x | 27,12x |

**Destaques:**
- **Aipo Wasm JIT é 8.6x mais rápido** que o Aipo CLI/VM e 6.8x mais rápido que a VM in-process.
- **Supera Lua 5.4 por 1.4x**, **CPython por 2.8x**, **Node.js por 5.3x** e **CRuby por 8.6x**.
- Fica a apenas **1,9x** do Rust nativo e LuaJIT.

#### 3. Startup de Processo e Inicialização (`startup`, checksum: `ready`)

Mede o ciclo de vida completo: invocação do processo CLI, parsing, lowering, compilação JIT de máquina e emissão de stdout.

| Runtime | Mediana (ms) | Mínimo (ms) | Observações |
|---|---:|---:|---|
| **Aipo VM in-process** | 0,31ms | 0,30ms | Sem custo de novo processo do SO |
| **LuaJIT / Lua / Rust** | ~5,50ms | ~5,45ms | Processo C/Rust mínimo sem compilação pesada |
| **Aipo CLI/VM** | 5,51ms | 5,46ms | Interpretador puro (sem JIT) |
| **Aipo Wasm JIT** | **15,64ms** | **10,68ms** | **Inclui parsing + codegen Wasm + compilação nativa Cranelift** |
| **Python (CPython)** | 20,80ms | 20,64ms | Inicialização de runtime CPython |
| **Node.js** | 57,83ms | 56,37ms | Inicialização da V8 |
| **Aipo → JS** | 68,40ms | 64,29ms | Node + runtime shim Aipo |
| **Ruby** | 89,63ms | 85,44ms | Inicialização de VM Ruby |

O tempo de startup do Aipo Wasm JIT (~15ms) inclui a compilação JIT de código nativo pelo Cranelift, mantendo-se **mais ágil que CPython (21ms), Node.js (58ms) e Ruby (90ms)**.

## Revalidação local de 2026-10-27

`aipo-bench --quick --json /tmp/aipo-bench-current.json` completou no HEAD com 49 amostras de frontend, comandos, VM, JS e scaling. O workload `vm/invariant-commit` usava `impl`/`init`/`invariant()` antigos e foi atualizado para `Tipo:metodo`/`init`/`invariant`; sem isso o runner abortava no parser atual. Os ratios de scaling foram classificados como `noisy` pelo próprio runner; trate os números como fumaça de performance, não como baseline certificada.

## Perfil Reg: microbenchmark não é ganho de usuário

O benchmark pareado de P07-G02 mede chamadas Reg contra uma baseline fixa (17,361 ms → 3,616 ms em Int; 20,129 ms → 4,426 ms em String). Esse número **não** é um ganho fim-a-fim do CLI: em 2026-10-27, `aipo plan` selecionou Reg nativamente em 20/36 programas de conformance e usou a VM canônica nos demais. Programs com `filter`/`map`/`reduce`, `attempt`/`or_else` ou IR não representável continuam na VM. Meça o workload real com `aipo plan` antes de citar a razão.

Esses números são evidência local, não gate nem promessa de performance. O relatório completo, com samples crus, profile release, dirty state e checksums, ficou em `target/cross-language-final.json` e `target/bench_wasm_comparison.json`, fora do versionamento. Anexe-o como evidence quando uma decisão de produto depender dos dados.
