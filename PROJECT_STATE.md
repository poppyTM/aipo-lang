# Current Project State

> **Reconciliação de auditoria, 2026-10-27:** o repositório foi reauditado sobre `ab5fbc0` + as correções desta reconciliação (árvore não commitada), contra o código, o corpus executável e o portal. O workspace completo passou com 1.130 testes, incluindo Poppy, eGUI e game host; a `libasound2-dev` foi instalada no ambiente de auditoria. Clippy, rustfmt e rustdoc estrito também passaram, e o build VitePress foi concluído. O gate de documentação estrita foi corrigido; o plano Reg agora seleciona nativamente 20/36 programas de conformance e usa a VM canônica com motivo explícito nos demais; o build JS grava `package.json` ESM; o catálogo ganhou `AIPO_COMPILE_WASM_UNSUPPORTED`. P07-G01 e P07-G02 continuam **DRAFT** até a certificação de todos os gates. Ver [matriz de backends](docs/architecture/backend-support-matrix.md) e [painel de progresso](website/public/progress/tasks.json).

> **Rodada P07-G02 (DRAFT), 2026-10-09:** implementação das cinco frentes publicada em `feat/runtime-and-tooling-completion`, com base P07-G01 e integração da `main` atual. Verifier Reg, revisões estruturais, planos explícitos, linker, sessão/scheduler persistentes, reload transacional guest, C cooperativo, provas/limites Wasm, ferramentas, perfis e distribuição. Testes não executados na entrega original; o PR #30 foi integrado em 2026-10-10. Não há certificação universal de paridade ou release. Veja [guia](docs/development/runtime-and-tooling-guide.md), [evidência](docs/evidence/P07-G02/README.md) e [estudos](docs/studies/runtime-and-tooling-follow-up.md). Os estados históricos abaixo não certificam esta revisão.

> **Revisão adicional P07-G01 (DRAFT), 2026-10-09:** correções de runtime e estudos reconstruídos sobre `488905ff`, integrados à `main` `bcbc4c9`, com documentação completa e publicação autorizada em draft. Testes não executados nesta reconstrução a pedido do usuário; nenhuma transição Prumo foi simulada. A fase/goal histórica abaixo é preservada. Veja [guia](docs/development/runtime-hardening-guide.md) e [evidência](docs/evidence/P07-G01-runtime-hardening.md).

- Project: **aipo**
- Prumo: **0.6.0**
- Current phase: **P07 — Runtime, embedding, shell, performance e tooling**
- Active goal: **P07-G02 — Runtime, embedding, shell, performance and tooling completion (DRAFT)**
- Last completed goal: **P06-G06 — Ferramental CLI, Host ABI & WASI (Marcos 1 a 6 concluídos)**
- Completed slices:
  - **S1 (P00-G01)**: Workspace, `aipo-source`, `aipo-diagnostics`
  - **S2 (P00-G02)**: Lexer Core (`aipo-lexer`)
  - **S3 (P00-G03)**: Parser Core (`aipo-syntax`, `aipo-ast`)
  - **S4 (P00-G04)**: HIR Lowering (`aipo-hir`)
  - **S5 (P00-G05)**: Semantic Analysis (`aipo-sema`)
  - **S6 (P00-G06)**: Core IR & Bytecode (`aipo-ir`, `aipo-bytecode`)
  - **S7 (P00-G07)**: VM Core (`aipo-vm`)
  - **S8 (P00-G08)**: Data & Errors (`aipo-vm`)
  - **S9 (P00-G09)**: Runtime & Minimum Stdlib (`aipo-runtime`, `aipo-stdlib`)
  - **Backend Completion (P00-G10)**: pipeline gaps inside S1–S9 (`aipo-syntax`, `aipo-ir`, `aipo-bytecode`, `aipo-vm`, `aipo-stdlib`, `aipo-cli`)
  - **S10 (P00-G11)**: CLI & Formatter (`aipo-cli`, `aipo-formatter`)
  - **S11 (P00-G12)**: Conformance hardening (`docs/conformance`, MVP gate)
  - **MVP contract closure (P00-G13)**: construction hooks and `Bytes` (`init`, `invariant`, `Bytes(count)`)
  - **Runtime contract enforcement (P00-G14)**: `invariant()` at stable mutable boundaries (journal + rollback, recoverable `Failure`) and signature contracts at runtime (`AssertContract` faults)
  - **Static contracts & interface conformance (P00-G15)**: pre-execution `AIPO_SEM_CONTRACT_VIOLATION_STATIC`, runtime structural conformance for interface contracts (caller-visible arity, struct named in the fault), NFC at the `String` construction boundaries, and the closed ADP-001 decisions on `clamp` bounds and tolerant slices
  - **Wave 1 exit review (P00-G16)**: module-scope bindings visible inside `fn`/`impl` bodies and canonical local `fn` declarations (self-recursion via `FillSelfCapture`, shared `var` capture), certified by `programs/19_local_functions` and `programs/20_module_scope`; exit criteria audited against `docs/waves/wave-1-mvp.md`, gauntlet 100% (13/13, 11/11, 3/3, 146/146), non-delivery explicit (`Bytes` packing, `aipo-js`, `Set`/`Sequence`, LSP/REPL) — see `docs/evidence/P00-G16-wave-1-exit-review.md`
  - **JS parity MVP (P01-G01)**: `aipo-js` emitter + versioned shim + source maps, `aipo build`, differential suite green (20/20 programs, 19/19 diagnostics, 3 module cases, 152 workspace tests) — see `docs/evidence/P01-G01-js-parity-mvp-subset.md`
  - **Deep quality gauntlet (P01-G02)**: performance baseline, fuzzing, property suites, supply-chain policy (`deny.toml`), and security boundaries — see `docs/evidence/P01-G02-deep-quality-gauntlet.md`
  - **Wave 3 tipos e valores (P02-G01)**: Set com ordem de inserção, Sequence lazy, packing de Bytes little-endian (`read_*`/`write_*`), Duration e Task/Group handles com paridade diferencial VM↔JS e conformance — ver `docs/evidence/P02-G01-wave3-types-and-values.md`
  - **Wave 3 combinadores assíncronos e scheduler (P02-G02)**: Scheduler cooperativo determinístico com tempo virtual, combinadores assíncronos (`task.spawn`, `task.sleep`, `task.all`, `task.race`, `task.timeout`, `task.cancel`, `task.group`), `await` opcode e paridade diferencial total VM↔JS — ver `docs/evidence/P02-G02-wave3-stdlib-async.md`
  - **Wave 3 infra assíncrona (P02-G03)**: `async fn` como protocolo de chamada em forma de topo, local, anônima e método de `impl` (receptor = argumento 0), `await do … end` sequencial, diagnósticos estáticos `AIPO_SEM_AWAIT_IN_SUBEXPRESSION`/`AIPO_SEM_FORGOTTEN_TASK`/`AIPO_SEM_NESTED_AWAIT_DO`, contrato estático do operando de `await`, faults de runtime `AIPO_RT_AWAIT_CYCLE`/`AIPO_RT_CANCELLED` com paridade VM↔JS, e endurecimento de literais numéricos com helper único em `aipo-lexer::number`; conformance programas 24–27 e diagnósticos 20–29 (suíte 301 testes) — ver `docs/evidence/P02-G03-wave3-async-syntax-and-diagnostics.md`
  - **Wave 4 ABI de host (P03-G01)**: crate `aipo-host` com o AHS (`HostSchema`) validado, capabilities deny-by-default (`CapabilitySet`, caminho = árvore), handles geracionais sem use-after-free (`HandleTable`), valores de host por cópia com `Int`/`Float` checados na fronteira e faults com código estável (`AIPO_RT_CAPABILITY_DENIED`, `AIPO_RT_STALE_HANDLE`, `AIPO_RT_SCOPE_ESCAPE`). Adaptador VM completo em `aipo-vm::host`, escape de escopo imposto nos 6 pontos de publicação (`SetGlobal`, `Return`, `SetField`, `SetIndex`, `BuildList`, `BuildDict`), `time.now`/`monotonic` atrás da capability `clock` (CLI instala `SystemClock`), fixtures de integração `host_scope_escape` e `host_capability_and_handles` verdes (suíte 377 testes) — ver `docs/evidence/P03-G01-host-abi.md`.
   - **Wave 4 Adaptador Poppy e demo headless (P03-G02)**: crate `aipo-poppy` com AHS (`poppy_schema`), handles geracionais `Entity`, buffer de comandos (`CommandBuffer`) com mutações estruturais deferidas a safe points, PRNG determinístico com semente (`PoppyRng`), simulação headless (`Simulation`), adaptador de VM expondo módulo `poppy` sob capabilities `poppy.*` (`poppy.ecs`, `poppy.random`) e fixture de integração (`tests/headless_demo.rs`) com 100% de determinismo entre execuções repetidas com mesma semente, variação com sementes distintas, stale handle faults e negação de capability (suíte 391 testes) — ver `docs/evidence/P03-G02-poppy-adapter-and-headless-demo.md`.
   - **Package foundation (P04-G01)**: coordinate `namespace.package`, manifest `aipo.toml`, lockfile determinístico, resolver `path`, imports qualificados, provenance, `aipo package lock/audit` e diagnósticos `AIPO_PKG_*` — ver `docs/adp/ADP-007-package-identity-and-distribution.md`.
   - **Capability service/env (P04-G02)**: `HostNative` com acesso ao `Vm` corrente, `EnvironmentSource` por VM, módulo `env.get`/`env.has` deny-by-default, metadata de capability e negação padrão no CLI — concluído e certificado.
   - **Async filesystem/env boundary (P04-G03)**: host tasks agendadas pelo VM, `FilesystemSource` por VM, `fs.read_text` async, `fs.roots` separado, path policy determinística e schema AHS — concluído e certificado.
    - **Pinned GitHub source contract (P04-G04)**: `PackageSource::GitHub` com slug, commit SHA e subpath, validação de lockfile, provenance separada do digest e `GitHubFetcher`/store offline — concluído e certificado.
      - **Public GitHub HTTP/cache (P04-G05)**: feature opt-in `github-http`, GET público por padrão com auth opcional, redirects bloqueados, timeout/limite de resposta, cache local atômico/digest-verificado e comando explícito `package fetch-github` — concluído e certificado.
      - **Explicit remote dependency graph (P04-G06)**: dependências GitHub pinadas no manifesto, resolução recursiva offline/online, lockfile completo, limite de 256 packages e fetch remainendo explícito — concluído e certificado.
      - **Cache-aware offline package consumption (P04-G07)**: cache local disponível no build padrão, leituras sem criação de diretórios, paths de entry verificados, `--package-cache` em check/run/build/disasm e lock stale fail-closed — concluído e certificado.
      - **Mixed local and remote package roots (P04-G08)**: root local com branches `path` e edges GitHub pinadas, `package lock --fetch-github --cache` explícito, lock determinístico único e replay offline pelo mesmo cache — concluído e certificado.
       - **Cache verification policy (P04-G09)**: `aipo package cache verify <dir>` audita todas as entradas existentes, valida segurança e digests, continua após falhas independentes e nunca cria, repara, remove ou acessa a rede — concluído e certificado.
       - **Explicit cache pruning (P04-G10)**: `aipo package cache prune <dir> --lock <lockfile>` faz dry-run por padrão; `--apply` remove somente entradas verificadas não referenciadas e aborta sem mutação se houver erro — concluído e certificado.
       - **Opt-in GitHub authentication (P04-G11)**: `package fetch-github` e `package lock --fetch-github` aceitam `--github-token-env <name>`, validam credenciais bearer, mantêm o padrão público e nunca persistem ou logam o valor — concluído e certificado.
   - **Product boundary (`aipo v0.1.0`)**: release language-first com async, CLI mínima, C ABI síncrona, thin proofs Rust/C/JS e test runner; engines, editors, registry e web products ficam pós-v1 — decisão em `docs/adp/ADP-008-v0.1.0-language-release.md`.
   - **Cross-language benchmark protocol**: `aipo-bench --compare` cobre Aipo CLI/VM, VM in-process, Aipo→JavaScript/Node, Lua/LuaJIT, Wren, Luau, CPython/PyPy, Ruby, JavaScript/Node e Rust nativo com checksums, limits, provenance e `peak_rss_bytes` amostrado — evidência manual em `docs/performance/cross-language.md`.
  - **Maturidade e Endurecimento do Frontend**: Varredura completa e auditoria 100% resolvida: (1) `aipo-source` com alinhamento seguro de caracteres UTF-8 em `Source::location` (`is_char_boundary`, compatível com a MSRV); (2) `aipo-lexer` com rejeição canônica de números malformados (`1.e5`, `1._5`) emitindo `AIPO_LEX_INVALID_NUMBER` e matching zero-alloc de keywords sobre `&str`; (3) `aipo-syntax` com parsing e AST de expressões condicionais de valor inline (`if c then a else b`), preservação de `async` em métodos de `interface`, recuperação resiliente de erros via `synchronize()` e desaçucaramento de continuações elididas de comparação (`val >= 0 and <= 100`); (4) `aipo-hir` com spans unificados no hook `invariant()`; (5) `aipo-sema` com validação estrita de contratos de interface (métodos, aridades, receptor mutável, `async`, contratos de tipo de parâmetro e retorno), verificação estática de mutação de campos imutáveis (`AIPO_SEM_FIXED_REASSIGN`) e contratos de `Tipo:metodo` e funções locais; (6) Lambdas curtas `=>` completas de ponta a ponta. Suíte de testes do frontend com 87 testes unitários e de propriedades verdes.
  - **Maturidade e Endurecimento da Stdlib, Backend e Paridade JS**: 4 etapas de maturidade da stdlib e backend concluídas com 100% TDD e paridade diferencial: (1) Operações ávidas de coleções (`list.map`, `list.any`, `list.all`, `list.flat_map`, `list.reduce`) e `dict.entries()`; (2) Extensões canônicas de `math` (trigonometria, logs, exp, hypot, sign, rad, deg) com checagem estrita de domínio de funções e Model B `Failure`; (3) Módulo `random` determinístico (PRNG SplitMix64 idêntico a nível de bits na VM e no JS, `random.create`, `random.seed`, despacho de métodos nativos em `Struct`); (4) Módulo `json` portável (`json.parse` com rejeição de chaves duplicadas, `json.stringify` com detecção de ciclo e formatação canônica de floats e inteiros); (5) Módulos canônicos `encoding`, `path`, `url` e `regex`; (6) Módulos canônicos `binary` (operações BE/LE sobre Bytes, varints LEB128 com limite de segurança, fatiamento), `time` (tipos puros de calendário civil `Date`, `TimeOfDay`, `DateTime`, construtores, algoritmo civil de Howard Hinnant para dias do epoch, parsers ISO 8601, `.to_iso()`, `.epoch_seconds()`), `testing` & `expect` (asserções puras com Model B `Failure`), e `log` (logging estruturado por nível com sink configurável). Suíte de testes com 26 testes diferenciais VM↔JS e 100% dos gates de qualidade verdes.
  - **Maturidade, Endurecimento e Otimização de Bytecode e VM**: (1) Serialização binária nativa e determinística `.aibc` (`write_to`, `read_from`, `to_bytes`, `from_bytes`) com verificação rigorosa de cabeçalho `AIBC` v1; (2) Mapeamento de linhas e colunas de código-fonte no desassemblador (`disassemble_with_source`); (3) Otimização do layout de `Value` na VM com variantes empacotadas em `Rc` (`Closure`, `BoundMethod`) reduzindo o tamanho de cada valor de ~72/80 bytes para 16 bytes neste alvo (medido por `layout_probe`) e eliminando a alocação `Box<Value>` de receptor em métodos ligados; (4) Travessia recursiva de segurança contra escape de capacidades em `publish_check` cobrindo Closures, BoundMethods e Sequences; (5) Detecção de ciclos de await transitivos estendida através de `WaitTarget::Join` e grupos de tarefas; (6) Preservação de erros originais em métodos de lista de ordem superior e liberação garantida do guard de iteração ativa em `reduce` contra travamento espúrio da coleção; (7) Endurecimento do verificador de bytecode com limites estritos em slots de locais, upvalues e contagem de campos de structs; (8) Diagnóstico explícito no emissor de bytecode para operadores não-reconhecidos; (9) Execução direta de arquivos `.aibc` pré-compilados via `aipo run` com detecção automática de extensão, desserialização, verificação e execução sem pipeline frontend; (10) Subcomando `aipo disasm` para desassembly de arquivos `.aipo` (com anotações de linha/coluna) e `.aibc` (sem anotações). 100% dos testes e verificações clippy verdes no workspace.
   - **Performance do caminho quente VM/JS — ganhos mantidos**: cache de constantes, slots de globals, índices de fields/methods, cache monomórfico de field slot, deduplicação de constantes, peephole de `PropagateFailure`, fast path ASCII, argumentos nativos emprestados, locals/upvalues JS por slot e cache preguiçoso de constantes no shim JS. Ganhos que a medição sustenta: cache de field slot reduziu `fields` de 1.001,77ms para 718,20ms (**-28,31%**); cache do frame base, que evita `frames.last()` por acesso a local, reduziu `arithmetic` -3,71%, `fields` -6,65% e `recursion` -4,17% com métricas e checksums idênticos. `SetField` deixou de clonar o valor de entrada para structs unguarded, eliminando 600.000 clones de `Value` de forma determinística com semântica e rollback preservados — mantido como redução de trabalho, **sem claim de velocidade**, porque o A/B de 31 samples não mostrou ganho de wall-clock. Evidência em `docs/performance/cross-language.md`.
     - **Sete experimentos revertidos por ausência de ganho demonstrado**: metadata de métodos cacheada, pool de nomes (`Rc<str>` e `Owned`/`Shared`), entry de cache com guardedness, flags `fixed` por slot, cache de tipos guarded, layout denso de `StructInstance` e despacho de método sem alocar. Todos medidos com `scripts/perf/paired.sh`, todos revertidos, e o código de produção permanece o anterior. Detalhes e relatórios por experimento em `docs/performance/cross-language.md`.
     - **Diagnóstico do laço de despacho**: calibração com três programas com número fixo de iterações mostrou que o custo por opcode é **constante** (~100ns) e independe da mistura de operações, o que localiza o custo no despacho e não em qualquer handler. Ablação descartou a contabilidade de métricas como causa (1% a 2%). O gargalo estrutural identificado é `VmFault` com 72 bytes, que torna `Result<_, VmFault>` largo em `push`, `pop` e `read_u16` e em todo opcode.
     - **Despacho de opcode sem `Result` largo**: `Vm::step` decodificava cada opcode com `try_from(..).map_err(..)?`, construindo um `Result` de 72 bytes e um closure que capturava `self` em **todo** opcode executado. O `match` equivalente só monta a falha no ramo frio, com offset e mensagem idênticos, e `test_unknown_opcode_reports_the_offset_of_the_bad_byte` fixa esse contrato. A medição por tempo de CPU com 24 repetições deu **nulo** (min -0,16%), porque o compilador já afundava a construção da falha para fora do caminho feliz. Mantido como **robustez e clareza, não como otimização**.
     - **Metodologia de medição (precondição para qualquer trabalho futuro)**: este host entrega ~±5% de ruído em `fields` e `arithmetic` em condições normais e **até 2,5x sob contenção**, com o mesmo binário variando 2,5x entre execuções (load average observado de 7,3 em 4 cores). Regras que decorrem disso e devem ser obedecidas: (1) diferença menor que ±5% não é evidência; (2) um único lote pareado não decide nada, e toda candidata precisa de pelo menos dois lotes; (3) `arithmetic` é o workload de controle por não usar structs nem despacho de método; (4) posição de campo em structs grandes é parte do resultado, porque deslocamento de offset alternou o mesmo código entre +7% e neutro; (5) sob disputa, wall-clock não decide e `scripts/perf/cpu_ab.py` (tempo de CPU de `RUSAGE_CHILDREN`) é o estimador robusto, com o **mínimo** de muitas repetições como decisão; (6) código que o compilador já elimina do caminho feliz não deve ser reescrito por ganho de código-fonte — medir antes de assumir.
     - **Roadmap de performance — encerrado e replanejado**: a trilha de micro-otimização do interpretador está **encerrada**. Sete experimentos consecutivos não produziram ganho demonstrável, e o diagnóstico mostrou que o gap real é de arquitetura, não de ajustes locais. O `fields` roda em ~1.470ms contra 120ms do Wren e 63ms do Luau, cerca de **12x**, e nenhuma das mudanças testadas o moveu. Reabrir a performance exige, **nesta ordem**: (1) host dedicado ou ocioso para estabelecer linha de base confiável, já que hoje nenhuma medição de wall-clock aqui decide; (2) `Call0..Call4` e `GetLocal8`, que reduzem dispatches por chamada e são a maior mudança estrutural de curto prazo com escopo fechado; (3) se ainda insuficiente, despacho *threaded* com jump table em vez de `match`, que é a mudança de maior teto. **Não tentar** mais variantes de cache, mais flags, mais pools de nomes ou mais reordenação de struct: sete tentativas já provaram que esse caminho não paga. Detalhes e método em `docs/performance/cross-language.md`.
     - **Performance reference expansion**: Wren 0.4.0/Wren CLI, Luau 0.739 e PyPy 8.0.0 adicionados como runtimes opcionais; fixtures, argumentos de CLI, fontes pinadas e paridade de checksum validados; RSS amostrado em Linux separado de timing com `--compare-resources`. Allocation counters cross-runtime permanecem pendentes.
  - **Maturidade de IR, Host e Runtime (auditoria exaustiva)**: (1) `aipo-ir` com `CompoundAssign` em `Index` avaliando `base`/`idx` uma única vez (hidden locals), targets inválidos emitindo `Fail` explícito e `FnDecl` registrando o nome em `seen`; (2) `aipo-host` com `HostFault::Schema → AIPO_RT_INVALID_SCHEMA`, `HandleTable::clear()` reaproveitando slots e namespace por categoria documentado; (3) `aipo-runtime` com `InitializationFailed → AIPO_RT_MODULE_INIT_FAILED`, `NativeRegistry` documentada como catálogo de introspecção e `topological_init_order` com min-heap; (4) catálogo de diagnósticos estendido com os dois novos códigos. Gates verdes (fmt, clippy zero warnings, testes, doc).
  - **Performance & Ergonomia de Linguagem (ADP-011)**:
    - **P1 (Compactação de Layout de `Value`)**: enum reduzida de 40B para 16B neste alvo (medido por `layout_probe`), economia substancial de cache L1/L2 e ganho empírico comprovado de **-13,94% na mediana de CPU**.
    - **P2 (Despacho Direto de Métodos de Structs)**: variante leve `Value::StructMethod` sem alocação no heap de `Rc<BoundMethodData>`, eliminando 200k alocações em 100k chamadas com ganho de **-7,54% na mediana de CPU**.
    - **P4 (Superinstruções `GetLocal0..7` e `Call0..4`)**: opcodes de byte único (60..72) eliminando decodificação de operandos no caminho quente; tracking exato de `call_inst_len` para suspensões cooperativas/temporizadas; ganho comprovado de **-8,86% mínimo / -6,49% mediana**.
    - **E1 (Falhas Estruturadas Tipadas / Model B+)**: `FailureValue.payload: Value`, construtores tipados e acesso a `err.payload` com 100% de paridade diferencial VM↔JS.
  - **Ergonomia e Pivot Moderno de Linguagem (ADP-012)**:
    - **Sintaxe de Blocos em Chaves (`{ ... }`)**: Delimitação universal de blocos com `{ ... }` em todas as estruturas de controle (`if`, `while`, `loop`, `repeat`, `each`, `match`, `attempt`, `await do`) e definições de itens (`fn`, `struct`, `impl`, `init`, `invariant`, `interface`), dispensando parênteses em condições e abolindo o `end`.
    - **Imutabilidade de Structs por Padrão**: Campos declarados de forma simples em `struct` são imutáveis por padrão. Mutabilidade exige prefixo explícito `var`. A palavra-chave `fixed` foi extinta.
    - **Instanciação com Dois-Pontos (`:`)**: Structs instanciadas com notação simétrica a dicionários (`User{ name: "Dev", age: 30 }`).
    - **Mutabilidade Universal de Receptores (`var self`)**: Receptores mutáveis padronizados com `var self` (substituindo `self!`).
    - **Subtipagem Estrutural Automática**: Structs satisfazem interfaces implicitamente quando suas assinaturas conferem; declarações top-level `satisfy` tornaram-se opcionais.
    - **Divisão Inteira Truncada (`//` e `//=`)**: `//` e `//=` integrados no lexer, parser, formatter e pipeline para divisão inteira em direção a zero.
    - **Formatter Canônico & Exemplos**: `aipo-formatter` e os 24 programas em `examples/` modernizados e validados com 100% de paridade entre VM e JS.
    - **Documentação Multi-idiomas Completa**: 100% das páginas de documentação em Português e Inglês atualizadas e construídas com VitePress sem erros.
  - **Test Runner Canônico `aipo test` (ADP-008, ADP-012)**:
    - Descoberta automática recursiva de testes em `*_test.aipo` e `test_*.aipo`, filtros `--filter`, saída humana e streaming NDJSON `--message-format <human|jsonl>`.
    - Execução isolada em VM limpa com congelamento temporal via `DeterministicClock` e semente PRNG zerada (`reset_default_seed(0)`).
    - Suporte nativo via `TestMode::Discover` e `TestMode::Execute` em `aipo-vm` e shims em `aipo-js`.
  - **C ABI Estável e Embedding Interface (ADP-009, ADP-010)**:
    - Crate `crates/aipo-c-abi` e cabeçalho canônico C `crates/aipo-c-abi/include/aipo.h`.
    - Interface C síncrona, versionada (`aipo_version`), puramente de thread única (`aipo_runtime_t`), com proteção integral de unwinding via `catch_unwind(AssertUnwindSafe(...))`.
    - Tipos de valor em C (`aipo_value_t`), handles geracionais (`aipo_handle_t`) com detecção de `AIPO_ERR_STALE_HANDLE`, callbacks nativos (`aipo_host_fn_t`), gating de capacidades (`grant`/`revoke`), e diagnóstico em `aipo_last_error`.
    - Suíte de 8 testes de integração exaustivos cobrindo todo o ciclo de vida, primitivas, callbacks, handles, erros e segurança contra ponteiros nulos.
- Context methodology: **Lean Progressive Context (LPC)**
- Last updated: `2026-10-27T00:00:00Z`
- **Fio Vertical Fino Wasm (Marco 1 / P06-G01)**:
  - Crate `aipo-wasm` integrada ao workspace com emissor de seções Wasm (`WasmEmitter`), mapeamento de tipos (`WasmType`, `WasmFnType`), e compilador de HIR para Wasm (`compile_hir`).
  - Suporte a funções aritméticas com contratos estáticos (`Int`, `Float`, `Bool`) e inferência de retorno.
  - Alocação e compressão de variáveis locais Wasm (`let`, `var`), reatribuição (`=`) e atribuições compostas (`+=`, `-=`, `*=`, `//=`, `%=`).
  - Emissão de operadores aritméticos (`+`, `-`, `*`, `//`, `/`, `%`) e relacionais (`==`, `!=`, `<`, `<=`, `>`, `>=`).
  - Chamadas diretas entre funções compiladas e geração de ponto de entrada (`__top_level__`).
  - Suíte de 17 testes automatizados com execução JIT direta via `wasmtime` 100% verde.
- **Controle de Fluxo Estruturado Wasm (Marco 2 / P06-G02)**:
  - Implementação completa de blocos condicionais (`if`, `elif`, `else`) e expressões de valor condicional (`if c then a else b`).
  - Implementação de laços de repetição mapeados para Wasm `block` + `loop`: `while condition { ... }`, `loop { ... }` incondicional, e `repeat count [as i] { ... }` com passo e contador de iteração.
  - Saltos rotulados (`br`, `br_if`) gerenciados via pilha de controle (`ControlFrame`) calculando a profundidade relativa exata para `break` e `continue` em qualquer nível de aninhamento.
  - Diagnóstico e rejeição estática de `break` e `continue` fora de contexto de laço.
  - Suíte de 31 testes de ponta a ponta validados com execução JIT `wasmtime` 100% verde.
- **Modelo de Memória Linear e Estruturas de Dados Wasm (Marco 3 / P06-G03)**:
  - Memória linear WebAssembly configurada e exportada (`memory`, com 1 página inicial de 64 KiB expansível dinamicamente).
  - Alocador nativo de bump em Wasm `__aipo_alloc(size: i32) -> i32` alinhado a 8 bytes, com crescimento sob demanda de páginas de memória (`memory.grow`) e armadilha para OOM.
  - Pool de strings estáticas na seção de dados Wasm (`DataSection`) com prefixo de tamanho de 4 bytes e suporte à propriedade `.len`.
  - Layout e alinhamento de memória para structs (`StructLayout`) com cálculo estático de deslocamentos de campos alinhados a 8 bytes e suporte a campos tipados (`Int`, `Float`, `Bool`, ponteiros).
  - Instanciação de structs (`HirExpr::Construct`) com alocação dinâmica em memória linear, suporte a inicialização nomeada e posicional, e pilha de 8 variáveis locais temporárias (`__struct_temp_0..7`) para aninhamento arbitrário seguro de instâncias de struct literais.
  - Acesso a campos (`HirExpr::Dot`) e mutação em memória via atribuição direta (`p.x = val`) e composta (`p.x += delta`) emitindo instruções de carregamento e armazenamento de memória (`i64.load`, `f64.load`, `i32.load`, `i64.store`, `f64.store`, `i32.store`).
  - Rastreamento de tipos de variáveis locais (`LocalKind::Struct, LocalKind::String, LocalKind::Int...`) para desambiguação semântica estrita entre acessos a propriedades de strings e campos de structs.
  - Suíte de 34 testes de ponta a ponta validados com execução JIT `wasmtime` 100% verde.
- **Chamadas de Função, Recursão e Tabela de Indireção Wasm (Marco 4 / P06-G04)**:
  - Emissor WebAssembly estendido com `TableSection` (Seção 4) e `ElementSection` (Seção 9) seguindo estritamente a ordenação canônica de seções do Wasm 2.0.
  - População automática da Tabela 0 (`funcref`) indexando todas as funções declaradas e funções anônimas sintetizadas (`__anon_fn_N`).
  - Suporte nativo completo a recursão direta (`factorial`, `fibonacci`) e recursão mútua (`is_even`, `is_odd`) emitindo instruções diretas `call` de zero overhead.
  - Implementação de despacho indireto via `call_indirect` para funções de primeira classe, suporte a atribuição de referências de função a variáveis locais (`let f = increment`), e passagem de funções como parâmetros de ordem superior (`apply(f, x)`).
  - Suporte completo a funções anônimas e lambdas curtas (`x => x * 2`, `(a, b) => a + b`), sintetizadas e compiladas como funções canônicas do módulo e registradas na Tabela 0.
  - Pré-registro de assinaturas de tipos para chamadas indiretas (`indirect_sigs`) cobrindo aridades de 0 a 8 com deduplicação estrutural de tipos.
  - Pilha de 8 temporários de chamada indireta (`__call_temp_0..7`) para suporte seguro a aninhamento arbitrário de chamadas indiretas (`f(g(h(x)))`).
  - Suíte de testes de integração expandida para 45 testes automatizados verdes em `crates/aipo-wasm` (40 de pipeline + 5 de emitter), executados e validados pelo motor JIT `wasmtime`.
- **Runtime Async/Await Wasm (Marco 5 / P06-G05)**:
  - Implementação completa do runtime cooperativo de tarefas assíncronas no módulo WebAssembly sem dependências externas.
  - Struct `AsyncHelpers` centralizando índices de `__aipo_task_create`, `__aipo_task_drive`, `__aipo_await`, `__aipo_task_sleep` e `__aipo_task_cancel`.
  - Layout linear de 48 bytes por tarefa com despacho polimórfico via `call_indirect` (aridades 0, 1 e 2).
  - Compilação de funções `async fn` com divisão automática em corpo (`__async_body_{name}`) e função wrapper de instanciação de tarefa.
  - Compilação de `HirExpr::Await` invocando `__aipo_await` até conclusão da tarefa (status Ready) retornando `I64`.
  - Exportação de global `__aipo_virtual_time` (i64, mutável) e threading de `async_helpers` por toda a árvore de compilação.
  - Suíte de testes de integração expandida para 50 testes automatizados verdes em `crates/aipo-wasm` (45 de pipeline + 5 de emitter).

- **Ferramental CLI, Host ABI & WASI (Marco 6 / P06-G06)**:
  - Integração nativa e ergonômica de WebAssembly no CLI (`aipo run`, `aipo build`, `aipo check`, `aipo disasm`).
  - Execução JIT de arquivos `.aipo` via `--wasm` / `-t wasm` / `--target=wasm` e execução direta de binários `.wasm` pré-compilados sem fonte.
  - Emissão de binários `.wasm` pelo CLI com `aipo build <path> --target wasm` (ou `--wasm`) gerando `dist/app.wasm`.
  - Desassembly para WAT padrão com `aipo disasm <file.wasm>` e `aipo disasm <file.aipo> --wasm` via `wasmprinter`.
  - Host ABI `"aipo_host"` em `aipo-wasm::runner` provendo bindings para `print_int`, `print_float`, `print_str`, `print_bool`, `println` e suporte condicional a seções de imports para `io.print` / `io.println`.
  - Suíte de 8 testes de integração exaustivos em `crates/aipo-cli/tests/wasm_cli.rs` cobrindo 100% dos novos fluxos e tratamento de erros.

- **Bateria de Benchmarks e Comparação Multi-linguagem Wasm (Certificação ADP-013)**:
  - Integração do runtime `aipo-wasm` ao harness comparativo canônico `aipo-bench --compare` (`crates/aipo-bench/comparisons/manifest.json` e `crates/aipo-bench/src/compare.rs`).
  - Suporte a interpolação de format strings desaçucaradas (`BinaryOp::Add` e chamada `String(...)`) com streaming print de zero heap allocation em `crates/aipo-wasm`.
  - Resultados empíricos certificados em bateria de 7 rounds (`target/bench_wasm_comparison.json`):
    - `arithmetic` (N=200.000): **10,53ms min / 15,72ms mediana** — ganho de **17x a 25x** sobre a Stack VM anterior, **4.0x mais rápido que Node.js**, **4.4x mais rápido que CPython**, **6.9x mais rápido que CRuby**, empatando com Lua 5.4 e ficando a apenas **1,9x** do teto de Rust nativo compilado.
    - `recursion` (Fibonacci 24): **10,60ms min / 11,35ms mediana** — ganho de **8.6x** sobre a Stack VM, **1.4x mais rápido que Lua 5.4**, **2.8x mais rápido que CPython**, **5.3x mais rápido que Node.js** e a **1.9x** do Rust nativo.
    - `startup`: **15,64ms** incluindo parsing, lowering, compilação Wasm e compilação JIT de código de máquina pelo Cranelift — superando os tempos de startup de CPython (21ms), Node.js (58ms) e Ruby (90ms).
  - Documentação canônica atualizada e sincronizada em `docs/performance/cross-language.md`.

- **Engine 2D Nativa e Host Desktop Miniquad (`aipo-game-host` & `packages/aipo-game`)**:
  - Implementação completa da crate `crates/aipo-game-host` conectando scripts dinâmicos `.aipo` diretamente ao loop GPU nativo a 60 FPS com backend Miniquad / Macroquad.
  - Ponte FFI de hardware (`host_bridge.rs`) fornecendo 23 funções nativas aceleradas: renderização 2D (retângulos, círculos, texto), carregamento e desenho de texturas e fatiamento de spritesheets com cache seguro, câmera 2D com tracking suave e zoom, e mapeamento de entradas de teclado/mouse GLFW.
  - Live Hot-Reload dinâmico via `F5` / `Ctrl+R` com overlay gráfico de diagnósticos em tela para erros de sintaxe ou tipos.
  - Proteção total headless com `catch_unwind(AssertUnwindSafe)` permitindo execução estável de 100% dos testes sem servidores X11 ou janelas gráficas.
  - Publicação de superfícies de compilação em `aipo-cli` (`compile_file`, `analyze_with_surface`, `prelude_surface`, `register_module_symbols`, `emit_diagnostics`).
  - Refatoração e validação canônica de 100% dos módulos do pacote `packages/aipo-game/src/` (`actor`, `audio`, `behaviors`, `ffi`, `input`, `lib`, `nodes`, `scene`, `sfx`, `tween`) aprovados pelo `aipo check`.
  - Exemplos interativos canônicos `examples/26_interactive_game.aipo` e `examples/27_camera_and_sprites.aipo` validados em suíte automatizada de testes.
- **Pipeline de Áudio Nativo e Sintetizador Chiptune Procedural (`aipo-game-host` & `packages/aipo-game`)**:
  - Backend de áudio acelerado integrado via Macroquad / QuadSnd com carregamento WAV/OGG e reprodução de trilha sonora em loop contínuo.
  - Sintetizador procedural em memória (`audio_system.rs`) estilo SFXR/ChipTone gerando streams RIFF/WAV 16-bit PCM (44.1 kHz, mono) sem arquivos de assets externos.
  - Formas de onda suportadas: Square (com duty cycle ajustável), Sawtooth, Sine, Triangle e Noise (via PRNG Xorshift32 rápido).
  - Envelopamento anti-click de 5ms, decaimento exponencial e deslizamento dinâmico de frequência (`freq_slide`).
  - Biblioteca de presets retro: `"coin"`, `"laser"`, `"jump"`, `"explosion"`, `"hit"`, `"powerup"`, `"click"`.
  - Ponte FFI (`host_bridge.rs`) expandida para 30 funções nativas com 7 novas funções de áudio (`host_load_sound`, `host_play_sound`, `host_play_preset`, `host_synth_sound`, `host_stop_sound`, `host_play_music`, `host_stop_music`).
  - Resolução assíncrona em 100% Safe Rust (`std::task::Waker::noop()`, `std::pin::pin!`) cumprindo `#![forbid(unsafe_code)]`.
  - Resiliência total para CI e ambientes headless sem placa de som através de `std::panic::catch_unwind(AssertUnwindSafe(...))`.
  - Módulos `packages/aipo-game/src/` (`ffi.aipo`, `audio.aipo`, `sfx.aipo`) e exemplos (`26_interactive_game.aipo`, `27_camera_and_sprites.aipo` e Snake embutido) integrados e validados por 9 testes automatizados verdes.
- **Adaptador Host Agnóstico e Bindings Immediate-Mode GUI (`aipo-egui` & `packages/aipo-egui`)**:
  - Crate `crates/aipo-egui` concretizando o *thin proof* de interoperabilidade Rust exigido pelo ADP-010.
  - Arquitetura 100% agnóstica de game engines operando sobre a máquina de estados pura do `egui::Context`, sem dependências de janelas de SO (zero `winit`) ou bibliotecas de jogos.
  - Segurança estrita Safe Rust (`#![forbid(unsafe_code)]`), conformidade total com o modelo de capabilities (`deny-by-default` com capability `"egui"`) e handles geracionais (`aipo-host::HandleTable`) prevenindo *use-after-free*.
  - Extração universal de primitivas geométricas (`shapes`): converte a saída do `egui::FullOutput` em uma lista de dicionários (`rect`, `text`, `circle`, `line`, `bezier`, `path`) consumível por qualquer renderizador externo (GPU, Wasm Canvas, TUI ou simulação headless).
  - API completa de widgets imediatos exposta ao VM: `create_context`, `destroy_context`, `begin_frame`, `end_frame`, `begin_window`, `end_window`, `label`, `heading`, `separator`, `button`, `checkbox`, `slider`, `text_edit`, `progress_bar`, `wants_pointer_input`, `wants_keyboard_input`.
  - Pacote canônico `packages/aipo-egui` com manifesto `aipo.toml` e ponte `ffi.aipo` validada pelo `aipo check`.
  - Exemplo executável `examples/28_egui_immediate_gui.aipo` e respectivo snapshot de saída `examples/28_egui_immediate_gui.stdout`.
  - Suíte de 7 testes de integração automatizados em `crates/aipo-egui/tests/egui_integration_tests.rs` cobrindo o ciclo de vida, negação padrão de permissões, handles obsoletos, interações sintetizadas de ponteiro e execução de script de ponta a ponta.

- **Framework Declarativo de GUI Zoe UI e Engine Torin (`packages/aipo-zoe`)**:
  - Implementação completa do pacote `packages/aipo-zoe` (`aipo.zoe`), disponibilizando a arquitetura declarativa de GUI do Zoe UI para Aipo com aceleração por GPU via `aipo-game-host`.
  - **Engine de Layout Torin (`torin.aipo`)**: Algoritmo hierárquico resolvendo dimensões absolutas em pixels (`Float`/`Int`), percentuais (`"100%"`, `"50%"`), intrínsecas e espalhamento flexível (`"flex"`, `"auto"`), com alinhamento (`align_items`), distribuição (`justify_content`), espaçamento interno (`padding`) e vão entre filhos (`gap`).
  - **Reatividade com Hooks (`hooks.aipo`)**: Runtime reativo (`use_state`, `set_state`) com rastreamento automático de quadros alterados (*dirty frames*), disparando reconstrução e re-layout eficientes apenas em caso de mutação.
  - **Catálogo de Componentes e Elementos (`elements.aipo`, `components.aipo`)**: Primitivas fundamentais (`rect`, `label`, `container`) e componentes de alto nível (`button`, `switch`, `slider`, `progress_bar`, `card`, `badge`).
  - **Paleta Catppuccin Mocha Embutida (`color.aipo`)**: Suporte a `rgb`, `rgba`, `hex` e constantes de tema modernas (`crust`, `mantle`, `base`, `surface_0..2`, `blue`, `lavender`, `green`, `red`, etc.).
  - **Ciclo de Vida e Renderizador GPU (`app.aipo`, `renderer.aipo`)**: Despacho de desenho recursivo, hit-testing de eventos de ponteiro/clique e renderização a 60 FPS com integração de lifecycle (`setup`, `update`, `draw`).
  - **Correção no Compilador (`crates/aipo-cli/src/modules.rs`)**: Reescrita de itens de topo do arquivo de entrada com `entry_scope` viabilizando resolução uniforme de acessos a módulos importados em funções.
  - **Demonstração e Testes**: Exemplo completo `packages/aipo-zoe/examples/dashboard.aipo`, suíte de testes de unidade `packages/aipo-zoe/tests/zoe_test.aipo` e testes de integração em `bridge_tests.rs` 100% verdes.

- **In-Game UI / HUD Imediato e Sistema de Tilemaps 2D com Colisão Contínua (`packages/aipo-game` & `aipo-game-host`)**:
  - **In-Game UI / HUD Imediato (`packages/aipo-game/src/ui.aipo`)**: Subsistema de HUD em modo imediato desenhado a 60 FPS com botões interativos (`button`), barras dinâmicas de vida com gradação de cor (`health_bar`), barras de recursos (`progress_bar`), janelas decorativas (`panel`), tipografia com sombra (`label`) e tags em formato de pílula (`badge`).
  - **Grid Tilemap 2D & Colisão AABB (`packages/aipo-game/src/tilemap.aipo`)**: Criação e manuseio de mapas tabulares com consultas espaciais $O(1)$ (`is_solid_cell`, `check_box`), descarte de tiles fora de visão (*viewport frustum culling*) e traçado rápido de raios em grade discreta (*DDA raycasting*).
  - **Resolução de Colisão Eixo por Eixo (*Swept AABB*)**: Algoritmo contínuo com separação de eixos $X$ e $Y$ que elimina o problema de atravessamento de paredes (*tunneling*) em alta velocidade e viabiliza deslizamento suave nas quinas de obstáculos.
  - **Nova Primitiva Host (`host_draw_line`)**: Integração de desenho de linhas com espessura e canal alfa no `host_bridge.rs` com fallbacks headless.
  - **Demonstração Completa (`tilemap_and_hud.aipo`)**: Exemplo executável em `packages/aipo-game/examples/tilemap_and_hud.aipo` integrando movimentação de jogador, câmera suave, zoom interativo, mira laser via raycast e HUD responsivo.
  - **Testes e Qualidade**: 100% dos 12 testes em `crates/aipo-game-host` e toda a suíte do workspace aprovados sem avisos de linter.

- **Máquina de Estados de Animação 2D e Sistema de Partículas com Física (`packages/aipo-game` & `aipo-game-host`)**:
  - **Máquina de Estados e Animação de Sprites (`packages/aipo-game/src/animation.aipo`)**:
    - Estruturas de dados `AnimationClip` e `SpriteAnimator` para controle quadro-a-quadro de spritesheets 2D.
    - Suporte a clipes cíclicos (`is_looping == true`) e de disparo único (*one-shot* com travamento no último quadro e sinalizador `is_finished`).
    - Recorte UV automatizado de células de spritesheet com mapeamento bidimensional de linha/coluna e renderização acelerada por GPU (`draw_animator`).
    - Suporte a espelhamento horizontal instantâneo (`flip_x`) conforme a direção do ator.
    - Introspecção de estado da animação (`current_clip`, `get_current_frame`, `is_finished`).
  - **Sistema de Partículas 2D com Física e Presets (`packages/aipo-game/src/particles.aipo`)**:
    - Emissor de partículas de alta densidade (`ParticleEmitter`) com alocação protegida e teto configurável `max_particles`.
    - Simulação física completa com vetores de velocidade ($v_x, v_y$), aceleração gravitacional contínua (`gravity_y`) e amortecimento por arrasto aerodinâmico (`drag`).
    - Desvanecimento linear de opacidade (*alpha fading*) e interpolação de tamanho com suporte a partículas circulares e retangulares.
    - Catálogo de presets visuais prontos para uso em jogos: `"sparks"`, `"explosion"`, `"smoke"`, `"dust"`, `"coins"` e `"trail"`.
  - **Demonstração Completa (`animation_and_particles.aipo`)**: Exemplo executável em `packages/aipo-game/examples/animation_and_particles.aipo` integrando ator animado (ciclos `idle`, `run` e `attack`), poeira de passos, faíscas e explosões com moedas e fumaça.
  - **Testes e Qualidade**: 100% dos 11 testes em `crates/aipo-game-host` e suíte de testes em Aipo puro `packages/aipo-game/tests/game_test.aipo` aprovados com zero warnings.

  - **Referência canônica de sintaxe e reparação de `aipo.ui`**:
    - `SYNTAX.md` (raiz, normativo): superfície sintática extraída do compilador — 42 keywords + `_`, 36 operadores, blocos `{ }` exclusivos e os limites do ADP-005. Fecha as lacunas do manual (`fixed` como legado, `end`, `satisfy`, `div`, `do`, e o fato de `#` ser o único comentário enquanto `//` é divisão inteira).
    - `packages/aipo.ui` estava com 148 erros de parse nos 7 módulos e nenhum gate o compilava. Reparado e coberto por 9 testes em `packages/aipo-ui/tests/ui_test.aipo`, executados no CI por `crates/aipo-cli/tests/package_suites.rs`.
    - Quatro defeitos foram documentados em `docs/journal/2026-10-05-syntax-drift.md` e reverificados em 2026-10-27: **D1** (named args + bloco trailer), **D2** (closure aninhado/upvalue) e **D3** (`aipo check` e membro inexistente) não se reproduzem; **D3** agora emite `AIPO_SEM_UNKNOWN_NAME`. **D4** (aspas aninhadas em f-string) parseia; o caso testado avançou para um erro de tipo legítimo por outro motivo.
    - As suítes host-free de `aipo.ui`, `aipo.html`, `aipo.http` e `aipo.egui` passam pelo `aipo test`; `aipo.zoe` e `aipo.game` exigem o bridge de host e são executadas em `aipo-game-host/tests/bridge_tests.rs`.
    - `aipo fmt --check` está limpo nos 47 fontes de `packages/*/src`; o drift histórico não permanece.

  - **Polinização Cruzada e Hardening omniLua (AIPO-OMNI-001 a 005 Concluídos e Certificados)**:
    - **AIPO-OMNI-001 (Canários de Vazamento de Memória)**: `CountingAllocator` em `crates/aipo-testkit`, 8 cenários de churn (`leak_canaries.rs`) com $\Delta \text{bytes} = 0$.
    - **AIPO-OMNI-002 (Instruction Budget Sticky e Ininterceptável)**: `VmFault::BudgetExhausted` ininterceptável por `attempt { ... }` na Stack VM, RegVm, Wasm e CLI (`--budget`).
    - **AIPO-OMNI-003 (Embedding Scoped em Rust Puro)**: `AipoEngine::scope`, `ScopedHandle<'scope>`, userdata geracional, verificação de scope escape e zero `unsafe`.
    - **AIPO-OMNI-004 (Tarifação Periódica de Fuel / Interval Metering)**: Amortização do débito de fuel com verificação a cada 64 instruções, reduzindo 98.4% dos branches no hot path, com reconciliação estrita na saída.
    - **AIPO-OMNI-005 (Janelas de Registrador na `RegVm`)**: Pilha contígua `register_stack` para salvamento/restauração seletiva (`0..callee_regs`), eliminando cópia cega de 256 registradores por ativação (-98.8% de tráfego de pilha).
    - 100% dos testes e verificações do workspace aprovados sem warnings.

  - **Remediação de Defeitos e Estabilização Pré-Alpha v0.1.0**:
    - **Wasm Match Arm / Local Function IO Scanner**: Corrigida a omissão de chamadas de host I/O (`io.println`, etc.) dentro de braços `match` (`when_arms`), funções locais (`HirStmt::FnDecl`) e métodos em `crates/aipo-wasm/src/compiler.rs`. Removido código morto (`program_stmt_span`, helpers assíncronos não utilizados). Coberto por `test_host_io_in_match_arms_without_outer_io`.
    - **Stdlib `math.tau`**: Adicionada constante canônica `math.tau` (6.283185307179586) em `crates/aipo-stdlib/src/math.rs` e no runtime ESM JavaScript `crates/aipo-js/runtime/aipo-runtime.js`. Coberto por `test_math_constants`.
    - **Stdlib `Set()` Zero-Argument Constructor**: Habilitada a instanciação de conjuntos vazios `Set()` sem argumentos na Stack VM (`crates/aipo-stdlib/src/convert.rs`, `crates/aipo-vm/src/convert.rs`) e no runtime JS (`aipo-runtime.js`). Coberto por `test_empty_set_creation`.
    - **C ABI Header `aipo.h` Sincronizado**: Declarados os protótipos de embedding da Register VM (`aipo_reg_vm_t`, `aipo_reg_vm_create`, `aipo_reg_vm_destroy`, `aipo_reg_vm_set_reg_int`, `aipo_reg_vm_get_reg_int`, `aipo_reg_vm_run`) em `crates/aipo-c-abi/include/aipo.h`.
    - **Suíte de Testes Automatizada para `aipo-egui`**: Criada suíte completa em `packages/aipo-egui/tests/egui_test.aipo` e integrada ao runner `crates/aipo-cli/tests/package_suites.rs`.
    - **Sincronização de Documentação Canônica**: Atualizado `SYNTAX.md` (preâmbulo e conformidade com ADR-001), `docs/reference/cli.md` (`--engine`, `--budget`, `--max-instructions`) e harmonizado o código de diagnóstico `AIPO_SEM_FIXED_REASSIGN` nos manuais e decisões.
  - **Substrato de Execução WebAssembly & C ABI (ADP-009, ADP-013)**:
    - **Crescimento Dinâmico de Listas e Buffer Geométrico (`aipo-wasm`)**: Refatoração do layout de listas para ponteiro indireto estável (`[data_ptr: i32]`) apontando para buffer redimensionável (`[len: i32, cap: i32, elements: [i64; cap]]`). Implementada rotina `__aipo_list_add` com realocação geométrica 2x em memória linear e lowering direto de `list.add(item)`.
    - **Compilação e Despacho de Métodos de Structs (`aipo-wasm`)**: Coleta e compilação de blocos `HirItem::Impl` (incluindo `init` e métodos com receptor `self: i32`), funções batch `Tipo:nome`, e despacho polimórfico de chamadas de método (`receiver.method(args)`) e chamadas estáticas (`Type.method(args)`).
    - **Workloads `fields` e `collections` no Wasm**: Cobertura com novos testes de ponta a ponta (`test_list_dynamic_add_growth`, `test_struct_methods_invocation_and_mutation`, `test_benchmark_fields_workload_on_wasm`) executados via Cranelift JIT / Wasmtime.
    - **Bytecode Pré-compilado na C ABI (`aipo-c-abi`)**: `aipo_compile_to_bytecode`, `aipo_bytecode_free` e `aipo_runtime_load_bytecode` para compilação externa, serialização binária `.aibc` v1 e carregamento atômico direto sem passar pelo frontend.
    - **Passo-a-passo Cooperativo e Iteração Limitada na C ABI (`aipo-c-abi`)**: `aipo_runtime_prepare_module`, `aipo_runtime_step` e `aipo_runtime_run_until_idle` com controle de orçamentos de instruções e frame stepping.
    - **Compilação e Execução de Wasm via C ABI (`aipo-c-abi`)**: `aipo_wasm_compile`, `aipo_wasm_free` e `aipo_wasm_run` integrando execução JIT com orçamentação estrita de fuel (`fuel > 0`).
    - **Sincronização do Cabeçalho C (`include/aipo.h`)**: Declarações e documentação Doxygen canônicas no header `crates/aipo-c-abi/include/aipo.h`. Suíte de testes de integração exaustiva com 100% de aprovação.

## Next action

Todos os subsistemas essenciais do runtime de jogos (`aipo.game`) foram finalizados e testados com sucesso:
1. Loop de Jogo, Cenas, Atores e Câmera 2D (`aipo-game-host`)
2. Sistema de Áudio Nativo e Sintetizador Procedural Chiptune (`audio.aipo`, `sfx.aipo`)
3. Sistema de Interpolação e Animação Elástica (`tween.aipo`)
4. In-Game UI / HUD em Modo Imediato (`ui.aipo`)
5. Grid Tilemap 2D com Resolução de Colisão Swept AABB e Raycasting DDA (`tilemap.aipo`)
6. Máquina de Estados e Animação de Sprites 2D (`animation.aipo`)
7. Sistema de Partículas 2D com Física e Presets (`particles.aipo`)

Próxima etapa imediata: fechar os gates de release `v0.1.0` — executar Clippy/rustfmt no MSRV 1.96 e registrar a evidência final no painel; a suíte completa já passou com `libasound2-dev`. Depois disso, retomar a **Game Engine Visual Completa** (Editor Visual, Árvore de Cenas, Inspetor de Propriedades, Viewport Interativo, Editor de Nós e Integração de Tooling).

## Recovery order

1. `ENTRYPOINT.md` or the platform adapter.
2. `prumo.json`.
3. `PROJECT_STATE.md`.
4. `docs/PRUMO.md`.
5. Active Goal under `.ai/goals/`.
6. Only relevant canonical docs/symbols/tests selected by the context strategy.

Do not load the entire repository by default.
