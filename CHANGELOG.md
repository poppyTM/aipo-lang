# Changelog

Todas as alterações notáveis deste projeto são documentadas neste arquivo.
O formato baseia-se no [Keep a Changelog](https://keepachangelog.com/pt-BR/1.0.0/) e adere ao [Semantic Versioning](https://semver.org/lang/pt-BR/).

## Runtime e ferramentas — P07-G02, 2026-10-09

- RegVerifier valida operandos/pools/targets, inicialização definida pelo CFG e escopos. `ExecutionPlan` seleciona Reg verificado ou serviços canônicos com motivo explícito, sem descartar IR.
- Coleções/Dict recebem revisão estrutural independente do comprimento; guards acompanham callbacks, handlers e tasks. Payloads Rust List/Set/Bytes usam `Collection`, e funções Reg compartilham corpos por Rc.
- Linker append-only, Session/REPL persistente, multilinha/histórico/completion, reload com migração e rollback de heap/aliases/scheduler. Tarefas e resultados sobrevivem a novas unidades; remoção de declarações e layouts incompatíveis são tratados explicitamente.
- C ABI liga unidades e acrescenta begin/pump/abort, budgets/erros Reg e recusa de runtime ocupado. Wasm ganha prova semântica conservadora para `is`, rejeição de aliases/shadowing, limites separados de memória/saída/fuel e cache LRU por Engine.
- CLI recebe new/watch/profile/debug/plan/LSP e vendor offline. Perfis shell/web/embedded, MSRV 1.96, scripts de empacotamento/instalação/checksum e workflows de compilação/distribuição manual.
- Chamadas Reg movem somente janelas vivas e reutilizam constantes/provas do módulo; protocolo pareado e CSV publicados. Medições e checks têm escopo e revisão descritos na evidência.
- Guia completo de uso/reimplementação e continuidade dos nove estudos pinados. Goal permanece DRAFT: testes não executados por instrução do usuário, sem merge ou release pública.

## Revisão de runtime — reconstrução de 2026-10-09

- RegEmitter experimental agora retorna Result, rejeita IR/operandos sem suporte e calcula alturas pelo CFG para emitir ramos condicionais corretamente.
- RegVM usa aritmética/igualdade compartilhadas, verifica operandos/jumps/arity, restaura caller e isola handlers; recebe orçamento cumulativo opt-in, guardas e contratos fundamentais/nominais/nullable.
- Iteração primária de Dict retorna chaves em VM/Reg/JS; novos opcodes de projeção preservam o registrador completo do índice. String indexing compartilha helper sem Vec<char> temporário.
- Wasm recebe opções de fuel, MissingExport, checagem de memória do binding de string e entrega de saída após traps de start/entrypoint, com propagação de erro de escrita/flush. Removidos metadados privados mortos; suites JIT/formatter respeitam features.
- Estudos com nove SHAs completos, fetch/lock fora do repo, guia de uso/reimplementação, compatibilidade, limitações e roteiro de medição.
- Publicação em draft. Testes não executados nesta reconstrução por pedido do usuário; resultados de arquivos temporários perdidos não certificam esta revisão.

## [0.11.0] - Em desenvolvimento (Trilha WebAssembly & Self-Hosting)

- **Substrato de Execução WebAssembly & C ABI (ADP-009, ADP-013)**:
  - **Expansão do Backend WebAssembly (`crates/aipo-wasm`)**:
    - **Crescimento Dinâmico de Listas e Buffer Geométrico**: Refatoração do layout de listas para ponteiro indireto estável (`[data_ptr: i32]`) apontando para buffer redimensionável (`[len: i32, cap: i32, elements: [i64; cap]]`). Implementada rotina `__aipo_list_add` com realocação geométrica 2x em memória linear e lowering direto de `list.add(item)`.
    - **Compilação e Despacho de Métodos de Structs**: Coleta e compilação de blocos `HirItem::Impl` (incluindo `init` e métodos com receptor `self: i32`), funções batch `Tipo:nome`, e despacho polimórfico de chamadas de método (`receiver.method(args)`) e chamadas estáticas (`Type.method(args)`).
    - **Workloads `fields` e `collections` no Wasm**: Cobertura com novos testes de ponta a ponta (`test_list_dynamic_add_growth`, `test_struct_methods_invocation_and_mutation`, `test_benchmark_fields_workload_on_wasm`) executados via Cranelift JIT / Wasmtime.
  - **Evolução e Novos Exports na C ABI (`crates/aipo-c-abi`, `include/aipo.h`)**:
    - **Suporte a Bytecode Pré-compilado**: `aipo_compile_to_bytecode`, `aipo_bytecode_free` e `aipo_runtime_load_bytecode` para compilação externa, serialização binária `.aibc` v1 e carregamento atômico direto sem passar pelo frontend.
    - **Passo-a-passo Cooperativo e Iteração Limitada**: `aipo_runtime_prepare_module`, `aipo_runtime_step` e `aipo_runtime_run_until_idle` com controle de orçamentos de instruções e frame stepping.
    - **Compilação e Execução de Wasm via C ABI**: `aipo_wasm_compile`, `aipo_wasm_free` e `aipo_wasm_run` integrando execução JIT com orçamentação estrita de fuel (`fuel > 0`).
    - **Sincronização do Cabeçalho C**: Declarações e documentação Doxygen canônicas no header `crates/aipo-c-abi/include/aipo.h`. Suíte de testes de integração exaustiva com 100% de aprovação.

  - **Wasm Host I/O Scanner em Match Arms e Funções Locais (`aipo-wasm`)**:
    - Corrigido `scan_stmts_for_io` e `scan_expr_for_io` em `crates/aipo-wasm/src/compiler.rs` para inspecionar recursivamente braços `match` (`when_arms`), funções locais (`HirStmt::FnDecl`), listas, indexações e métodos (`HirItem::Impl`), garantindo a geração de imports de I/O de host no Wasm mesmo quando as chamadas ocorrem exclusivamente no interior dessas construções.
    - Eliminado código morto (`program_stmt_span`, helpers assíncronos não utilizados em `AsyncHelpers`).
    - Adicionado teste de regressão ponta a ponta `test_host_io_in_match_arms_without_outer_io` com execução via Wasmtime.
  - **Constante `math.tau` Canônica na Stdlib e Runtime JS (`aipo-stdlib`, `aipo-js`)**:
    - Adicionada constante `math.tau = 6.283185307179586` ($\tau = 2\pi$) na biblioteca padrão Rust (`crates/aipo-stdlib/src/math.rs`) e no runtime ESM JavaScript (`crates/aipo-js/runtime/aipo-runtime.js`).
    - Testado com asserções estritas de precisão em `crates/aipo-stdlib/tests/stdlib_tests.rs`.
  - **Construtor de Conjunto Vazio `Set()` Sem Argumentos (`aipo-stdlib`, `aipo-vm`, `aipo-js`)**:
    - Habilitada a invocação de `Set()` sem argumentos para criação direta de conjuntos vazios na VM (`crates/aipo-stdlib/src/convert.rs`, `crates/aipo-vm/src/convert.rs`) e no runtime JS (`crates/aipo-js/runtime/aipo-runtime.js`).
    - Adicionado teste unitário `test_empty_set_creation` e validada execução pelo CLI `aipo-sh -c`.
  - **Sincronização do Cabeçalho C ABI (`crates/aipo-c-abi/include/aipo.h`)**:
    - Adicionados os protótipos de embedding e controle da Register VM em C: `aipo_reg_vm_t`, `aipo_reg_vm_create`, `aipo_reg_vm_destroy`, `aipo_reg_vm_set_reg_int`, `aipo_reg_vm_get_reg_int`, `aipo_reg_vm_run`.
    - 100% dos testes da C ABI aprovados, incluindo harness C nativo.
  - **Suíte de Testes Automatizada para `aipo-egui` (`packages/aipo-egui`)**:
    - Criado arquivo de testes `packages/aipo-egui/tests/egui_test.aipo` cobrindo ciclo de vida de contexto, renderização de frames imediatos, layout de janelas e consulta de estado de input.
    - Integrado `aipo-egui` à suíte de verificação contínua de pacotes em `crates/aipo-cli/tests/package_suites.rs`.
  - **Sincronização e Harmonização da Documentação**:
    - Atualizado `SYNTAX.md` formalizando a remoção definitiva das construções legadas (`end`, `div`, `self!`) e a integração de `Tipo:nome`, `Tipo::[...]`, `#!satisfies`, `#!test` e `enum`.
    - Documentadas as opções `--engine=<vm|reg>`, `--budget=<N>` e `--max-instructions=<N>` em `docs/reference/cli.md`.
    - Sincronizado o nome do diagnóstico de reatribuição de campos imutáveis (`AIPO_SEM_FIXED_REASSIGN`) em todos os manuais técnicos e decisões arquiteturais.

  - **AIPO-OMNI-001 (Canários de Vazamento de Memória / `CountingAllocator`)**:
    - Implementado `CountingAllocator` em `crates/aipo-testkit` monitorando alocações atômicas em nível global.
    - Suíte com 8 canários de churn (`leak_canaries.rs`) cobrindo Stack VM, closures e upvalues, RegVm, frontend/HIR/sema, compilador de bytecode, compilador Wasm, `HandleTable` geracional e embedding scoped, certificando $\Delta \text{bytes} = 0$.
  - **AIPO-OMNI-002 (Instruction Budget Ininterceptável e Sticky)**:
    - Endurecida a proteção contra negação de serviço e loops infinitos (`while true { }`): falhas de esgotamento de quota de instruções (`VmFault::BudgetExhausted`) são ininterceptáveis pelo construto de linguagem `attempt { ... }`.
    - Garantido desbobinamento incondicional de pilha e propagação de estado sticky em `Vm`, `RegVm`, runtime Wasm e CLI (`--budget`, `--max-instructions`).
  - **AIPO-OMNI-003 (API de Embedding Rust Scoped / `AipoEngine::scope`)**:
    - Implementada fachada de embedding ergonômica e segura em Rust puro (`AipoEngine::scope`) com suporte a handles temporários `ScopedHandle<'scope>` e injeção de userdata em `HandleTable` geracional.
    - Prevenção garantida de escape de referências (`AIPO_RT_SCOPE_ESCAPE`), invalidação automática de handles no descarte do escopo e zero código `unsafe`.
  - **AIPO-OMNI-004 (Tarifação Periódica de Fuel no Despacho / Interval Metering)**:
    - Amortizado o custo de verificação de limites de instrução no loop principal de despacho da Stack VM e da `RegVm`.
    - O consumo de fuel é debitado deterministicamente, mas a verificação de estouro ocorre a cada `DEFAULT_METER_INTERVAL = 64` instruções (configurável via `set_meter_interval` e no `AipoEngineBuilder`), eliminando 98.4% das checagens de branch no caminho quente, com reconciliação estrita na saída de módulos e em opcodes de retorno.
  - **AIPO-OMNI-005 (Janelas de Registrador na `RegVm` / Register Windowing)**:
    - Eliminada a cópia cega de 256 `Value`s (4 KiB) e alocações de `Value::None` por ativação de função em `RegVm::invoke_with_upvalues`.
    - Implementada pilha contígua `register_stack: Vec<Value>` onde apenas os registradores ativos do chamador (`0..callee_regs`) são salvos e restaurados cirurgicamente, preservando isolamento em chamadas recursivas e reduzindo o tráfego de memória de pilha em até 98.8% sem qualquer `unsafe`.

- **Referência canônica de sintaxe (`SYNTAX.md`)**:
  - Novo documento normativo na raiz extraído do compilador (`aipo-lexer`, `aipo-syntax`, `aipo-ast`), não da prosa: 46 keywords e 36 operadores, as duas terminações de bloco (`end` e `{ }`), e os limites de aninhamento do ADP-005.
  - Fecha lacunas do manual, que omitia `fixed`, `end`, `satisfy`, `div` e `do`, nunca declarava `#` como único comentário e não alertava que `//` é divisão inteira.

- **Reparação de `packages/aipo.ui` (148 erros de parse → 0)**:
  - Nenhum gate compilava este pacote, e o drift acumulou-se nos 7 módulos: `scripts/verify.sh` só reloca os lockfiles e as suítes Rust que executam fontes `.aipo` cobrem apenas `aipo-zoe` e `aipo-game`.
  - Correções de gramática: `import.types` → `import types`; `- >` → `->`; `//` como comentário → `#`; campos de struct sem anotação de tipo; tipos sem qualificação de módulo; `if cond { a } else { b }` como expressão → forma `then`; `List.pop()` inexistente → `remove_last()`; `props.get(k, default)` (aridade inexistente) → helper `prop_of`; `init` (palavra reservada) → `initial`; contratos `Fn`/`Any` → `Fn?`/`Any?`.
  - As quatro tags de layout viraram structs com token canônico e um construtor `make_*` por variante, já que `enum` é pós-V1.
  - `render_frame` passou a atribuir o retorno de `view` a `root_node`, que nada atribuía antes: a passagem do descritor recebia `none`.
  - Fachada reescrita no padrão de `aipo-zoe`/`aipo-html` (submódulo exporta `make_*`, fachada define o nome público), porque `export` não aceita nome qualificado e `import` não cria namespace.

- **Nova guarda de suítes de pacote (`crates/aipo-cli/tests/package_suites.rs`)**:
  - Executa `aipo test` para todo pacote sem host (`aipo.ui`, `aipo.html`, `aipo.http`) e exige `aipo fmt --check` limpo em `aipo.ui`. Validado que falha quando há drift injetado.
  - `packages/aipo-ui/tests/ui_test.aipo`: 9 testes (tags, cores, árvore, semântica de props, primitivas, mount MVU).

- **Defeitos de linguagem registrados, com reprodução mínima (`docs/journal/2026-10-05-syntax-drift.md`)**:
  - **D1**: argumento nomeado combinado com bloco trailer não preenche defaults — `f8(a = 1) do { }` falha com `AIPO_RT_TYPE_MISMATCH`, enquanto 0 nomeados ou todos os parâmetros funcionam.
  - **D2**: closure invocado que, dentro dele, cria outro closure capturando upvalue causa `operand stack underflow`.
  - **D3**: `aipo check` não valida existência de membro; `l.pop()` passa no check e falha só em runtime.
  - **D4**: aspas aninhadas quebram interpolação f-string.

- **Fase 1 — Remediação dos Defeitos Críticos P0 da C ABI, ABI 0.2.0 e Harness Nativo em C com ASan (`aipo-c-abi`)**:
  - **Remediação dos 7 defeitos de fronteira P0 identificados no Dossiê Técnico**:
    - **C1 (Ownership e ciclo de vida de `Bytes`)**: valores `Bytes` retornados pela fronteira não mais emprestam buffers do `RefCell<Vec<u8>>` interno do guest (eliminando ponteiro pendente e use-after-free). Payloads heap agora são copiados para snapshots com sentinela explícita, gerenciados pelo runtime e liberados deterministicamente pelo host via `aipo_value_release`.
    - **C2 (Despacho estrito de callbacks nativos)**: removido o fallback por aridade (`HashMap::find(|cb| cb.arity == args.len())`). O despacho exige identificação unívoca pelo nome do callee nativo (`Value::Native`), falhando fechado (`VmFault::NotCallable`) em caso de não correspondência.
    - **C3 (Validação canônica de invariantes C)**: valores recebidos pela ABI passam por checagem estrita (`MIN_SAFE_INT..=MAX_SAFE_INT`, `float.is_finite()`, fatias `(str_ptr, str_len)` e normalização Unicode NFC com `unicode-normalization`), impedindo violações de invariantes da VM.
    - **C4 (Criação segura de handles com `aipo_handle_create_checked`)**: nova API que valida o valor e retorna erro explícito (`AIPO_ERR_USAGE`) em caso de falha de conversão, sem emitir handles corrompidos para `none`. `aipo_handle_create` legado mantido como wrapper para compatibilidade.
    - **C5 (Bloqueio estrito de reentrância e aliasing FFI)**: implementado `CallbackDepthGuard` e `in_host_callback()`. Tentativas de invocar `aipo_runtime_call`, `load_module`, `grant_capability`, `revoke_capability`, `register_host_fn` ou `destroy` durante a execução de um callback de host são interceptadas e recusadas com `AIPO_ERR_USAGE` (ou safe no-op para destroy), evitando aliasing mutável (`&mut`) do runtime. Escopo de empréstimo de `rt` no dispatcher delimitado antes da chamada do callback.
    - **C6 (Rejeição de ponteiros nulos com `argc > 0`)**: chamadas com `argc > 0` e `args == NULL` são rejeitadas explicitamente com `AIPO_ERR_NULL_POINTER` em vez de serem interpretadas silenciosamente como zero argumentos.
    - **C7 (Integridade de strings com NUL embutido)**: strings com NUL interno preservam seu comprimento exato `str_len` e bytes reais, eliminando discrepâncias de tamanho e leituras out-of-bounds.
  - **Sincronização canônica do cabeçalho `include/aipo.h`**:
    - Versão da ABI atualizada para `0.2.0` (`AIPO_VERSION_MINOR 2`).
    - Declaradas as novas funções `aipo_handle_create_checked` e `aipo_value_release`.
    - Atualizada a documentação de posse de memória e liberação de snapshots.
  - **Suíte de testes em Rust e Harness Nativo em C**:
    - `crates/aipo-c-abi/tests/c_abi_p0_tests.rs`: 7 testes de reprodução e regressão cobrindo exaustivamente C1–C7 e rejeição de reentrância em todos os pontos de mutação.
    - `crates/aipo-c-abi/tests/host/main.c` + `tests/c_host_tests.rs`: executável C puro compilado com `clang`/`gcc` com flags `-Wall -Wextra -Werror` e executado tanto nativamente quanto sob **AddressSanitizer e UndefinedBehaviorSanitizer** (`-fsanitize=address,undefined`), garantindo ausência de corrupção de memória e conformidade total com o cabeçalho.

- **Fase 2 — Integridade de Módulos, Isolamento de Funções, Carregamento Atômico e Orçamento de Execução (M1, M2, S1 / P1)**:
  - **M1 / P1 (Isolamento estrito de funções por módulo e zero-clone em chamadas)**:
    - Funções são resolvidas estritamente dentro da tabela de funções do módulo solicitado (`module.functions`), eliminando o sombreamento e a colisão de nomes através de `vm.globals`.
    - Módulos carregados no `AipoRuntime` são armazenados como `Arc<BytecodeModule>`, eliminando a clonagem de bytecode por chamada FFI (`aipo_runtime_call`).
    - Preparação de caches de execução isolada na VM (`push_module_execution` / `pop_module_execution`): constantes, slots globais e cache de campos são vinculados dinamicamente ao módulo em execução durante `invoke`, restaurando o contexto anterior de forma segura e sem blocos `unsafe`.
    - A superfície de semântica (`PreludeSurface`) ao compilar novos módulos expõe apenas funções de host e nomes de módulos, sem vazar símbolos internos de módulos anteriores.
  - **M2 (Carregamento atômico em dois estágios com rollback de definições)**:
    - Implementado `snapshot_definitions()` e `restore_definitions()` na `Vm`: captura o estado de definições de structs, métodos, invariantes, globais e constantes antes da avaliação do código de topo de nível do módulo.
    - Em caso de falha de runtime durante a inicialização (ex: divisão por zero no topo do script), o snapshot é restaurado deterministicamente, descartando definições órfãs e mantendo o runtime limpo.
  - **S1 (Orçamento de execução determinístico e contadores de instruções)**:
    - Adicionado suporte a cota de instruções na `Vm` (`set_max_instructions`, `instruction_count`, `reset_instruction_count`). A VM debita cada instrução no despacho e interrompe loops infinitos imediatamente com `VmFault::Overflow`.
    - Exportadas funções C ABI canônicas em `include/aipo.h`: `aipo_runtime_set_instruction_budget`, `aipo_runtime_instruction_count`, `aipo_runtime_reset_instruction_count`.
  - **Testes e Verificação**:
    - `crates/aipo-vm/tests/vm_core.rs`: testes unitários `test_instruction_budget_halts_infinite_loop` e `test_vm_definition_snapshot_and_rollback`.
    - `crates/aipo-c-abi/tests/c_abi_p1_tests.rs`: testes de integração multi-módulo cobrindo isolamento (M1), rollback atômico (M2) e cotas de execução (S1).
    - `crates/aipo-c-abi/tests/host/main.c`: harness C nativo estendido com testes M1, M2 e S1, compilado e executado sob AddressSanitizer/UBSan com 100% de aprovação.

- **M15-A — Culling de Quads, Métricas de Draw Call e o Anel de Foco que o M14 Não Entregava (`aipo.zoe` / `aipo-game-host`)**:
  - **Anel de foco desenhado dentro do fragment shader (corrige o defeito de acessibilidade deixado pelo M14)**:
    - O M14 trocou o anel externo de `renderer.aipo` por duas props (`focus_ring_width`, `focus_ring_color`) passadas aos natives SDF, mas **o shader nunca leu esses valores**: os uniforms não existiam, então um widget focado ficou **sem nenhuma indicação visual de foco** — regressão de WCAG 2.4.13 (`Focus Appearance`) introduzida silenciosamente naquele commit.
    - Agora o shader declara `u_focus_width` e `u_focus_color` e compõe o anel na **mesma passada** do fill, da borda e da sombra, com o ramo inteiro saltado quando `u_focus_width <= 0.0` — ou seja, um widget sem foco não paga nada.
    - O anel tem **SDF próprio** (`abs(d - ring_mid)`), e não a distância da borda: um SDF compartilhado faria o anel coincidir com a borda e ler como uma borda mais grossa, e não como indicador de foco. Ele fica deslocado `FOCUS_GAP` (1.5px) para fora, para sobreviver ao fundo de qualquer superfície — claro ou escuro.
  - **Culling de quads que de fato não pintam (`sdf_cull_reason`)**:
    - Três motivos de descarte, decididos **antes** de qualquer chamada GL: `OffScreen` (fora do retângulo de clip ativo), `ZeroArea` (filho flex colapsado, divisor de altura zero) e `Invisible` (sem fill, sem borda, sem sombra e sem anel — o fragment shader descartaria cada pixel depois de avaliar o SDF inteiro e as duas camadas de sombra).
    - O predicado é uma **função pura**, portanto testável sem contexto GL: 7 testes exercitam cada motivo de descarte e, sobretudo, os casos que **não** podem ser cortados (preenchimento transparente com sombra visível, quad só-borda, anel de foco sobre superfície transparente) — um culling agressivo demais apaga sombras e indicadores de foco sem quebrar nenhum teste de fumaça.
    - O teste de padding com clip usa `padding()` real em vez de uma constante inventada — a versão anterior assumia 10px de padding para `shadow-xl`, que na verdade tem 48px de blur.
  - **Métricas de draw call observáveis (`sdf_draws_issued`, `sdf_draws_culled`)**:
    - Contadores atômicos públicos, antes inexistentes. Sem eles o objetivo do M15 (≤ 8 draw calls por quadro) não tinha como ser medido nem verificado.
  - **Elisão de uploads de uniform redundante (`host_bridge.rs`)**:
    - `SdfUniformState` lembra os últimos 11 valores por pipeline e só reenvia o que mudou. Uma tela cheia de linhas com o mesmo estilo reenviava nove uniforms por linha mesmo quando nada diferia.
    - O estado é **invalidado em `init_zoe_shaders()`**, porque um contexto GL novo não herda os valores antigos. Os campos começam em `f32::NAN`, que nunca compara igual, então o primeiro quad após um reset sempre envia tudo — não há janela em que um uniform obsoleto seja reutilizado.
  - **Paridade Rust↔GLSL do `FOCUS_GAP_PX`**: o shader é compilado de uma string bruta e não pode referenciar constantes Rust. O teste `test_sdf_focus_gap_parity` lê o **fonte do shader** e compara o literal, de modo que mexer num lado sem o outro falha o build.
  - **Correção no `padding()`**: o anel alcança `focus_width + FOCUS_GAP` para fora do box, mas o padding somava apenas `focus_width` — o anel externo era cortado em 1.5px. O cálculo agora inclui o gap e é **público**, porque três pontos precisam concordar: o caminho de desenho, o predicado de culling e os testes.
  - **`sdf_cull_reason` não aceita mais dimensões já com padding do chamador**: o predicado deriva o retângulo crescido de `SdfQuad::padding()`. Antes ele recebia `quad_w`/`quad_h` prontos **e** recalculava o padding por dentro, então um chamador podia testar o clip contra um retângulo diferente do que era de fato rasterizado.
  - **13 testes novos, 42 no total** (`cargo test -p aipo-game-host`), todos verdes sem GPU; `cargo fmt --check`, `clippy -D warnings` e `npm run docs:build` verdes.
  - **M15-B (instancing por material) — viabilidade verificada, implementação pendente**: a análise da wave anterior concluiu que macroquad bloqueia instancing porque `Context::camera_matrix` é privado. **Isso está incorreto e foi corrigido aqui**: `get_internal_gl()` é público e expõe `quad_gl` + `quad_context` junto de um `flush()` documentado para "combinar o desenho do macroquad com chamadas miniquad/OpenGL cruas"; `QuadGl::get_projection_matrix()` devolve a projeção corrente **incluindo a câmera**; e `miniquad::VertexStep::PerInstance` existe no backend GL. O caminho para o F1 integral está aberto por design, não bloqueado.
    - **Mas há uma barreira real, e ela é de postura de segurança, não de API**: o crate declara `#![forbid(unsafe_code)]` em `lib.rs` **e** em `main.rs`, e `get_internal_gl()` é `unsafe`. `forbid` não pode ser afrouxado por `#[allow]` em nenhum módulo filho, então o pipeline instanciado exige trocar para `#![deny(unsafe_code)]` com **um único** `#[allow(unsafe_code)]` em módulo isolado e auditado — ou mover o renderizador para um crate próprio. Isso é decisão explícita, não detalhe de implementação, e deve ser registrada como ADP antes do código.
    - O restante do trabalho é implementar o pipeline instanciado (VBO de instâncias, layout de 9 × vec4 espelhado no shader, descarga por lote) e — decisivo para o risco — **verificá-lo**, o que exige rodar a GUI numa máquina com display, algo que o gate headless não cobre. Até então, o caminho imediato (1 draw call por quad) permanece, agora com elisão de uniform e culling mensuráveis.

- **M14 — Shader SDF v2: Antialiasing Escalado, Sombra Real e Anel de Foco Composto (`aipo.zoe`)**:
  - **Quatro defeitos matemáticos corrigidos no fragment shader analítico**:
    - **(1) Antialiasing com banda fixa de 1px**: `clamp(0.5 - dist, 0, 1)` só é correto em escala 1:1. Sob zoom de câmera de viewport a distância-por-pixel muda e a borda fica ora grossa demais ora fina demais. Corrigido com o uniform `u_pixel_scale`, derivado de `miniquad::window::dpi_scale()`, para que a banda continue com um pixel físico em qualquer HiDPI e qualquer zoom.
      - **Decisão de implementação**: usar uniform derivado do DPI em vez de `fwidth`/`dFdx`. Manter o shader em GLSL ES 100 dispensa a extensão `GL_OES_standard_derivatives` e assim funciona em **todos** os backends já suportados pelo host, incluindo WebGL1. Derivados também seriam desnecessários aqui: estes quads são rasterizados geometricamente completos e todo o antialiasing é analítico no fragment shader, então MSAA de hardware não acrescentaria nada.
    - **(2) Borda composta por `mix` sobre SDF único**: `b_dist = dist + border_width` destrói o termo `min(max(q.x,q.y),0.0)`, fazendo o raio interno sair errado, e `mix` não é um `over` correto quando o fill é translúcido. Agora: dois SDFs independentes (`abs(d) - w*0.5`, que centra o traço no limite do box) e saída com alfa pré-multiplicado.
    - **(3) Ausência total de sombra**: sobreposições simulavam profundidade com um retângulo preto deslocado, produzindo uma borda dura. Agora: duas camadas de sombra suave compostas na ordem `over` do CSS, com **compensação de gamma de 2.2** — sem ela um alfa de 0.05 aterrissa a cerca de metade da escuridão pretendida em superfícies claras.
    - **(4) Raio único de canto**: `sd_rounded_box` agora recebe raios por canto (`tl, tr, br, bl`), permitindo formas assimétricas.
  - **Modelo de duas camadas de sombra**: um único blur lê como borrão; o que lê como profundidade é uma camada de *contato* estreita e densa sob uma camada *ambiente* ampla e leve. O nível de elevação (0..5, contínuo) interpola entre âncoras discretas (xs/sm/md/lg/xl), então elevar é animável sem "pop".
  - **Anel de foco: props introduzidas, desenho adiado**: `renderer.aipo` emitia um segundo `host_draw_rect_lines` externo com uma única cor, que falha WCAG 2.4.13 tanto em superfícies claras quanto escuras. A intenção era compor o anel na mesma passada do fill, da borda e da sombra; as props `focus_ring_width` e `focus_ring_color` e o encaminhamento aos natives entraram nesta fase, **mas o ramo no fragment shader ficou para o M15** — durante o M14 um widget focado não desenhava anel nenhum. Ver a entrada **M15-A**, que concluiu a parte gráfica.
  - **Novos host natives**: `host_draw_sdf_rect_v2` (21 args, elevação + anel de foco) e `host_draw_sdf_rect_corners` (24 args, raios por canto). `host_draw_sdf_rect` de 15 args é preservado para compatibilidade.
  - **Padding do quad**: o quad rasterizado cresce simetricamente em torno do centro do box para acomodar o blur da sombra e o anel de foco externo, evitando corte. O shader trabalha em coordenadas locais ao box, então a origem do quad cancela.
  - **Tokens de elevação** (`tokens.aipo`): `elevation_none` a `elevation_max`. `card` recebe `elevation_sm` por padrão e `button` repassa a prop `elevation`, então a sombra nova é visível no catálogo.
  - **Fallback headless preservado**: sem contexto GL, o caminho de software desenha sombra, preenchimento, borda e anel de foco — os 29 testes continuam rodando sem GPU.
  - **Testes de paridade Rust↔GLSL (16 testes novos em `bridge_tests.rs`)**: o teste lê o **fonte do shader** via `include_str!` e falha se a banda fixa de 1px, o `dist + border_width`, a ausência de compensação de gamma ou o raio escalar reaparecerem. Testes de invariante cobrem monotonicidade da tabela de âncoras, interpolação estrita entre âncoras, continuidade na resolução de 1/16 de nível, extrapolação além de `xl` e a relação de ordem entre as camadas de contato e ambiente. **Um desses testes pegou um bug real na primeira implementação** — a camada de contato usava a âncora bruta em vez do valor interpolado, e a extrapolação nunca crescia.

- **M13 — Fundamentos Numéricos, Tipografia Real e Correções de API do `aipo.zoe`**:
  - **Trigonometria correta no subsistema 3D (`retro3d.aipo`)**:
    - Removidas as aproximações polinomiais locais de `sin`, `cos` e `sqrt` (série de Taylor de 3 termos com `cos` derivado de `sin(x + pi/2)`).
    - O módulo passou a delegar a `math.sin` / `math.cos` / `math.sqrt` da biblioteca padrão, que operam em `f64`.
    - **Correção de bug quantificada**: a aproximação anterior atingia **4,22% de erro relativo em `cos(0.6)`** — exatamente o `yaw` padrão de `camera_3d(yaw = 0.6, ...)` — corrompendo de forma visível a projeção de profundidade do viewport 3D. O erro agora é da ordem do épsilon de ponto flutuante.
    - Teste de regressão `M13: retro3d trigonometry matches the standard library` valida que um ponto no alvo da órbita projeta no centro do viewport.
  - **Peso de fonte real (`font_weight`)**:
    - `font_weight` era aceito como propriedade e **nunca lido**: todo `"font_weight": "bold"` no código era um no-op e a hierarquia tipográfica não existia.
    - `host_draw_text` e `host_measure_text` agora aceitam um peso CSS (100–900) e aplicam engrossamento sintético por passagens offset nos pontos cardeais.
    - O bundled Inter é uma fonte variável confirmada (`fvar` + `gvar` presentes), porém o pipeline de texto do macroquad expõe apenas a instância estática; o engrossamento sintético é a via correta dentro dessa restrição.
    - **Medição e renderização agora concordam**: Leona mede com o mesmo peso que o renderizador pinta.
  - **Novo módulo `text.aipo` — resolução tipográfica centralizada**:
    - Ponto único de resolução de peso, tamanho, `line_height`, `letter_spacing`, alinhamento horizontal/vertical, corte com reticências e quebra de linha.
    - `text_ellipsize` faz busca binária sobre o número de grafemas (O(log n) medições em vez de O(n)).
    - `text_wrap` quebra em espaços quando possível e apenas no meio da palavra para um token indivisível, com teto de `max_lines` e reticência na última linha preservada.
  - **Medição de cor em hexadecimal completa (`color.aipo`)**:
    - `hex()` antes reconhecia **apenas** `#ffffff` e `#000000`; qualquer outro valor retornava cinza opaco `(100,100,100)`. Agora aceita `#RGB`, `#RRGGBB`, `#RRGGBBAA`, com ou sem `#`, maiúsculas ou minúsculas.
    - Entrada inválida retorna transparente em vez de uma cor arbitrária, para que um erro de digitação nunca seja renderizado como cor plausível.
    - Novo `to_hex` / `color_to_hex` para serialização, com round-trip verificado em teste.
    - Nota de implementação: `Int("a")` em Aipo faz parse **decimal apenas**, então os dígitos hexadecimais usam uma varredura posicional sobre o alfabeto — sem custo de conversão, sem dependência de locale e sem modo de falha.
  - **Tabela de strings em runtime (`strings.aipo`)**:
    - Elimina os rótulos `"Confirmar"` / `"Cancelar"` hardcoded num framework de alcance global.
    - English é o padrão; `zoe.set_strings({...})` sobrescreve parcialmente com segurança e `zoe.reset_strings()` restaura.
  - **Distribuição flex ponderada (`leona.aipo`)**:
    - `spacer(n)` honrosa o argumento: `spacer(3.0)` ocupa três vezes o espaço restante de `spacer(1.0)`. Antes o parâmetro era completamente ignorado.
    - Leona agora soma os pesos `flex` e divide proporcionalmente; `justify_content` centraliza contra o bloco de conteúdo real, e não contra o maior filho flex.
    - `spacer()` sem argumento mantém a grafia `"flex"` para não quebrar árvores e testes existentes.
  - **Correção do caminho intrínseco de layout (`leona.aipo`)**:
    - Um `width: "auto"` explícito agora resolve para o tamanho intrínseco medido. Antes, tanto `"auto"` quanto uma propriedade ausente caíam no espaço disponível, e um `zoe.label(..., {"width": "auto"})` na raiz era esticado pela viewport inteira, descartando a largura medida.
  - **Rolagem com clamp e polegar proporcional (`components.aipo`)**:
    - Clamp superior aplicado a partir da segunda passagem de layout, quando o transbordo já foi medido por Leona e republicado via `Element.max_scroll`.
    - O offset inicial de `scroll_y` passa intocado na primeira passagem, para que restaurar uma posição de rolagem salva continue possível.
    - O polegar reflete a fração visível do conteúdo e é posicionado pelo deslocamento, em vez de ser um bloco fixo de 40px preso ao topo.
    - Arrastar o polegar move o conteúdo proporcionalmente ao span, e não por um multiplicador fixo.
  - **Slider arrastável (`make_slider`)**:
    - O slider expunha apenas `on_click`, então o valor **não podia ser alterado arrastando**. Agora suporta pressionar na trilha (salto para a posição) e pressionar-e-arrastar (seguimento contínuo), com `step` opcional, `on_drag_end` para push de entrada de undo e trilho/polegar estilizados separadamente.
  - **Cache de ícones com LRU real (`host_bridge.rs`)**:
    - O README prometia cache LRU; o código usava `HashMap` sem eviction, e a chave inclui RGBA — então trocar de tema enumerava uma nova chave por ícone e a memória de GPU crescia sem limite.
    - Implementado LRU com `VecDeque` de 512 entradas, mantendo a superfície de dependências em zero. `icon_cache_len()` expõe a contagem para teste.
  - **Telemetria real no editor (`examples/editor.aipo`)**:
    - `"FPS: 60"` e `"Leona Layout: OK"` eram strings hardcoded. Agora derivam de `host_frame_time()`, com cor por faixa de FPS, tempo de quadro em ms e contagem de objetos.
    - "Delete Entity" estava atrás de um closure vazio (`fn(_) {}`); agora limpa a seleção.
  - **Suíte de regressão M13 (13 testes em `tests/zoe_test.aipo`)**: cada teste trava um defeito corrigido e falha contra a implementação pré-M13.
  - **Portão de verificação (`scripts/verify.sh`)**: re-bloqueia os pacotes Aipo locais (o digest cobre fontes, então qualquer edição invalida o lockfile) e executa fmt, clippy, testes e build da documentação. `aipo-c-abi` fica fora do escopo por padrão por trabalho em andamento já presente na árvore; use `--all` quando esse trabalho for integrado.

- **Site Documental Dedicado do Zoe UI & Playground Interativo (M12)**:
  - **Hub Documental no VitePress (`docs/zoe/` e `docs/en/zoe/`)**:
    - Estruturação completa de 11 páginas documentais especializadas em Português e Inglês: Landing Page com arquitetura gráfica, Guias de Primeiros Passos, Motor Leona 2.0, Reatividade & Sinais, Criação de Componentes Customizados, Catálogo detalhado (Botões, Inputs/Scrubbers, Layout, Navegação, Widgets Avançados) e Playground interativo.
    - Integração de navegação dedicada e sidebars estruturadas no `docs/.vitepress/config.mts`, com compilação e verificação de integridade 100% verde (`npm run docs:build`).
  - **Playground com Pré-Visualização e Código em Aipo**:
    - Seção com exemplos ao vivo unindo editores de código, split views e renderização em GPU.

- **Catálogo de Componentes Avançados em `aipo.zoe` (M11 - Advanced Widgets)**:
  - **Módulo de Widgets Especializados (`packages/aipo-zoe/src/widgets.aipo`)**:
    - **`code_editor`**: Editor de código virtualizado com gutter e numeração dinâmica de linhas, realce sintático léxico completo para Aipo (`fn`, `let`, `var`, `if`, strings, comentários, números), cursor piscante com tween, destaque de linha ativa e descarte vertical de linhas invisíveis (culling).
    - **`node_graph`**: Canvas infinito para grafos visuais e shaders lógicos com grade de fundo, cartões de nós com portas de soquete coloridas por tipo semântico (float, vec, color, tex) e cabos suaves renderizados via nova primitiva `host_draw_bezier` com brilho (*glow*) e anti-aliasing contínuo.
    - **`modal_dialog` & `open_modal`**: Diálogos modais flutuantes com escurecimento de fundo (*scrim backdrop*), botão de descarte e botões de ação estilizados.
  - **Primitiva Nativa Bézier em GPU (`host_bridge.rs`)**:
    - Exposição de `host_draw_bezier` (13 argumentos) e registro na tabela `NATIVES` e prelúdio para renderização de curvas cúbicas em GPU com 24 segmentos e recorte por hardware.
  - **Re-exportações em `lib.aipo`**: `code_editor`, `node_graph`, `modal_dialog`, `open_modal`.

- **Leona 2.0, Alinhamento por Baseline Tipográfica & Otimização Estrutural de Stack (M10)**:
  - **Alinhamento na Linha de Base Tipográfica (`align_items: "baseline"`)**:
    - Suporte a alinhamento horizontal orientado à tipografia usando `host_font_metrics["ascent"]`, eliminando flutuações e desalinhamentos entre ícones e rótulos de texto de diferentes tamanhos.
  - **Otimização Estrutural de Stack Frames no Leona (`leona.aipo`)**:
    - Decomposição das passadas de medição e flex em funções modulares auxiliares (`distribute_flex_and_measure`, `get_child_ascent`, `compute_max_ascent`).
    - Redução do frame de execução de `layout_node` de 70 slots para ~18 slots por nível na pilha do interpretador, permitindo a travessia de árvores arbitrariamente profundas (30+ níveis) com zero risco de transbordamento de operand stack (`AIPO_RT_OVERFLOW`).

- **Shaders Analíticos de UI por GPU em `aipo.zoe` (M9 - GPU SDF)**:
  - **Pipeline de Fragment Shader SDF (`host_bridge.rs`)**:
    - Novo pipeline de renderização acelerada por GPU utilizando *Signed Distance Fields* analíticos (`sd_rounded_box`), eliminando a decomposição em polígonos aproximados da CPU.
    - Suavização contínua de bordas (*anti-aliasing* subpixel) calculada via função de distância analítica direta no pixel shader.
    - Suporte a *Top Inner Highlight* de 1px simulando reflexão de luz física ambiente no chanfro superior de botões, cartões e abas.
    - Borda interna precisa calculada continuamente ao longo da curvatura de `border_radius` sem emendas ou bicos.
    - Nova primitiva nativa `host_draw_sdf_rect` unificando fundo, curvatura, borda e luz interna em uma única passada de draw call na GPU.
  - **Integração com o Renderizador (`renderer.aipo`)**:
    - Todos os nós com fundo ou borda agora emitem quads analíticos para a GPU, com fallback automático e seguro para testes headless.
  - **Testes**: 13/13 aprovados (`cargo test -p aipo-game-host`).

- **Motor Tipográfico Subpixel com Fonte Inter em `aipo.zoe` (M8)**:
  - **Fonte Inter Embutida (`crates/aipo-game-host/assets/fonts/Inter.ttf`)**:
    - Substituição da fonte padrão ProggyClean (bitmap pixelada) pela Inter Variable (860KB), uma fonte profissional otimizada para UI com hinting subpixel, carregada via `set_default_font` do macroquad no início de cada sessão.
  - **Medição de Texto por Hardware (`host_measure_text`)**:
    - Nova primitiva nativa que retorna a largura exata em pixels de qualquer texto para um dado `font_size`, usando `fontdue` diretamente (CPU-side, headless-safe) com suporte a kerning horizontal.
    - Substitui a aproximação grosseira `text.len() * font_size * 0.55` em todos os pontos de `leona.aipo`.
  - **Métricas Tipográficas Reais (`host_font_metrics`)**:
    - Nova primitiva nativa retornando `{ ascent, descent, line_gap, line_height }` para posicionamento de texto por baseline.
    - `renderer.aipo` agora usa alinhamento por baseline real: `text_y = box_top + (box_h - (ascent - descent)) / 2 + ascent`, eliminando o fator fudge `font_size * 0.2`.
  - **Layout Leona Pixel-Perfect**:
    - `measure_node_intrinsic` em `leona.aipo` usa `host_measure_text` e `host_font_metrics["line_height"]` para dimensionamento intrínseco de labels, buttons e badges.
    - Tooltips em `renderer.aipo` também atualizados para medição real.
  - **Recorte por Hardware e Geometria Estabilizada em `host_bridge` e `leona`**:
    - **Recorte de Linhas Cohen-Sutherland (`safe_draw_line`)**: Algoritmo exato de clipping de segmentos contra o retângulo ativo (`CLIP_STACK`), eliminando o vazamento de linhas da grade 3D sobre o painel lateral de hierarquia.
    - **Primitivas de Cantos Arredondados (`host_draw_round_rect`, `host_draw_round_rect_lines`)**: Suporte real a `border_radius` no renderizador para botões, cartões e abas com cantos suaves.
    - **Medição Intrínseca Hierárquica no Leona (`leona.aipo`)**: Resolução completa de `tab_item`, `segment_item`, `segmented_group`, `button` (composição ícone + texto) e `badge`, extinguindo sobreposições como `"ierarchAssets"` e o corte de texto em botões com ícones.
    - **Distribuição Flex Proporcional em Linhas**: Campos de coordenadas (X, Y, Z) agora dividem o container do Inspetor em 3 colunas simétricas de 33.3% (`width: "flex"`).
  - **Testes**: 13/13 aprovados (`cargo test -p aipo-game-host`).

- **Modernização de UI, Motor de Ícones Vetoriais e Tokens de Design em `aipo.zoe`**:
  - **Design Tokens Canônicos (`packages/aipo-zoe/src/tokens.aipo`)**:
    - Tipografia escalada (`10.0` a `18.0`), grade geométrica estrita de 4px/8px, raios de borda padronizados, alturas táteis de controle e paleta de superfícies Catppuccin Mocha (`bg_canvas`, `bg_panel`, `bg_surface`, `accent_x`, `accent_y`, `accent_z`).
  - **Motor de Ícones Vetoriais por Hardware (`crates/aipo-game-host`)**:
    - Integração de `tiny-skia` e `svgtypes` no host nativo com LRU GPU Cache (`Texture2D`), rasterizando caminhos SVG em resolução nítida no primeiro frame e desenhando quads acelerados a 60+ FPS nos frames seguintes.
    - Nova primitiva nativa `host_draw_icon_path` e módulo declarativo `icons.aipo` com `zoe.icon` e `zoe.register_icon`, incluindo suporte canônico a Lucide (`play`, `pause`, `rotate-ccw`, `grid`, `box`, `layers`, `trash`, `camera`, `sun`, `volume`, `sparkles`, etc.), Heroicons, Tabler, Material Symbols e Devicons (`rust`).
  - **Componentes Profissionais de Alta Precisão (`packages/aipo-zoe/src/components.aipo`)**:
    - `scrubber_input`: Controle numérico estilo Blender com arrasto contínuo, badges coloridos de eixos, precisão decimal configurável e suporte a modificadores de teclado (`Shift` para ajuste fino 0.1x, `Ctrl` para saltos 10x).
    - `segmented_group`: Controle em pílula (Linear/Raycast) com superfície ativa elevada, suporte a ícones vetoriais e rótulos tipográficos.
    - `hierarchy_tree`: Árvore de cena profissional (Godot/Blender) com seleção em largura total (Fitts's Law), guias verticais de indentação, carets rotativos e badges semânticos (`[3D]`, `[2D]`, `[CAM]`, `[SFX]`).
    - `button`: Suporte aprimorado a ícones embutidos (`icon`, `icon_size`).
  - **Evolução do Editor Visual de Game Engine (`packages/aipo-zoe/examples/editor.aipo`)**:
    - Adoção completa de `segmented_group` para alternância 2D/3D, `hierarchy_tree` na aba de hierarquia, `scrubber_input` para coordenadas X/Y/Z no Inspetor e ícones vetoriais em botões da barra de ferramentas e lista de assets.
  - **Suíte de Testes Automatizados**:
    - Novo teste de unidade `test_host_draw_icon_path` e validação multi-frame em `test_zoe_ui_editor_compilation_and_execution` e `test_zoe_ui_unit_test_suite` (13/13 testes aprovados).

- **Amadurecimento Completo de `aipo.html` (Web Declarativa, SSR e TEA/MVU)**:
  - **Evolução Sintática do Compilador (`crates/aipo-syntax/src/parser.rs`)**:
    - Suporte a identificadores contextuais para `div`, permitindo que `fn div`, `export div` e chamadas `div(...)` funcionem como expressões e declarações sem conflito com o operador legado de divisão inteira.
  - **Correção no Sistema de Módulos (`crates/aipo-cli/src/modules.rs`)**:
    - Renomeação consistente de identificadores privados em declarações de funções (`HirItem::Fn`), estruturas (`HirItem::Struct`), blocos de implementação (`HirItem::Impl`) e construtores (`HirExpr::Construct`), garantindo isolamento estrito de visibilidade em submódulos de pacotes.
  - **Serializador SSR Puro e Fragmentos (`packages/aipo-html/src/tags.aipo`)**:
    - `render_to_string(vnode)`: serialização pura de árvores virtuais para HTML5 canônico com escape preventivo de entidades (`&`, `<`, `>`, `"`) em nós de texto e atributos.
    - `fragment(body)`: suporte a fragmentos virtuais para renderizar múltiplos elementos irmãos sem contêineres adicionais no DOM ou HTML final.
    - `each_item(items, render_fn)`: iterador declarativo para renderização de coleções.
  - **CSS-in-Aipo Tipado com Media Queries (`packages/aipo-html/src/css.aipo`)**:
    - `css(rules)`: geração determinística de classes com hash (`aipo-s{id}`), suporte a pseudo-classes (`hover`, `active`, `focus`, `disabled`) e blocos responsivos `@media`.
    - `get_injected_css()` / `clear_css_registry()`: concatenação de folhas de estilo injetadas para embutimento no `<head>` em SSR.
  - **Runtime MVU/TEA Reativo com Comandos (`packages/aipo-html/src/mvu.aipo`)**:
    - Padrão The Elm Architecture (TEA) suportando tuplas `[model, cmd]` e despachos reativos sem necessidade de diffing da árvore inteira.
    - Construtores de comandos explícitos: `cmd_none()`, `cmd_msg(msg)` e `cmd_task(task_fn, on_success, on_error)`.
  - **Bateria de Testes Automatizados (`packages/aipo-html/tests/html_test.aipo`)**:
    - 6 testes cobrindo árvores aninhadas, tags folha e void, serialização SSR com escaping, fragmentos, regras de CSS com media queries e cadeia de comandos MVU.

- **Implementação do Framework HTTP e Roteador de Microsserviços (`packages/aipo-http`)**:
  - **Roteador Zero-Regex em Árvore de Segmentos (`packages/aipo-http/src/router.aipo`)**:
    - Implementação de árvore Radix de segmentos (`RouteNode` / `Router`) sem dependência de expressões regulares em runtime.
    - Suporte a rotas estáticas exatas, parâmetros dinâmicos (`:id`), múltiplos parâmetros aninhados e rotas coringa (*catch-all* `*filepath`).
  - **Engine de Contexto Tipado (`packages/aipo-http/src/context.aipo`)**:
    - Estrutura `Context` com métodos ergonômicos e orientados a objeto (`impl Context`): `status`, `set_header`, `header`, `param`, `query_param`, `json`, `text`, `html` e `body_json`.
    - Parsing automatizado de query strings (`?key=val&k2=v2`) e decodificação segura de corpo JSON via `json.parse`.
  - **Pipeline de Middlewares Estilo Cebola (`packages/aipo-http/src/middleware.aipo`)**:
    - Executor recursivo `run_middleware_chain` com encadeamento de `next()`, permitindo pré-processamento, pós-processamento e curto-circuito seguro (e.g. guardas de autenticação).
    - Middlewares canônicos embutidos:
      - `cors(options)`: headers de Cross-Origin Resource Sharing e resposta automática de preflight OPTIONS (204 No Content).
      - `logger(custom_logger)`: registro estruturado de método HTTP, caminho e status code da resposta.
      - `recover(on_error)`: captura de falhas de runtime e exceções não tratadas com resposta 500 JSON.
  - **Aplicação Central e Agrupamento de Rotas (`packages/aipo-http/src/app.aipo` & `lib.aipo`)**:
    - Registro de rotas por verbo HTTP (`get`, `post`, `put`, `delete`, `patch`).
    - Agrupamento hierárquico de rotas (`group`) com concatenação normalizada de caminhos (`combine_path`) e suporte a middlewares dedicados por grupo.
    - Dispatcher puro e desacoplado (`handle_request(req_dict) -> res_dict`), permitindo testes unitários ultrarrápidos e desacoplamento de sockets de rede do SO.
  - **Bateria de Testes Automatizados (`packages/aipo-http/tests/http_test.aipo`)**:
    - 6 testes cobrindo roteamento básico, respostas tipadas, extração de parâmetros e wildcards, query strings e body JSON, esteira de middlewares e auth guard, middlewares embutidos (cors, recover) e agrupamento de rotas com 404.
  - **Documentação Canônica Bilíngue (`docs/packages/aipo-http.md`, `docs/en/packages/aipo-http.md`)**:
    - Guias detalhados com exemplos práticos de APIs REST, middlewares e testes desacoplados.
  - **Máquina de Estados e Animação de Spritesheets (`packages/aipo-game/src/animation.aipo`)**:
    - Estruturas de dados canônicas `AnimationClip` e `SpriteAnimator` para controle quadro-a-quadro de spritesheets 2D.
    - Suporte a clipes cíclicos (`is_looping == true`) e de disparo único (*one-shot* com travamento no último quadro e sinalizador `is_finished`).
    - Recorte UV automatizado de células de spritesheet com mapeamento bidimensional de linha/coluna e renderização acelerada por GPU (`draw_animator`).
    - Suporte a espelhamento horizontal instantâneo (`flip_x`) conforme a direção do ator.
    - Introspecção de estado da animação (`current_clip`, `get_current_frame`, `is_finished`).
  - **Sistema de Partículas 2D com Física e Presets (`packages/aipo-game/src/particles.aipo`)**:
    - Emissor de partículas de alta densidade (`ParticleEmitter`) com alocação protegida e teto configurável `max_particles`.
    - Simulação física completa com vetores de velocidade ($v_x, v_y$), aceleração gravitacional contínua (`gravity_y`) e amortecimento por arrasto aerodinâmico (`drag`).
    - Desvanecimento linear de opacidade (*alpha fading*) e interpolação de tamanho com suporte a partículas circulares e retangulares.
    - Catálogo de presets visuais prontos para uso em jogos: `"sparks"`, `"explosion"`, `"smoke"`, `"dust"`, `"coins"` e `"trail"`.
  - **Exemplo Executável Completo (`packages/aipo-game/examples/animation_and_particles.aipo`)**:
    - Demonstração a 60 FPS com ator animado (ciclos `idle`, `run` e `attack`), detecção de colisão com bordas da tela, emissão de poeira nos pés durante a corrida, faíscas e explosões pirotécnicas com drop de moedas e fumaça.
  - **Bateria de Testes Automatizados**:
    - Suíte de unidade em Aipo puro (`packages/aipo-game/tests/game_test.aipo`) e teste de integração `test_game_subsystems_unit_test_suite` em `crates/aipo-game-host/tests/bridge_tests.rs`.
    - Teste de integração de ponta a ponta `test_animation_and_particles_compilation_and_execution` validando múltiplos quadros de simulação sem vazamento ou panics.
  - Implementação do subsistema de HUD imediato (`packages/aipo-game/src/ui.aipo`):
    - `button(x, y, w, h, text) -> Bool`: botões imediatos com hit-testing de ponteiro e estados normal, hover e pressionado.
    - `health_bar(x, y, w, h, current, max)`: barra de vida reativa com gradação dinâmica de cores (verde $\to$ amarelo $\to$ vermelho) e texto numérico.
    - `progress_bar(x, y, w, h, current, max, r, g, b)`: barra genérica para estamina, mana e recursos.
    - `panel(x, y, w, h, title)`: janelas com barra de cabeçalho e molduras visuais.
    - `label(x, y, text, size, r, g, b)`: tipografia com drop shadow de alto contraste.
    - `badge(x, y, text, r, g, b)`: tags e contadores compactos em pílula.
  - Implementação do subsistema de Tilemap 2D (`packages/aipo-game/src/tilemap.aipo`):
    - `create_tilemap(cols, rows, tile_size, tex_id, tileset_cols)`: criação e alocação de malhas 2D.
    - `set_solid`, `is_solid_cell`, `is_solid_at`, `check_box`: consultas espaciais $O(1)$ em grid discreto.
    - `resolve_box_collision(map, x, y, w, h, vx, vy, dt)`: resolvedor de colisão contínua (*swept AABB*) com separação de eixos $X$ e $Y$, eliminando o problema de tunelamento (*wall tunneling*) em altas velocidades e permitindo deslizamento suave nas quinas.
    - `draw_tilemap(map, cam_x, cam_y, zoom, screen_w, screen_h)`: renderização otimizada com descarte de tiles fora da visão (*frustum culling*).
    - `raycast(map, x1, y1, x2, y2) -> Dict`: algoritmo DDA para traçado rápido de raios na grade.
  - Função nativa `host_draw_line(x1, y1, x2, y2, thickness, r, g, b, a)` adicionada a `crates/aipo-game-host/src/host_bridge.rs` com fallbacks headless.
  - Exemplo executável `packages/aipo-game/examples/tilemap_and_hud.aipo` demonstrando todos os subsistemas em ação com movimentação fluida, colisão suave, zoom de câmera e mira laser via raycasting.
  - Teste de integração automatizado `test_tilemap_and_game_hud_compilation_and_execution` em `crates/aipo-game-host/tests/bridge_tests.rs`.

- **Framework Declarativo de GUI Zoe UI e Engine Leona (`packages/aipo-zoe`)**:
  - Implementação completa do pacote `packages/aipo-zoe` (`aipo.zoe`), disponibilizando a arquitetura declarativa de GUI do Zoe UI para Aipo com aceleração por GPU via `aipo-game-host`.
  - **Engine de Layout Leona 2.0 (`leona.aipo`)**:
    - Algoritmo hierárquico resolvendo dimensões absolutas em pixels (`Float`/`Int`), percentuais (`"100%"`, `"50%"`), intrínsecas e espalhamento flexível (`"flex"`, `"auto"`), com alinhamento (`align_items`), distribuição (`justify_content`), espaçamento interno (`padding`) e vão entre filhos (`gap`).
    - Clamping estrito de restrições dimensionais: `min_width`, `max_width`, `min_height`, `max_height` em cartões e contêineres.
    - Fluxo multi-linha com quebra automática (*flow wrap* via `wrap: true` ou `flex_wrap: true`) para chips, tags e elementos dinâmicos.
    - Contêiner de sobreposição espacial (*stack layout* via `direction: "stack"`) para crachás flutuantes e sobreposições de camadas.
    - Otimização extrema de profundidade de pilha da VM Aipo via decomposição modular da função `layout_node`, prevenindo saturação de operand stack em árvores profundas.
  - **Reatividade Fina e Hooks Avançados (`hooks.aipo`, `types.aipo`)**:
    - Estado reativo fundamental com `use_state` e `set_state` integrado ao rastreador de dirty frames.
    - `MemoSignal` e hook `use_memo(compute_fn, deps)` com cache comutativo de avaliações caras e invalidação estrita de dependências.
    - Hook `use_effect(effect_fn, deps)` com ciclo de vida acoplado e execução garantida de callbacks de limpeza (*cleanup*).
  - **Camada de Overlays, Diálogos Modais e Portais (`overlay.aipo`)**:
    - Pilha global de overlays (`overlay_push`, `overlay_pop`, `overlay_clear`, `has_overlays`, `get_top_overlay`) desacoplada da hierarquia visual primária.
    - Suporte a scrim semitransparente escurecido e fechamento por toque externo (`dismissible`).
    - Diálogos modais prontos (`dialog`, `open_dialog`) com cabeçalho, corpo flexível e ações de confirmação e cancelamento.
    - Menus flutuantes tipo dropdown (`dropdown_menu`, `dropdown_item`) com auto-posicionamento relativo ao elemento de ativação.
    - Sistema de tooltips em pílula com temporizador de permanência (*hover threshold* de 350ms) e descarte automático ao mover o mouse.
  - **Acessibilidade, Navegação por Teclado e Foco (`app.aipo`, `renderer.aipo`)**:
    - Navegação sequencial ciclável por teclado com `Tab` e `Shift+Tab` entre controles focáveis (`focusable: true`, campos de texto).
    - Descarte de overlays e liberação de foco via tecla `Escape`.
    - Ativação imediata de botões e itens com as teclas `Enter` e `Espaço`.
    - Indicador visual de foco (*focus ring*) com contorno e halo difuso azul neon Catppuccin Mocha.
    - APIs programáticas de foco: `get_focused_node()` e `set_focused_node(node)`.
  - **Controles Especializados e Virtualização (`components.aipo`)**:
    - Catálogo completo: `button`, `switch`, `slider`, `progress_bar`, `card`, `badge`, `split_view`, `tab_bar`, `tab_view`, `scroll_view`, `viewport`, `text_input`, `number_input`, `dropdown_select`, `color_picker`.
    - Árvore hierárquica `tree_view(props)` com nós aninhados, chevrons colapsáveis e seleção reativa.
    - Lista virtualizada `virtual_list(props)` de complexidade $O(\text{viewport})$, renderizando grandes volumes de dados (10.000+ itens) com janela dinâmica e overscan protetor sem engasgos de memória.
  - **Micro-Animações e DevTools Inspector (`hooks.aipo`, `renderer.aipo`, `app.aipo`)**:
    - `TweenSignal` e hook `use_tween(initial, target, duration, easing)` suportando curvas `linear`, `ease_in`, `ease_out` e `ease_in_out` sincronizadas com o delta time `dt`.
    - Inspetor de árvore em tempo real (*DevTools Inspector*) alternável via `F12` ou `toggle_inspector()`, exibindo contornos de nós, destaques ao pairar e badge flutuante `<tag#id> WxH`.
  - **Paleta Catppuccin Mocha Embutida (`color.aipo`)**: Suporte a `rgb`, `rgba`, `hex` e constantes de tema modernas (`crust`, `mantle`, `base`, `surface_0..2`, `blue`, `lavender`, `green`, `red`, etc.).
  - **Ciclo de Vida e Renderizador GPU (`app.aipo`, `renderer.aipo`)**: Despacho de desenho recursivo, hit-testing de eventos de ponteiro/clique e renderização a 60 FPS com integração de lifecycle (`setup`, `update`, `draw`).
  - **Documentação e Roadmap Arquitetural Canônico (`docs/packages/aipo-zoe.md`, `docs/en/packages/aipo-zoe.md`)**:
    - Análise comparativa aprofundada de mercado com frameworks que compartilham a mesma filosofia declarativa sem widgets do SO (Freya/Torin, Flutter, Egui, SolidJS e Slint).
    - Conclusão com sucesso dos 6 eixos estratégicos de amadurecimento (M1 a M6) implementados em Aipo puro e testados de ponta a ponta.
    - Integração de `aipo.zoe` ao catálogo de pacotes oficiais e menus de navegação do site VitePress.
  - **Demonstração e Testes Automatizados**: Exemplos `packages/aipo-zoe/examples/dashboard.aipo` e `packages/aipo-zoe/examples/editor.aipo` e testes automatizados em `crates/aipo-game-host/tests/bridge_tests.rs` (`test_zoe_ui_dashboard_compilation_and_execution`, `test_zoe_ui_editor_compilation_and_execution`, `test_zoe_ui_unit_test_suite`) validando 100% das asserções.

- **Adaptador Host Agnóstico e Bindings Immediate-Mode GUI (`aipo-egui` & `packages/aipo-egui`)**:
  - Implementação completa da crate `crates/aipo-egui` concretizando o *thin proof* de interoperabilidade Rust do ADP-010.
  - Arquitetura 100% agnóstica de game engines: opera sobre a máquina de estados pura do `egui::Context`, sem dependências de janelas de SO (zero `winit`) ou bibliotecas de jogos.
  - Segurança estrita Safe Rust (`#![forbid(unsafe_code)]`), conformidade total com o modelo de capabilities (`deny-by-default` com capability `"egui"`) e handles geracionais (`aipo-host::HandleTable`) prevenindo *use-after-free*.
  - Extração universal de primitivas geométricas (`shapes`): converte a saída do `egui::FullOutput` em uma lista de dicionários (`rect`, `text`, `circle`, `line`, `bezier`, `path`) consumível por qualquer renderizador externo (GPU, Wasm Canvas, TUI ou simulação headless).
  - API completa de widgets imediatos exposta ao VM: `create_context`, `destroy_context`, `begin_frame`, `end_frame`, `begin_window`, `end_window`, `label`, `heading`, `separator`, `button`, `checkbox`, `slider`, `text_edit`, `progress_bar`, `wants_pointer_input`, `wants_keyboard_input`.
  - Pacote canônico `packages/aipo-egui` com manifesto `aipo.toml` e ponte `ffi.aipo` validada pelo `aipo check`.
  - Exemplo executável `examples/28_egui_immediate_gui.aipo` e respectivo snapshot de saída `examples/28_egui_immediate_gui.stdout`.
  - Suíte de 7 testes de integração automatizados em `crates/aipo-egui/tests/egui_integration_tests.rs` cobrindo o ciclo de vida, negação padrão de permissões, handles obsoletos, interações sintetizadas de ponteiro e execução de script de ponta a ponta.

- **Pipeline de Áudio Nativo e Sintetizador Chiptune Procedural (`aipo-game-host` & `packages/aipo-game`)**:
  - Integração do backend de áudio nativo acelerado via Macroquad / QuadSnd com suporte a carregamento e reprodução de arquivos de áudio WAV e OGG.
  - Implementação de sintetizador de ondas procedural em memória (`crates/aipo-game-host/src/audio_system.rs`) no estilo SFXR/ChipTone gerando buffers RIFF/WAV 16-bit PCM (44.1 kHz, mono) sem necessidade de arquivos externos de áudio.
  - Suporte a múltiplas formas de onda: Quadrada (Square com duty cycle ajustável), Dente-de-serra (Sawtooth), Senoidal (Sine), Triangular (Triangle) e Ruído Branco (Noise gerado por PRNG Xorshift32 rápido).
  - Envelopamento sonoro anti-click com ataque suave de 5ms, curva de decaimento exponencial e deslizamento dinâmico de frequência (`freq_slide`).
  - Presets retro configurados: `"coin"`, `"laser"` / `"shoot"`, `"jump"`, `"explosion"`, `"hit"` / `"hurt"`, `"powerup"`, `"click"` / `"beep"`.
  - Ponte FFI (`host_bridge.rs`) expandida com 7 novas funções nativas de áudio (totalizando 30 funções nativas):
    - `host_load_sound(path)`: carrega áudio do disco e retorna handle ID numérico.
    - `host_play_sound(sound_id, volume, pitch)`: reproduz som por ID.
    - `host_play_preset(name, volume, pitch)`: dispara preset procedural em tempo real com pitch ajustável.
    - `host_synth_sound(wave_type, start_freq, freq_slide, duration, volume)`: sintetiza onda customizada e retorna handle ID.
    - `host_stop_sound(sound_id)`: para reprodução de som.
    - `host_play_music(sound_id, volume, is_loop)`: gerencia música de fundo em loop contínuo.
    - `host_stop_music()`: para imediatamente a música de fundo.
  - Resolução de futuros assíncronos em Safe Rust estrito (`#![forbid(unsafe_code)]`) via `std::task::Waker::noop()` e `std::pin::pin!`.
  - Proteção total contra falhas em ambientes sem dispositivo de som / CI headless via `std::panic::catch_unwind(AssertUnwindSafe(...))`.
  - Módulos `packages/aipo-game/src/` (`ffi.aipo`, `audio.aipo`, `sfx.aipo`) atualizados com paridade total e validação pelo `aipo check`.
  - Efeitos sonoros procedurais integrados aos exemplos `examples/26_interactive_game.aipo`, `examples/27_camera_and_sprites.aipo` e ao jogo Snake embutido em `main.rs`.
  - Suíte de 9 testes automatizados verdes em `crates/aipo-game-host`.

- **Engine 2D Nativa e Host Desktop Miniquad (`aipo-game-host` & `packages/aipo-game`)**:
  - Implementação da crate `crates/aipo-game-host` fornecendo runtime nativo em GPU a 60 FPS com backend Miniquad / Macroquad.
  - Execução direta de scripts `.aipo` com descoberta de hooks de ciclo de vida (`setup()`, `update(dt)`, `draw()`).
  - Suporte a live hot-reload de scripts em tempo real via `F5` ou `Ctrl+R` com overlay gráfico de diagnósticos e números de linha em caso de erro.
  - Ponte FFI (`host_bridge.rs`) expondo 23 funções nativas registradas como globais, sob aliases canônicos `__aipo_game_*` e dentro do dicionário `game`:
    - Renderização 2D acelerada: `host_clear_background`, `host_draw_rect`, `host_draw_rect_lines`, `host_draw_circle`, `host_draw_text`.
    - Texturas e Spritesheets: `host_load_texture`, `host_draw_sprite`, `host_draw_sprite_subrect` com cache de texturas e textura de fallback segura.
    - Câmera 2D: `host_set_camera(tx, ty, zoom)` para rastreamento de alvo e zoom, e `host_reset_camera()` para renderização de HUD em coordenadas de tela.
    - Entrada e Tempo: `host_key_down`, `host_key_pressed` (mapeamento GLFW completo: WASD, Setas, Espaço, Enter, Esc), `host_mouse_x`, `host_mouse_y`, `host_mouse_btn`, `host_screen_width`, `host_screen_height`, `host_frame_time`.
  - Resiliência headless total: wrappers com `catch_unwind(AssertUnwindSafe)` permitindo execução de 100% dos testes sem servidores X11 ou janelas ativas.
  - Exposição de superfícies de compilação pública em `aipo-cli` (`compile_file`, `analyze_with_surface`, `prelude_surface`, `register_module_symbols`, `emit_diagnostics`).
  - Refatoração completa dos módulos de `packages/aipo-game/src/` (`actor`, `audio`, `behaviors`, `ffi`, `input`, `lib`, `nodes`, `scene`, `sfx`, `tween`) alinhando sintaxe de import, structs, laços e closures à gramática canônica da linguagem com validação total via `aipo check`.
  - Exemplos interativos: `examples/26_interactive_game.aipo` (física 2D, aceleração, amortecimento e coleta de moeda) e `examples/27_camera_and_sprites.aipo` (câmera com zoom e seguimento do jogador, mundo 1000x1000, 8 gemas colecionáveis e HUD).
  - Suíte de 5 testes de integração automatizados em `crates/aipo-game-host/tests/bridge_tests.rs`.
  - Documentação bilíngue completa atualizada em `docs/packages/aipo-game.md` e `docs/en/packages/aipo-game.md`.

- **Transição Arquitetural para WebAssembly e Roteiro de Self-Hosting (ADP-013)**:
  - Congelamento formal da versão inicial v0.1.0 (Stack VM) na branch `legacy/v0.1.0-stack-vm` e tag `v0.1.0-stack-vm-final`.
  - Adoção do WebAssembly (Wasm 2.0 / WASI) como substrato padrão de execução de alta performance, superando o piso de ~100ns do despacho em pilha para ~1-3ns com JIT nativo via Cranelift/Wasmtime.
  - Setup inicial da crate `aipo-wasm` integrada ao workspace para emissão limpa de arquivos binários `.wasm` via `wasm-encoder`.
  - Preservação integral de 100% das camadas de frontend (`aipo-source`, `aipo-diagnostics`, `aipo-lexer`, `aipo-syntax`, `aipo-hir`, `aipo-sema`).
  - Estabelecimento do roteiro de self-hosting em 3 fases, habilitando o futuro compilador auto-hospedado da Aipo sem aprisionamento em FFI.
- **Marco 1: O Fio Vertical Fino (Thin Vertical Slice - ADP-013)**:
  - Implementação completa do compilador de HIR para Wasm (`aipo_wasm::compile_hir`) em `crates/aipo-wasm/src/compiler.rs`.
  - Suporte a funções aritméticas com contratos estáticos (`Int`, `Float`, `Bool`) e resolução automática de tipos e inferência de retorno.
  - Alocação e compressão de variáveis locais Wasm (`let`, `var`), reatribuição (`=`) e atribuições compostas (`+=`, `-=`, `*=`, `//=`, `%=`).
  - Emissão de operadores aritméticos (`+`, `-`, `*`, `//`, `/`, `%`) com promoção automática de inteiros em divisões flutuantes `/` e operadores relacionais de comparação (`==`, `!=`, `<`, `<=`, `>`, `>=`).
  - Chamadas diretas entre funções compiladas no módulo Wasm e geração de ponto de entrada para scripts de nível superior (`__top_level__`).
  - Execução e verificação de ponta a ponta com o motor JIT `wasmtime` na suíte de integração de 17 testes automatizados em `aipo-wasm`.
- **Marco 2: Controle de Fluxo Estruturado (Structured Control Flow - ADP-013)**:
  - Implementação completa de controle de fluxo estruturado em WebAssembly: blocos condicionais (`if`, `elif`, `else`) e expressões de valor condicional (`if c then a else b`).
  - Mapeamento de laços para os blocos formais `block` e `loop` do WebAssembly: `while condition { ... }`, `loop { ... }` incondicional, e `repeat count [as i] { ... }` com passo e contador de iteração.
  - Saltos rotulados (`br`, `br_if`) gerenciados via pilha de controle (`ControlFrame: LoopBreak, LoopContinue, RepeatStep, Block`) calculando a profundidade relativa exata para `break` e `continue` em qualquer nível de aninhamento.
  - Validação estática de rejeição de `break` e `continue` fora de laços com diagnóstico claro de erro de compilação.
  - Suíte de 31 testes de ponta a ponta verdes em `crates/aipo-wasm` validados com o motor JIT `wasmtime`.
- **Marco 3: Modelo de Memória Linear e Estruturas de Dados (Memory Model & Dynamic Data Structures - ADP-013)**:
  - Configuração e exportação da memória linear WebAssembly (`memory`, 1 página inicial de 64 KiB expansível dinamicamente via `memory.grow`).
  - Implementação do alocador de bump nativo WebAssembly `__aipo_alloc(size: i32) -> i32` alinhado a 8 bytes, com crescimento sob demanda de páginas de memória e proteção contra out-of-memory.
  - Pool de strings estáticas na seção de dados Wasm (`DataSection`) no offset 1024 com prefixo de comprimento little-endian de 4 bytes e suporte a leitura de tamanho de strings via propriedade `.len`.
  - Layout e alinhamento de memória para structs (`StructLayout`) com cálculo estático de deslocamentos de campos alinhados a 8 bytes e suporte a campos tipados (`Int`, `Float`, `Bool`, ponteiros).
  - Instanciação de structs (`HirExpr::Construct`) com alocação dinâmica em memória linear, suporte a inicialização nomeada e posicional, e pilha de 8 variáveis locais temporárias (`__struct_temp_0..7`) para aninhamento arbitrário seguro de instâncias de struct literais.
  - Acesso a campos (`HirExpr::Dot`) e mutação em memória via atribuição direta (`p.x = val`) e composta (`p.x += delta`) emitindo instruções de carregamento e armazenamento de memória (`i64.load`, `f64.load`, `i32.load`, `i64.store`, `f64.store`, `i32.store`).
  - Rastreamento de tipos de variáveis locais (`LocalKind::Struct, LocalKind::String, LocalKind::Int...`) para desambiguação semântica estrita entre acessos a propriedades de strings e campos de structs.
  - Suíte de testes de integração expandida para 34 testes de ponta a ponta verdes em `crates/aipo-wasm` verificando isolamento entre instâncias, aninhamento e integridade dos bytes na memória linear.
- **Marco 4: Chamadas de Função, Recursão e Tabela de Indireção (Function Calls, Recursion & Indirect Calls - ADP-013)**:
  - Emissor WebAssembly estendido com suporte a tabelas de funções (`TableSection`, Seção 4) e segmentos ativos de elementos (`ElementSection`, Seção 9) em estrita conformidade com a ordenação canônica de seções do Wasm 2.0.
  - População automática da Tabela 0 (`funcref`) indexando todas as funções declaradas e funções anônimas sintetizadas (`__anon_fn_N`).
  - Suporte nativo completo a recursão direta (`factorial`, `fibonacci`) e recursão mútua (`is_even`, `is_odd`) emitindo instruções diretas `call` de zero overhead.
  - Implementação de despacho indireto via `call_indirect` para funções de primeira classe, suporte a atribuição de referências de função a variáveis locais (`let f = increment`), e passagem de funções como parâmetros de ordem superior (`apply(f, x)`).
  - Suporte completo a funções anônimas e lambdas curtas (`x => x * 2`, `(a, b) => a + b`), sintetizadas e compiladas como funções canônicas do módulo e registradas na Tabela 0.
  - Pré-registro de assinaturas de tipos para chamadas indiretas (`indirect_sigs`) cobrindo aridades de 0 a 8 com deduplicação estrutural de tipos.
  - Pilha de 8 temporários de chamada indireta (`__call_temp_0..7`) para suporte seguro a aninhamento arbitrário de chamadas indiretas (`f(g(h(x)))`).
  - Suíte de testes de integração expandida para 45 testes automatizados verdes em `crates/aipo-wasm` (40 de pipeline + 5 de emitter), executados e validados pelo motor JIT `wasmtime`.
- **Marco 5: Runtime Async/Await (Async/Await Runtime - ADP-013)**:
  - Implementação completa do runtime cooperativo de tarefas assíncronas integrado ao módulo WebAssembly, sem dependências externas.
  - Estrutura `AsyncHelpers` centralizando os índices das 5 funções intrínsecas do runtime: `__aipo_task_create`, `__aipo_task_drive`, `__aipo_await`, `__aipo_task_sleep`, `__aipo_task_cancel`.
  - Layout de memória linear de 48 bytes por tarefa: status (i32@0: Pending/Running/Ready/Cancelled), fn_table_idx (i32@4), result (i64@8), state (i32@16), arg_count (i32@20), arg0 (i64@24), arg1 (i64@32), next_ptr (i32@40).
  - Compilação de funções `async fn` com divisão automática em corpo canônico (`__async_body_{name}`) e função wrapper que cria o handle de tarefa via `__aipo_task_create`.
  - Despacho polimórfico via `call_indirect` no `__aipo_task_drive` com suporte a aridades 0, 1 e 2, permitindo passagem de argumentos para corpos assíncronos com diferentes assinaturas.
  - Compilação de `HirExpr::Await` com chamada ao intrínseco `__aipo_await` que conduz a tarefa até conclusão (status Ready) e retorna o resultado como `I64`.
  - Exportação do global `__aipo_virtual_time` (i64, mutável) para controle de tempo virtual por runtimes externos.
  - Propagação do parâmetro `async_helpers` por toda a cadeia de compilação: `compile_function_body`, `compile_stmts`, `compile_if_stmt`, `compile_while_stmt`, `compile_loop_stmt`, `compile_repeat_stmt` e `compile_expr` com todas as ~50 chamadas recursivas internas atualizadas.
  - Suíte de testes de integração expandida para 50 testes automatizados verdes em `crates/aipo-wasm` (45 de pipeline + 5 de emitter), incluindo 6 testes específicos de async: compilação de função assíncrona, exportação de funções de runtime, retorno de task handle, `await` conduzindo tarefa, funções assíncronas com parâmetros, e múltiplas tarefas independentes.
- **Marco 6: Ferramental CLI, Host ABI & WASI (Tooling CLI, Host ABI & WASI - ADP-013)**:
  - Integração completa do backend WebAssembly à interface de linha de comando `aipo`:
    - `aipo run <file.aipo> --wasm` e `aipo run <file.aipo> -t wasm` / `--target=wasm`: compilação JIT de ponta a ponta e execução direta via Wasmtime com streaming de I/O.
    - `aipo run <file.wasm>`: execução nativa de módulos binários `.wasm` pré-compilados sem necessidade de código-fonte.
    - `aipo build <file.aipo> --target wasm` / `--wasm`: compilação de código Aipo e emissão de binário WebAssembly canônico (`dist/app.wasm`).
    - `aipo disasm <file.wasm>` e `aipo disasm <file.aipo> --wasm`: desassembly de código WebAssembly diretamente para o formato de texto padrão (WAT) via `wasmprinter`.
    - `aipo check <file.aipo> --wasm` e `aipo check <file.wasm>`: validação estática de pipeline, contratos semânticos e conformidade do binário Wasm sem execução.
  - Implementação do Host ABI e streaming de I/O em `aipo-wasm`:
    - Módulo de importação de host `"aipo_host"` com bindings nativos para `print_int`, `print_float`, `print_str`, `print_bool` e `println`.
    - Suporte a chamadas canônicas da biblioteca padrão `io.print` e `io.println` e funções utilitárias `print` e `println`.
    - Importações condicionais: o emissor Wasm inclui a seção de imports (Seção 2) apenas se chamadas de I/O forem detectadas no programa, preservando zero overhead para módulos puramente computacionais.
    - Leitura direta da memória linear Wasm para strings literais com cabeçalho little-endian de comprimento.
  - Crate `aipo-cli` atualizada com o novo alvo `BuildTarget::Wasm`, documentação de flags e ajuda canônica (`aipo --help`) e `crates/aipo-cli/README.md`.
  - Suíte de 8 testes de integração exaustivos de ponta a ponta em `crates/aipo-cli/tests/wasm_cli.rs` cobrindo execução, build, desassembly, checagem, validação de saída, erros de sintaxe/semântica e rejeição estrita de flags inválidas.
  - 100% dos testes e verificações clippy sem advertências (`-D warnings`) em todo o workspace.

## [0.1.0] - 2026-09-26 (Linha de Base Stack VM)

- **Especificação Ergonômica de Sintaxe e Filosofia de Design (ADP-012)**:
  - **Blocos Delimitados por Chaves (`{ ... }`)**: Blocos estruturados explícitos com `{ ... }` em todas as estruturas de controle (`if`, `while`, `loop`, `repeat`, `each`, `match`, `attempt`, `await do`) e definições (`fn`, `struct`, `impl`, `init`, `invariant`, `interface`), dispensando parênteses ao redor de condições e garantindo alta legibilidade e suporte pleno a rainbow brackets.
  - **Imutabilidade de Campos de Struct por Padrão**: Campos declarados em `struct` (`id`, `created_at`) são estritamente imutáveis por padrão. Mutabilidade de campos exige declaração explícita com `var` (`var status = "idle"`).
  - **Instanciação Simétrica de Struct com Dois-Pontos (`:`)**: Padronização da inicialização de structs usando pares chave-valor idênticos aos dicionários (`User{ name: "Dev", age: 30 }`), unificando o modelo mental da linguagem.
  - **Mutabilidade Universal de Receptores (`var self`)**: Uso canônico de `var self` em métodos mutadores e hooks, alinhando a mutabilidade de instâncias à palavra-chave universal `var`.
  - **Subtipagem Estrutural Automática**: Structs satisfazem interfaces implicitamente quando suas assinaturas de método conferem, dispensando burocracias de acoplamento.
  - **Divisão Inteira Truncada (`//` e `//=`)**: Operadores canônicos `//` e `//=` para divisão inteira truncada em direção a zero, com simetria aritmética total.
  - **Formatter Canônico (`aipo-formatter`)**: Formatação canônica de blocos com `{ ... }`, espaçamento interno ergonômico e alinhamento idempotente.
  - **Documentação e Exemplos Multi-idiomas**: 100% da documentação oficial (em Português e Inglês) e todos os 24 programas em `examples/` alinhados e validados com 100% de paridade entre a VM, WebAssembly e o backend JavaScript.
- **Recorte da `aipo v0.1.0` (ADP-008)**: a primeira release é da linguagem, com async, CLI mínima, test runner, C ABI síncrona e thin proofs de interoperabilidade Rust/C/JS; engines, editors, registry e web profile completo ficam pós-v1.
- **Test Runner Canônico `aipo test` (ADP-008, ADP-012)**:
  - Implementado o comando `aipo test` no CLI para descoberta automática recursiva de testes em arquivos `*_test.aipo` e `test_*.aipo`.
  - Suporte a filtros de teste (`--filter <substr>`), saída formatada para humanos e streaming NDJSON legível por máquinas (`--message-format <human|jsonl>`) e `--package-cache`.
  - Isolamento estrito por teste: cada caso de teste roda em uma instância limpa da VM, com congelamento do relógio (`DeterministicClock`) e reset da semente do PRNG para zero (`reset_default_seed(0)`), garantindo determinismo temporal e numérico absoluto.
  - Interceptação nativa do subcomando de testes através de `TestMode::Discover` e `TestMode::Execute` em `aipo-vm`, com correspondente paridade diferencial em `aipo-js` (`setTestMode`, `testCall`).
- **C ABI Estável e Embedding Interface (`aipo-c-abi`, ADP-009, ADP-010)**:
  - Crate dedicada `crates/aipo-c-abi` e cabeçalho canônico C `crates/aipo-c-abi/include/aipo.h` para consumo direto em C, C++, Python ctypes, Go cgo e outras linguagens hospedeiras.
  - ABI estável, estritamente síncrona e single-threaded (`aipo_runtime_t`), sem promessa de concorrência interna, garantindo integridade sem overhead de locks.
  - Blindagem total contra panics do Rust: todas as funções exportadas são encapsuladas em `catch_unwind(AssertUnwindSafe(...))`, mapeando falhas internas para códigos C (`AIPO_ERR_FAULT`).
  - Representação desacoplada de valores em C (`aipo_value_t`), mantendo os invariantes de float finito e inteiros no intervalo seguro ±(2^53 - 1).
  - Handles geracionais do host (`aipo_handle_t`) validados por índice de slot e contador de geração; acesso após liberação retorna `AIPO_ERR_STALE_HANDLE` sem corrupção de memória.
  - Registro de funções nativas de host (`aipo_host_fn_t`), com controle de acesso baseado em capacidades (`grant`/`revoke`), retornando `AIPO_ERR_CAPABILITY_DENIED` quando não autorizadas.
  - Mapeamento transparente de falhas recuperáveis (Model B / `AIPO_ERR_UNCAUGHT_FAILURE`), erros diagnósticos (`AIPO_ERR_DIAGNOSTIC`), e recuperação do buffer do último erro (`aipo_last_error`).
- **Roteiro de Performance Estrutural e Ergonomia de Linguagem (ADP-011)**: formalizada a estratégia de superação do piso de despacho (~100ns/opcode) através de compactação de `Value` (48B -> 16B/24B), invocação fundida `InvokeMethod` (sem alocação de `BoundMethod`), laço de despacho com variáveis hoisted e superinstruções `Call0..Call4`/`GetLocal0..GetLocal3`; além da expansão ergonômica com falhas estruturadas tipadas (Model B+), pattern matching e type narrowing estático em `aipo-sema`.
- **Performance — Compactação de Layout de `Value` (P1)**:
  - Redução do tamanho da enum central `Value` de 40 bytes para 24 bytes (redução de 40% em memória por slot), alinhando o descritor de chamada e economizando largura de banda de cache L1/L2.
  - Validação empírica A/B comprovou ganho de **-13,94% na mediana de tempo de CPU** em workloads intensivos de pilha e chamadas de função.
- **Performance — Superinstruções `GetLocal0..7` e `Call0..4` (P4)**:
  - Introduzidos opcodes de byte único `GetLocal0..GetLocal7` (60..67) e `Call0..Call4` (68..72), eliminando decodificação de operandos Big-Endian no caminho quente da VM.
  - A VM rastreia a largura dinâmica da instrução de chamada (`call_inst_len`), garantindo que suspensões cooperativas e temporizadas (`task.sleep`, `task.timeout`, `task.all`, `race`) retrocedam o IP com exatidão matemática.
  - Validação empírica A/B via `scripts/perf/cpu_ab.py` com 8 rounds e 8 reps atestou **-8,86% no tempo mínimo de CPU** e **-6,49% na mediana** na suíte completa (`fields, arithmetic, recursion, collections`).
- **Performance — Despacho Direto de Métodos de Structs (P2)**:
  - Adicionada a variante leve `Value::StructMethod { receiver, entry_ip, total_arity, is_async }` e registro monomórfico na VM, eliminando a instanciação transitória de `Rc<BoundMethodData>` no heap.
  - Elimina 200.000 alocações no heap por 100k chamadas no benchmark `fields`, com ganho comprovado de **-7,54% na mediana de CPU**.
- **Ergonomia — Falhas Estruturadas Tipadas (E1 / Model B+)**:
  - `FailureValue` agora carrega `payload: Value`, suportando construtores `FailureValue::new` e `with_payload`, e construtores correspondentes em `Value`.
  - Acesso a `err.payload` suportado nativamente na VM e na função nativa `fail` da stdlib; paridade diferencial de 100% mantida no backend JavaScript (`aipo-js`).

### Adicionado
- **Performance — Cross-language benchmark suite**:
  - `aipo-bench --compare` compara workloads determinísticos com Aipo CLI/VM, Aipo VM in-process, Aipo→JavaScript/Node, Lua, LuaJIT, Wren, Luau, CPython, PyPy, Ruby, JavaScript/Node e Rust nativo.
  - O runner exige checksums iguais, executa as mesmas operações lógicas no Rust nativo, mede warmup separadamente e grava samples brutos, median/MAD, p95, versões, profile e estado do Git em JSON.
  - `--compare-workloads` seleciona workloads individualmente para A/B isolado, sem misturar o custo de outras operações.
  - Rust nativo é controle de lower bound; startup, compilação e steady-state são reportados separadamente, com limites explícitos de entrada e rondas.
  - A primeira onda adiciona Wren 0.4.0, Luau 0.739 e PyPy 8.0.0 como referências opcionais, com fixtures portáveis, candidatos de executável, argumentos específicos do Luau e fontes pinadas em `aipo-reference-runtime/SOURCES.json`.
  - O workload `fields` mede seis campos, mutação e dispatch de método; a primeira evidência directional mostrou Aipo VM em 1.430,92ms contra 120,35ms do Wren e 63,15ms do Luau, motivando uma otimização de lookup de fields como próximo experimento.
  - **Performance — Cache monomórfico de field slot**: `aipo-vm` memoriza o slot por site e tipo, acelerando `GetField`/`SetField` sem alterar o layout público de `StructInstance`; A/B de 15 samples reduziu o median de 1.001,77ms para 718,20ms (-28,31%) no workload `fields`, com 2.399.992 hits e 8 misses.
  - **Performance — Cache do frame base**: `aipo-vm` mantém o `stack_base` do frame ativo para evitar `frames.last()` em cada acesso a local; A/B pareado de 31 samples reduziu `arithmetic` de 276,23ms para 265,98ms (-3,71%), `fields` de 736,02ms para 687,05ms (-6,65%) e `recursion` de 97,72ms para 93,65ms (-4,17%), com checksums e métricas idênticos. A validade da restauração após retorno aninhado e troca de tasks tem teste de regressão dedicado.
  - **Performance — `SetField` sem cópia de entrada para structs unguarded**: a cópia do valor anterior agora ocorre somente quando o tipo é guarded e precisa entrar no journal; o fixture `fields` elimina 600.000 clones de `Value` de forma determinística. O A/B de 31 samples não mostrou ganho consistente de wall-clock, portanto não há claim de velocidade.
  - O relatório agora usa schema 5, expõe `field_cache_hits`/`field_cache_misses` e pode registrar `peak_rss_bytes` por amostragem Linux com `--compare-resources`, em uma execução separada; contadores de alocação permanecem nulos até adapters runtime-specific existirem.
  - **Performance — runner pareado dedicado**: `scripts/perf/paired.sh` fixa CPU, alterna baseline/candidate, exige `taskset` por padrão e grava relatórios com manifest de proveniência sem modificar o estado do Git.
  - **Performance — entry de cache com guardedness**: o cache por site foi estendido para guardar `type_is_guarded`, mas o A/B pareado não mostrou ganho (+6,32% em `arithmetic`, +1,40% em `fields`, +2,13% em `recursion`) e a extensão foi revertida.
  - **Performance — cache de flags `fixed` por slot**: o `Vec<bool>` experimental foi mais lento no A/B pareado (+5,34% em `arithmetic`, +11,32% em `fields`, +144,36% em `recursion`) e foi revertido.
  - **Performance — cache de tipos guarded**: o `HashSet<String>` experimental foi mais lento no A/B pareado (+6,58% em `arithmetic`, +1,63% em `fields`, +3,36% em `recursion`) e foi revertido.
  - **Performance — layout denso de `StructInstance`**: separar `field_names` de `fields` (em vez de `Vec<(String, Value)>`) piorou `fields` nos três pares do A/B pareado (+3,73%, +7,39% e +24,23%), em parte por trocar uma alocação por instância de struct por duas. Como o experimento também era API-breaking, foi revertido e a API pública permanece inalterada.
  - **Performance — despacho de método sem alocar**: um índice aninhado para `struct_methods` eliminou quatro alocações de `String` por bind de método, mas o A/B de quatro lotes não demonstrou ganho: a mesma mudança oscilou de -5,6% a +4,5% conforme a posição do novo campo na struct `Vm`. O protótipo foi revertido para não duplicar estado de registro sem benefício.
  - **Performance — despacho de opcode sem `Result` largo**: `Vm::step` decodificava cada opcode com `try_from(..).map_err(..)?`, construindo um `Result` de 72 bytes (o tamanho de `VmFault`) e um closure que capturava `self` em **todo** opcode executado. O `match` equivalente só constrói a falha no ramo frio, com offset e mensagem idênticos, e um teste novo fixa esse erro. A medição por tempo de CPU com 24 repetições alternadas deu resultado **nulo** (min 3,019s contra 3,014s, -0,16%), porque o compilador já afundava a construção da falha para fora do caminho feliz. A alteração foi mantida como robustez e clareza, não como otimização: o caminho de erro frio passa a ser explícito e o contrato de `unknown opcode` ganhou teste.
  - **Metodologia — piso de ruído do runner**: o host entrega aproximadamente ±5% de ruído em `fields` e `arithmetic` em condições normais, e até 2,5x sob contenção; diferenças menores que isso não são evidência, e `arithmetic` passa a ser usado como workload de controle por não tocar o caminho alterado. `scripts/perf/cpu_ab.py` compara tempo de CPU do processo filho em vez de wall-clock, que é o estimador robusto sob disputa.
- **Performance — Otimizações do caminho quente da VM e do shim JS**:
  - `aipo-vm` passou a usar cache de constantes, slots de globals e índices de campos/métodos, com fallback para structs nativas não registradas; natives e conversões também recebem argumentos por slice emprestado, removendo `Vec` por chamada.
  - `aipo-bytecode` deduplica constantes e remove propagações de falha redundantes após constantes e functions; strings ASCII têm fast path para concatenação e indexação.
  - `aipo-js` prepara locals/upvalues por slot no carregamento do módulo, usa cache preguiçoso de constantes por instrução e mantém o contrato de paridade; métricas da VM são coletadas fora do tempo de execução no relatório cross-language.
  - A rodada local final registrou 45 resultados, 0 skipped e 0 failures; o relatório completo permanece em `target/cross-language-final.json`, e a medição pós-mudança sob carga elevada está em `target/cross-language-final-current.json`.
- **CLI — Autenticação GitHub opt-in (P04-G11)**:
  - `aipo package fetch-github` e `aipo package lock --fetch-github` aceitam `--github-token-env <name>`; o valor do token vem exclusivamente da variável de ambiente nomeada.
  - Credenciais bearer são validadas, redacted em `Debug` e nunca são aceitas diretamente como argumentos, persistidas, logadas ou incluídas em diagnostics; sem a flag, o comportamento público sem `Authorization` permanece o padrão.
  - Tokens ausentes, vazios ou com whitespace/caracteres de controle falham com `AIPO_PKG_FETCH` antes de qualquer request; redirects continuam bloqueados.
- **CLI — Prune explícito do cache (P04-G10)**:
  - `aipo package cache prune <dir> --lock <lockfile>` é dry-run por padrão e lista somente entradas verificadas não referenciadas pelo lockfile.
  - `--apply` é obrigatório para remover; qualquer entrada inválida bloqueia toda a operação, sources referenciadas são preservadas e o lockfile nunca é alterado.
  - A política não usa rede, autenticação ou registry e não remove entradas corruptas/desconhecidas.
- **CLI — Verificação read-only do cache (P04-G09)**:
  - `aipo package cache verify <dir>` audita todas as entradas GitHub existentes, validando symlinks, arquivos regulares, metadata, source/key, manifest e digests SHA-256.
  - Entradas corruptas são reportadas como `AIPO_PKG_FETCH`; cache ausente ou vazio retorna sucesso com zero entradas.
  - A operação está disponível no build padrão, não acessa a rede e nunca cria, repara, remove ou faz prune no cache.
- **CLI — Raízes locais com dependências GitHub pinadas (P04-G08)**:
  - `aipo package lock <dir> --fetch-github --cache <dir>` resolve branches `path` locais e edges GitHub pinadas em um lockfile determinístico único, usando o cache explícito e a feature opt-in `github-http`.
  - `run`, `check`, `build` e `disasm` consomem o grafo misto com `--package-cache`, sem rede ou escrita no cache; provenance `path`/`github` e paths físicos das entries são preservados.
  - A descoberta local não carrega edges GitHub; conflitos, ciclos, capability widening, locks ausentes/stale e artifacts ausentes/corrompidos continuam fail-closed.
- **CLI — Execução Direta de Bytecode `.aibc` e Subcomando `disasm`**:
  - `aipo run file.aibc`: execução direta de módulos bytecode pré-compilados, com detecção automática pela extensão `.aibc`, desserialização via `BytecodeModule::from_bytes`, verificação estrutural pelo `BytecodeVerifier`, e execução na VM sem passar pelo pipeline frontend (lexer→parser→HIR→sema→IR→emitter). Erros de desserialização e verificação reportam falha de linguagem (exit 1); arquivos inexistentes reportam erro de uso (exit 2).
  - `aipo disasm <file.aipo|file.aibc>`: novo subcomando de desassembly, exibindo listagem legível de constantes, nomes, funções e instruções. Para arquivos `.aipo`, compila e usa `disassemble_with_source` com anotações `[line:col]`; para arquivos `.aibc`, usa `disassemble` sem anotações de código-fonte. Erros de compilação/desserialização são reportados normalmente.
  - `docs/reference/cli.md`: referência normativa atualizada com ambos os comandos, exemplos e estabilidade declarada desde Wave 4.
- **Bytecode e VM — Serialização Binária Nativa `.aibc` e Mapeamento de Linhas**:
  - `aipo-bytecode`: implementação da serialização e desserialização binária nativa e determinística (`BytecodeModule::write_to`, `read_from`, `to_bytes`, `from_bytes`) com codificação BigEndian para cabeçalho mágico (`AIBC`), versão 1, tabela de nomes internados, pool de constantes tipadas, layouts de structs, metadados de funções compiladas (com slots de upvalues e flag assíncrona), fluxo de bytes de instruções e mapa de spans de código fonte.
  - `aipo-bytecode`: mapeamento de linhas e colunas no desassemblador via `disassemble_with_source(module, source)`, anotando cada instrução com `[line:col]` a partir dos spans do módulo compilado.
- **VM — Otimização de Layout de Memória e Eliminação de Alocações Transitórias**:
  - `aipo-vm`: empacotamento em `Rc` das variantes pesadas `Value::Closure(Rc<ClosureData>)` e `Value::BoundMethod(Rc<BoundMethodData>)`, reduzindo o tamanho de `Value` de ~72/80 bytes para 48 bytes (redução de até 40% na pilha de avaliação, frames locais e coleções).
  - `aipo-vm`: eliminação da alocação de `Box<Value>` para o receptor (`receiver`) dentro de métodos ligados (`BoundMethodData`), unificando o armazenamento em um único nó de heap e tornando a clonagem de closures instantânea (sem clonar o vetor de upvalues).
- **Maturidade e Fechamento Integral do Frontend V1**:
  - **Teste de Tipo Anulável em `is` (`value is Type?`)**: parsing em `aipo-syntax`, variante `BinaryOp::IsNullable` no AST, instrução `CoreInst::TypeIsNullable` no Core IR, OpCode 58 `TypeIsNullable` no bytecode, execução nativa na VM Rust e no runtime JavaScript (`aipo-runtime.js`), avaliando o sujeito uma única vez e retornando `true` imediatamente se o valor for `none` ou delegando ao teste de tipo concreto caso contrário, com 100% de paridade diferencial verificada.
  - **Açúcar Sintático de Múltiplos Sujeitos em `is` (`a, b, c is Type[?]`)**: suporte canônico (`docs/canon/Aipo V1 — Language Reference:131`) no parser expandindo `a, b, c is T` em `((a is T and b is T) and c is T)`, com avaliação estrita da esquerda para a direita, curto-circuito em `and`, suporte a tipos anuláveis e isolamento contextual de vírgulas dentro de argumentos de chamada, literais de coleção e padrões de `match`.
  - **Diagnóstico Educativo Canônico para `is not`**: detecção no parser de tentativas de uso da sintaxe Python `is not`, gerando diagnóstico antecipado `AIPO_PARSE_UNEXPECTED_TOKEN` com sugestão explícita para uso da forma canônica `not value is Type`.
- **Maturidade da Stdlib e Backend — Módulos Canônicos `binary`, `time` (Tipos Puros e Parsers), `expect`/`testing`, e `log`**:
  - `binary`: operações completas de leitura e escrita big-endian e little-endian sobre `Bytes` (`read_*_be`, `read_*_le`, `write_*_be`, `write_*_le` para `i8`, `u8`, `i16`, `u16`, `i32`, `u32`, `i64`, `u64`, `f32`, `f64`), codificação e decodificação de varints LEB128 não-assinados (`read_varint`, `write_varint`) com validação de limite seguro (`MAX_SAFE_INT`), e fatiamento tolerante `slice(start, end)`.
  - `time`: introdução dos tipos puros de calendário civil e horários (`Date`, `TimeOfDay`, `DateTime`, construtores `time.date`, `time.time_of_day`, `time.date_time`, `time.duration`), cálculo de anos bissextos e dias desde o epoch Unix via algoritmo civil puro de Howard Hinnant (`days_since_epoch`), parsers ISO 8601 (`parse_date`, `parse_time`, `parse_iso`), formatadores `.to_iso()` e cálculo de segundos Unix via `.epoch_seconds()`.
  - `testing` & `expect`: suíte canônica de asserções puras (`expect.equal`, `expect.not_equal`, `expect.true`, `expect.false`, `expect.none`, `expect.some`, `expect.failure`, `expect.contains`, `expect.approx`) retornando `none` em caso de sucesso e `Failure` recuperável (Model B) em caso de divergência.
  - `log`: módulo de logging estruturado baseado em níveis (`trace`, `debug`, `info`, `warning`, `error`), com sink de log configurável e roteamento para o sink de `io` por padrão.
  - Runtime e VM: isenção das asserções de inspeção de falhas (`expect.failure`, `testing.failure`) da auto-propagação imediata de argumentos em chamadas de função, permitindo asserir falhas recuperáveis sem aborto prematuro.
  - Paridade diferencial VM↔JS: 26 testes diferenciais bit-a-bit passando 100% de forma determinística no Node.js e na VM Rust.
- **Maturidade da Stdlib e Backend — Módulos Canônicos `encoding`, `path`, `url` e `regex`**:
  - `encoding`: codificação e decodificação estrita de `base64_encode`, `base64_decode`, `base64url_encode`, `base64url_decode`, `hex_encode`, `hex_decode`, `utf8_encode`, `utf8_decode` com `Failure` recuperável (Model B) em entradas inválidas.
  - `path`: manipulação puramente lógica e normalização de caminhos (`join`, `normalize`, `is_absolute`, `basename`, `dirname`, `ext`) com suporte a múltiplos separadores (`/`, `\`).
  - `url`: modelo canônico WHATWG URL em `url.parse(text)` retornando dicionário estruturado com `href`, `origin`, `protocol`, `username`, `password`, `host`, `hostname`, `port`, `pathname`, `search`, `hash` ou `Failure` em URLs inválidas.
  - `regex`: motor regex em tempo linear (`regex.compile`, `regex.is_match`, `regex.replace`) retornando struct `Pattern` com métodos (`is_match`, `find`, `find_all`, `replace`, `split`) com 100% de paridade diferencial VM↔JS.
  - `collections`: alinhamento de vocabulário único eager/lazy em `List` (`first_or`, `last_or`, `find_index`, `take`, `skip`, `distinct`, `zip`, `chain`, `chunk`, `window`, `enumerate`).
  - `string`: operações canônicas human-facing em aglomerados de grafemas estendidos (`graphemes()`, `words()`, `lines()`, `casefold()`, `encode_utf8()`, e `Bytes.decode_utf8()`).
- **Maturidade da Stdlib e Backend — Operações Ávidas de Coleções e Métodos de Dicionário**: implementação de métodos ávidos de ordem superior em listas (`list.map(fn)`, `list.any(fn)`, `list.all(fn)`, `list.flat_map(fn)`, `list.reduce(init, fn)`) no despachador de métodos da VM (`aipo-vm/src/vm/call.rs`, `aipo-vm/src/vm/method.rs`) e no runtime JS (`aipo-runtime.js`), além de `dict.entries()` retornando pares `[[k, v]]`.
- **Maturidade da Stdlib e Backend — Extensões Canônicas do Módulo `math`**: trigonometria (`sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`), funções logarítmicas/exponenciais (`log`, `log2`, `log10`, `exp`), além de `hypot`, `sign`, `rad`, `deg` em `aipo-stdlib/src/math.rs` e `aipo-runtime.js`, com validação estrita de domínio gerando `Failure` recuperável (Model B) em `log(<=0)` e `asin/acos(|x|>1)`.
- **Maturidade da Stdlib e Backend — Módulo `random` Portável (PRNG SplitMix64 Determinístico)**: implementação exata a nível de bits do PRNG SplitMix64 em `aipo-stdlib/src/random.rs` e `aipo-runtime.js` (`random.create(seed)`, `random.seed(seed)`, `int(min, max)`, `float()`, `bool()`, `choice(list)`, `shuffle(list)`), com suporte a despacho de métodos nativos em instâncias de `Struct` (`Rng`).
- **Maturidade da Stdlib e Backend — Módulo `json` Portável**: serializador e deserializador canônico em `aipo-stdlib/src/json.rs` e `aipo-runtime.js` (`json.parse` com rejeição estrita de chaves duplicadas como `Failure`, `json.stringify` com detecção de ciclo e formatação canônica de floats e inteiros), com 100% de paridade diferencial verificada.
- **Maturidade do Frontend — Lambdas Curtas `=>`**: suporte completo a lambdas de expressão concisas (`x => expr`, `_ => expr`, `() => expr`, `(a, b) => expr`, `(a, _) => expr`) no lexer (`TokenKind::FatArrow`), parser (com lookahead `is_lambda_ahead` semântico), HIR, IR, VM e `aipo-js`, com paridade diferencial 100% verificada no Node.js.
- **Maturidade do Frontend — Continuações Elididas de Comparações**: parsing e desaçucaramento canônico de cadeias com sujeito elidido (`val >= 0 and <= 100`, `val is Int and >= 0 and <= 100`), além de diagnóstico educativo amigável `AIPO_PARSE_UNEXPECTED_TOKEN` para comparações encadeadas diretas sem `and` (`1 < x < 10`).
- **Maturidade do Frontend — Otimização Zero-Alloc no Lexer**: eliminação de alocação de `Vec<(usize, char)>` no scan de arquivos inteiros; matching de keywords diretamente sobre fatias de bytes/string `&str` sem alocação intermediária; validação e parsing de literais numéricos em `&[u8]` sem cópias desnecessárias de string.

### Corrigido
- **Auditoria de IR, Host e Runtime — Semântica Canônica e Robustez de Fronteira**:
  - `aipo-ir` (navegação segura): `p?.name` e `svc?.call(args)` baixam para um teste de `none` com o receptor avaliado uma única vez; se o receptor for `none`, nem o acesso nem os argumentos executam e a expressão é `none` — em ambos os backends (canon §143; auditoria IR-5).
  - `aipo-ir`/`aipo-vm`/`aipo-js` (or_else lazy): `a or_else b` agora registra um handler de recuperação; o fallback só executa quando a esquerda termina em `Failure`, e um fallback que também falha propaga para o handler externo (auditoria IR-6/IR-7).
  - `aipo-ir` (falhas em controle de fluxo): condições de `if`/`elif`/`while`/`match`/`repeat`/`each` e condições de `if`-expressão propagam `Failure` recuperável via `PropagateFailure` antes do desvio, em vez de convertê-la em fault de tipo (auditoria IR-9).
  - `aipo-ir`/`aipo-vm`/`aipo-js` (iteração): `each` projeta bindings por modo (`IterAt`), então `each key[, value] in dict` itera chaves em ordem de inserção com o valor lido ao vivo, `each i, v in list` mantém índice/elemento e `each x in list` o elemento; guarda de mutação preservada em ambos os backends (auditoria IR-10).
  - `aipo-ir`/`aipo-vm`/`aipo-js` (unwind de loop): `break`, `continue` e `return` que saem de um `attempt`/`each` recolhem os handlers e guards abertos no corpo, eliminando handlers obsoletos e guards de iteração fantasma após o retorno (auditoria IR-8/IR-12). `IterGuard`/`IterGuardEnd` agora são balanceados também para iteráveis sem identidade (ranges, strings), com sentinela que nunca casa uma coleção real.
  - `aipo-ir`/`aipo-vm`/`aipo-js` (atribuição): um `Failure` no alvo ou no valor de `a.field = v`/`c[i] = v` propaga no ato (Model B) em vez de virar fault de tipo ou ficar na pilha; `SetField`/`SetIndex` deixam de despejar `none` na pilha do backend JS, eliminando o vazamento de operandos (auditoria IR-11/IR-18).
  - `aipo-ir` (await do): `await do` agora dessuga também `while`, `loop`, `repeat`, `attempt`, atribuições, `compound assign`, `return` e `fail`, e reconhece métodos assíncronos registrados como `Type.method` (auditoria IR-13).
  - `aipo-ir` (capturas, pipeline e contratos): corpos de closures anônimas não poluem mais a coleta de nomes livres do escopo externo; `a |> f` estaciona o argumento antes do callee; `return` sem valor sob contrato não-nulável emite `none` + checagem e falha como violação de contrato (auditoria IR-14/IR-16/IR-17); `a?.b = v` é rejeitado de forma consistente entre parser, sema e IR; alvos inválidos emitem `Fail` explícito; nomes de hidden locals não colidem mais com bindings do usuário; `each` sem nomes não lê a coleção.
  - `aipo-host` (fronteira): `HostValue::validate` re-checa `Int`/`Float` construídos diretamente nas variantes; nomes vazios/duplicados de tipos, valores, campos, parâmetros e operações de handle são rejeitados; profundidade de contratos limitada; parâmetro obrigatório após opcional e `subject` opcional rejeitados; `clear()` não queima gerações de slots já livres; mensagens de fault sem problemas renderizam sentença completa (auditoria N-1..N-9).
  - `aipo-runtime` (determinismo): registro duplicado de módulo é rejeitado em vez de sobrescrever silenciosamente, caminhos canônicos validados, dependência ausente reportada em ordem lexicográfica, `NativeRegistry` reporta colisões, ordena listagens e normaliza módulo vazio para o Prelude, `InitializationFailed` carrega `[AIPO_RT_MODULE_INIT_FAILED]` no `Display` e o fallback de ciclo do Kahn reporta os nós não resolvidos (auditoria N-1..N-11).
  - Testes: 8 testes diferenciais VM↔JS novos (`safe_navigation`, `or_else_lazy`, `condition_failure`, `each_dict`, `assign_pipeline`, `await_do_blocks`, `loop_unwind`), 6 testes unitários de lowering no `aipo-ir`, matriz de contrato para `return` vazio, validações de host, testes de runtime e remoção da corrida de arquivo temporário na suíte de conformance da CLI. Shim JS versionado em `1.2.0`.
- **Bytecode e VM — Correções Críticas de Segurança, Concorrência e Verificador**:
  - `aipo-vm` (Segurança Sandbox): travessia recursiva profunda em `publish_check` em `aipo-vm/src/host.rs` inspecionando `Value::Closure` (células de upvalues), `Value::BoundMethod` (receptor) e `Value::Sequence` (cache, pipeline e geradores), prevenindo escape de handles e capacidades não-publicáveis fora de seu escopo.
  - `aipo-vm` (Concorrência Assíncrona): detecção de ciclos de await transitivos estendida para abranger `WaitTarget::Join` e grupos estruturados de tarefas (`JoinKind::All` e `JoinKind::GroupWait`), detectando deadlocks circulares e emitindo `AIPO_RT_AWAIT_CYCLE`.
  - `aipo-vm` (Ordem Superior): métodos ávidos de lista (`filter`, `any`, `all`, `flat_map`) preservam o erro exato original (`VmError`) retornado pela função de callback em vez de substituir espuriamente por `StackUnderflow`; liberação garantida do guard de mutação em iteração ativa (`active_iterations`) no método `reduce` mesmo na ocorrência de falhas ou erros na função de callback.
  - `aipo-bytecode` (Verificador Estrutural): validação estrita de bounds de slots locais (`GetLocal`, `SetLocal`, `JumpIfSetLocal` limitados a `params + locals`), slots de upvalues (`GetUpvalue`, `SetUpvalue` limitados a `func.upvalues`) e integridade de contagem de campos em `BuildStruct` comparados ao layout declarado da struct.
  - `aipo-bytecode` (Emissor): substituição de fallback arbitrário para `Add` em `CoreInst::Binary` por erro de compilação explícito caso um operador de IR não seja reconhecido.
- **Maturidade do Frontend — Varredura e Correções Canônicas Integrais**:
  - `aipo-source`: cálculo de linha e coluna UTF-8 em `Source::location` corrigido com alinhamento seguro à fronteira de caractere (`is_char_boundary`), garantindo compatibilidade com MSRV 1.85.0 e prevenindo erros em offsets em código com caracteres multi-byte.
  - `aipo-lexer`: tratamento e rejeição canônica de literais numéricos com ponto sem fração seguidos de expoente (`1.e5`) e com sublinhados inválidos (`1._5`), emitindo `AIPO_LEX_INVALID_NUMBER`.
  - `aipo-syntax`: suporte e testes da forma condicional inline de valor (`if c then a else b`), preservação do modificador `async` em assinaturas de métodos de interface (`interface ... async fn ... end`).
  - `aipo-hir`: unificação estrita de spans no hook `invariant()` (`acc.span().merge(right.span())`) garantindo diagnósticos apontando exatamente para o bloco de expressão correto.
  - `aipo-sema`: validação profunda de contratos de interface em `satisfy` — verificação de presença, aridade de parâmetros explícitos (descontando `self`), mutabilidade de receptor (`self!`), modificador `async`, contratos de tipo de parâmetros e tipo de retorno (`AIPO_SEM_CONTRACT_VIOLATION_STATIC`); validação estática de reatribuição de campos declarados `fixed` em instâncias de struct conhecidas (`AIPO_SEM_FIXED_REASSIGN`); registro e checagem de aridade e contratos de métodos de `impl` e funções locais.
  - `aipo-testkit`: injeção de manifesto de pacote ESM (`package.json` com `{"type": "module"}`) em diretórios temporários de bundles JS gerados para execução com Node.js, eliminando warnings espúrios na saída de erro padrão no Node 22+.
- **Maturidade do Frontend — Correções Semânticas e Lowering**:
  - `Assign` e `CompoundAssign` analisam a expressão do lado direito como topo (`analyze_expr_top`), evitando falso positivo de `AIPO_SEM_AWAIT_IN_SUBEXPRESSION` em comandos de atribuição.
  - Verificação estrita de mutabilidade de caminhos com validação de receptor `self!` vs `self` em métodos e proibição de reatribuição de campos declarados `fixed` fora de `init` (`AIPO_SEM_FIXED_REASSIGN`).
  - Verificação de tarefas esquecidas (`AIPO_SEM_FORGOTTEN_TASK`) estendida para chamadas de métodos assíncronos em instâncias (`expr.method()`).
  - Acumulação de span com `merge` correto no lowering de hooks de invariante em `aipo-hir`.
  - Recuperação resiliente de erros em declarações no parser via `synchronize()` com garantia estrita de progresso e prevenção de estouro de pilha sob aninhamentos profundos de parênteses (`parse_paren_expr` e helpers de lambda `#[inline(never)]`).
- **Maturidade de IR, Host e Runtime — Auditoria e Correções de Corretude**:
  - `aipo-ir`: `CompoundAssign` em `Index` avalia `base`/`idx` uma única vez via hidden locals (`list[f()] += 1` sem duplo efeito colateral); targets inválidos emitem `Fail` explícito em vez de silêncio (backstop do `check_assignment_target` da sema); `FnDecl` registra o nome declarado em `seen` na coleta de livres.
  - `aipo-host`: `HostFault::Schema` mapeia para o novo `AIPO_RT_INVALID_SCHEMA` (antes `AIPO_PARSE_UNEXPECTED_TOKEN`); `HandleTable::clear()` repopula a free-list reaproveitando slots; namespace por categoria em `ModuleSchema::validate` documentado como intencional.
  - `aipo-runtime`: `InitializationFailed` mapeia para o novo `AIPO_RT_MODULE_INIT_FAILED` (antes `AIPO_RT_TYPE_MISMATCH`); `NativeRegistry` documentado como catálogo de introspecção/tooling (despacho real por nome em `aipo-vm/src/vm/call.rs`); `topological_init_order` com min-heap O(n log n).

### Corrigido
- **Auditoria de docs (P01-G02)**: varredura completa de `docs/` — resíduo "Odin"
  confinado ao material histórico do canon (1 menção normativa corrigida sem
  mudança semântica); `scope.md`, `architecture/overview.md`, coding-standards,
  governance, deployment, observability, lifecycle, security-contract (paths +
  tabela `unsafe`), PRUMO.md, mvp-subset, status das waves e ADR-001 alinhados
  ao implementado (Waves 1–2, `aipo build`, sem GC, sem stdlib externa).
  114 links internos validados, zero quebrados. Registro em
  `docs/journal/2026-09-20-docs-audit.md`.

### Adicionado
- **Wave 4 — Adaptador Poppy e demo headless determinística (P03-G02)**: novo crate `aipo-poppy` implementando o perfil de referência do Poppy Game Engine sobre `aipo-host`. AHS canônico (`poppy_schema`) expondo tipos (`Vec2`, `Transform`), handles (`Entity`) e funções (`spawn`, `despawn`, `query`, `get_position`, `set_position`, `get_velocity`, `set_velocity`, `random_float`, `random_int`, `step`, `digest`); handles geracionais sem use-after-free (`AIPO_RT_STALE_HANDLE`); buffer de comandos (`CommandBuffer`) com mutações estruturais (spawn/despawn) deferidas a safe points; PRNG determinístico com semente (`PoppyRng`); simulação headless em taxa fixa (`Simulation`); adaptador de VM expondo o módulo `poppy` sob a árvore de capabilities `poppy.*` (`poppy.ecs`, `poppy.random`); fixture de integração com script real de jogo (`tests/headless_demo.rs`) comprovando digest idêntico entre execuções repetidas com a mesma semente, variação sob sementes distintas, fault em handle obsoleto e negação de capability (`AIPO_RT_CAPABILITY_DENIED`).
- **Wave 4 — escape de escopo imposto nos pontos de publicação (P03-G01)**: um binding criado dentro de um callback de host escopado não pode sair do escopo, e a regra é verificada onde o valor se torna alcançável no heap — `SetGlobal`, `Return`, `SetField`, `SetIndex`, `BuildList` e `BuildDict` —, cada um nomeando o ponto na mensagem de `AIPO_RT_SCOPE_ESCAPE`. Fechar um escopo libera os handles que ele cunhou pela própria tabela geracional (a geração avança, então nenhum deles volta a resolver memória do host) e os lembra à parte, para que a tentativa de publicação reporte o fault específico em vez de uma leitura obsoleta. A verificação é pulada enquanto nenhum escopo fechou, então um programa sem bindings de host (todos os das Waves 1–3) paga um booleano por ponto de publicação. Testes dirigem o pipeline real para cada um dos seis opcodes.
- **Wave 4 — módulo `time` atrás da capability `clock` (P03-G01)**: `time.now()` (wall) e `time.monotonic()` como `Duration` em segundos, sem fallback — sem relógio instalado a leitura é fault `AIPO_RT_CAPABILITY_DENIED` nomeando a capability, nunca um zero silencioso. `time::install_clock`/`revoke_clock` governam o serviço; o perfil da CLI instala `SystemClock` (então `aipo run` lê os relógios do SO) e um perfil determinístico instala seu próprio `ClockSource`, que é o que faz um replay repetir. Paridade VM↔JS implementada: o entry emitido instala o relógio do sistema e um host que precise de determinismo injeta sua fonte em `globalThis.__aipoClock` antes do bundle carregar (ou `null` para negar). A suíte diferencial passa a rodar sob um perfil determinístico único dos dois lados — sem ele a comparação de paridade não teria sentido. Conformance: programa 28.
- **Wave 4 — ABI de host (`aipo-host`, `P03-G01`)**: novo crate com o contrato entre host e script: **AHS** (`HostSchema`) descrevendo módulos, tipos, valores, handles, funções, parâmetros, contratos de retorno, mutabilidade, async, docs, capabilities e depreciação como dado, com validação que reporta todos os problemas de consistência de uma vez; **capabilities deny-by-default** onde o caminho *é* a árvore (`clock` cobre `clock.wall`/`clock.monotonic`, `poppy` cobre `poppy.*`) e um conjunto declarado é limite superior que a política só pode estreitar; **handles geracionais** que nunca usam estado liberado (liberar incrementa a geração, então o handle antigo resolve como obsoleto e nunca para o valor que reusa o slot, com aposentadoria do slot em vez de wrap); **valores de host** por cópia, com `Int` em ±(2^53−1) e `Float` finito checados na fronteira; e **faults** com código estável (`AIPO_RT_CAPABILITY_DENIED`, `AIPO_RT_STALE_HANDLE`, `AIPO_RT_SCOPE_ESCAPE`). Sem dependência de engine e sem referência/lifetime Rust atravessando a fronteira. Adaptador completo no VM (`aipo-vm/src/host.rs`) e testes de integração de pipeline (`tests/host_scope_escape.rs` e `tests/host_capability_and_handles.rs`).
- **Wave 3 encerrada (P02-G01, P02-G02, P02-G03 → DONE)**: os três slices da Wave 3 estão certificados e o gate de saída de `docs/waves/wave-3-async.md` está cumprido (programas 24–27 e diagnósticos 20–29 verdes na VM e no Node; `AIPO_RT_CANCELLED` e `AIPO_RT_AWAIT_CYCLE` como faults commitados; fmt/clippy/test/doc verdes; `unsafe_code = "forbid"` e MSRV 1.85 inalterados). Pendências registradas, não escondidas: `Task[T]` (ADP-006 §G) e `Date`/`TimeOfDay`/`DateTime` (ADP-006 §D, pacote `timezone` da Wave 6).
- **Wave 3 infra assíncrona: sintaxe `async fn`, `await do` e diagnósticos (P02-G03)**: `async fn` como protocolo de chamada em declarações de topo, locais, anônimas e métodos de `impl` (o receptor viaja como argumento 0 do corpo spawnado); `await do … end` como açúcar sequencial sobre `await`; diagnósticos estáticos `AIPO_SEM_AWAIT_IN_SUBEXPRESSION`, `AIPO_SEM_FORGOTTEN_TASK` e `AIPO_SEM_NESTED_AWAIT_DO`; faults de runtime `AIPO_RT_AWAIT_CYCLE` e `AIPO_RT_CANCELLED` com paridade VM↔JS; endurecimento de literais numéricos com helper único em `aipo-lexer::number` (bases, separadores, limites de `i64`) usado pelo IR builder. Conformance: programas 24–27, diagnósticos 20–29.
- **Wave 3 combinadores assíncronos e scheduler (P02-G02)**: Scheduler cooperativo determinístico mono-thread com tempo virtual (`tick`), combinadores assíncronos (`task.spawn`, `task.sleep`, `task.all`, `task.race`, `task.timeout`, `task.cancel`, `task.group`), `group.spawn`, `group.wait`, suporte completo a `await` e detecção de ciclo de await (`AIPO_RT_AWAIT_CYCLE`), restrição de bloqueio em callbacks de host (`AIPO_RT_AWAIT_IN_CALLBACK`) e 100% de paridade diferencial VM↔JS (`aipo-runtime.js`).
- **Wave 3 tipos e valores (P02-G01)**: `Set` ordenado com semântica de conjunto (`has`, `add`, `remove`, `clear`, `to_list`, `lazy`); `Sequence` lazy iterável com pipeline (`map`, `filter`, `flat_map`, `take`, `skip`, `distinct`, `enumerate`) e terminais (`collect`, `find`, `any`, `all`, `count`, `reduce`, `group_by`); `Bytes` com buffer mutável (`Rc<RefCell<Vec<u8>>>`) e packing binário little-endian (`read_*`/`write_*` para `i8`..`f64`), mais `String.encode()` e `Bytes.decode()`; `Duration` com precisão em segundos, aritmética (`+`, `-`, comparação) e `.total_seconds()`; handles de `Task` e `Group` no modelo de valores com combinadores do módulo `task` (`spawn`, `sleep`, `all`, `race`, `timeout`, `cancel`, `group`); paridade estrita entre VM Rust e runtime JavaScript (`aipo-runtime.js`) comprovada pela suíte diferencial e selftest.
- **Deep quality gauntlet (P01-G02)**: `aipo-testkit` (Rng, AipoSmith generator,
  differential/metamorphic harnesses, portable subprocess watchdog),
  `aipo-bench` (frontend/VM/JS/scaling baselines → `docs/performance/baseline.md`),
  libFuzzer targets (`fuzz/`, nightly tier), LLVM coverage (`docs/testing/coverage.sh`),
  property suites (lexer, source, parser totality, values, contracts, unicode, fmt),
  UI diagnostic goldens + accessibility rubric/protocol, source-map conformance,
  determinism suite, resource-exhaustion suite, examples 06–24 with executable
  harness, programs 21–23, `deny.toml` supply-chain policy, ADPs 003/004/005.
  Suite: 165 → 253 testes, zero falhas; Miri verde em `aipo-vm`; MSRV 1.85 verificado.

### Corrigido
- **`await` de literal só falhava em runtime (P02-G03)**: um `Task` só nasce de chamada `async fn` ou combinador, então `await <literal>` é provável antes da execução e passa a reportar `AIPO_SEM_CONTRACT_VIOLATION_STATIC` (ADP-006 §G) em vez de exigir execução.
- **Recovery de contrato paramétrico comia o `)` (P02-G03)**: o skip de `Task[T]`/`List[Int]` consumia o fechamento do chamador e cascateava um `AIPO_PARSE_UNEXPECTED_TOKEN` falso depois do `AIPO_SEM_PARAMETRIC_CONTRACT` real.
- **`async fn` de topo não era um item (P02-G03)**: `is_item_start` omitia `TokenKind::Async`, então uma declaração `async fn` no topo era parseada como statement local — `AIPO_SEM_FORGOTTEN_TASK` nunca disparava para ela.
- **`async fn` anônima travava o parser (P02-G03)**: `async fn(...) … end` como valor não avançava além de `fn`.
- **Laço infinito ao dirigir uma `Task` descartada (P02-G03)**: dirigir/aguardar uma task cujo desfecho já fora consumido reexecutava o corpo indefinidamente; o desfecho passa a ser absorvido no boundary e o ciclo de `await` faulta (`AIPO_RT_AWAIT_CYCLE`), nos dois backends.
- **Métodos `async fn` de `impl` não spawnavam (P02-G03)**: `register_struct_method` propagava o flag `async` apenas para funções livres.
- **Abort do host em nesting profundo (SIGABRT)**: guardas de profundidade no
  parser + garantia de progresso + `AIPO_PARSE_NESTING_TOO_DEEP` (ADP-005);
  latente: loops de corpo travavam em qualquer falha sem consumo.
- **Panic do IR builder em programa válido**: construção com `init` dentro de
  função com parâmetros (achado do fuzzer); usa `hidden_name` (regressão: programa 23).
- **Divergência VM↔JS no recovery de `attempt`**: journal truncado até o frame
  sobrevivente (regressão: programa 21).
- **Divergência de zero com sinal**: `-0.0` no display e normalização de `Int`
  negativo inexistente no JS (regressão: programa 22).
- **Parsing quadrático de f-strings**: slices sem padding + shift de spans
  (`shift.rs`), 23,4 ms → 1,8 ms em 400 f-strings, snapshots idênticos.
- **Examples apodrecidos**: 03 (`return` em `invariant`), 04 (`fold`
  inexistente, pipelines liderando linha).
- **Fixture `programs/21_attempt_recovery_and_journal`**: `return fail(…)` propagando até o `attempt` do chamador e recovery descartando entradas do journal anteriores ao handler (só mutações pós-handler revertem) — cobre os dois backends.
- **Índice de chaves `String` no `Dict`** (`DictMap`): lookup/upsert O(1) no caso comum, preservando ordem de inserção e igualdade estrutural; espelhado no shim JS.
- **Fuzz gramatical** (`fuzz_smoke`): mutações por tokens (keywords, `end`-stripping, splice de programas) sobre `check`/`fmt --check`, mais execução real de mutantes sob `timeout` com assert de exit codes e ausência de panic.
- **Property tests do `aipo-js`**: determinismo da emissão e coerência do bundle sobre todo o corpus, mais `runtime/selftest.mjs` (propriedades da camada pura do shim sob `node`).
- **Documentação de distribuição**: `README.md` reescrito (quickstart, comandos, backends, testes), `CONTRIBUTING.md`, licença `LICENSE` (licença MIT do workspace).

### Corrigido
- **Emissor rejeita estouros de operandos**: contagens que não cabem em u16/u8 (nomes, constantes, itens, aridade, slots, capturas, alvos de salto) e funções desconhecidas agora são erro de compilação em vez de truncamento silencioso.
- **Paridade do journal no `attempt` (JS)**: o recovery passa a descartar até o `journal_start` do frame sobrevivente, como a VM — antes entradas pré-handler sobreviviam e o rollback divergia (fixture 21 prova).
- **Mensagem de `Overflow`**: inclui frames ativos e a causa usual (recursão profunda), nos dois backends.

### Removido
- **Variante morta `VmFault::InvalidDictKey` e código `AIPO_RT_INVALID_DICT_KEY`**: nada a produzia (qualquer `Value` é chave válida com igualdade `==`); catálogo atualizado.

### Refatorado
- **`aipo-vm/src/vm.rs` fatiado em `src/vm/`** (`mod`, `dispatch`, `call`, `journal`, `contract`, `method`, `failure`, `helpers`) sem mudança semântica — suíte completa verde antes e depois.
- **Contrato `aipo-vm`**: threading single-thread documentado como decisão (`Rc<RefCell>` ⇒ `!Send`), menções obsoletas a `gc-arena` removidas.
- **Performance — decisão sobre cache de metadata de métodos**: o protótipo de cache por site/tipo foi medido no workload `fields`, registrou 199.998 hits e 2 misses, mas não trouxe ganho reproduzível sob o runner compartilhado; foi revertido, mantendo os caches de fields e frame base. Evidências em `docs/performance/cross-language.md`.
- **Performance — pool de nomes de métodos**: as variantes experimentais com `Rc<str>` e `Owned`/`Shared` não mostraram ganho reproduzível e foram revertidas; a API pública de `BoundMethodData` permanece com `String`. Evidências em `docs/performance/cross-language.md`.

### Corrigido
- **Escopo de módulo (P00-G16)**: bindings `let`/`var` de topo de módulo agora são visíveis dentro de funções, métodos e `impl` — o padrão canônico `var counter` + `fn bump()` compila e executa. Causa: o `aipo-sema` analisava corpos de itens antes de declarar os bindings de topo.

### Adicionado
- **Funções locais (P00-G16)**: `fn nome(params) ... end` declarado dentro de uma função agora cria um binding local (canon: "Funções locais e closures — decidido"). O binding existe quando a execução alcança a declaração, o nome é visível no próprio corpo para autorrecursão e capturas lexicais seguem `let`/`var` (captura compartilhada de `var`). Novo opcode `FillSelfCapture` completa o handle de recursão: a captura de si mesma nasce com o sentinel `Unset` e é reescrita com o closure recém-criado. Fixtures `programs/19_local_functions` e `programs/20_module_scope`.
- **Wave 1 exit review (P00-G16)**: auditoria dos critérios de saída em `docs/waves/wave-1-mvp.md` contra o corpus, gauntlet 100% (conformance 13/13, formatter 11/11, fuzz 3/3, workspace 146/146), inventário verificado (20 programas, 19 diagnósticos, 8 formatações, 3 módulos) e non-delivery explícito (`Bytes` packing, `aipo-js`, `Set`/`Sequence`, LSP/REPL). Hand-off em `docs/evidence/P00-G16-wave-1-exit-review.md`.

## [0.1.0] - 2026-09-15

### Adicionado
- **Slice S1 (P00-G01)**:
  - Crate `aipo-source`: normalização UTF-8 NFC/BOM/CRLF, codificação de spans, cálculo de linha e coluna.
  - Crate `aipo-diagnostics`: catálogo estável de diagnósticos, emissores humano e JSON Lines (`--message-format=jsonl`).
- **Slice S2 (P00-G02)**:
  - Crate `aipo-lexer`: análise léxica completa, números sem avaliação precoce, strings (`f`, `r`, `fr`, `"""`), pipeline `|>`.
- **Slice S3 (P00-G03)**:
  - Crate `aipo-ast`: AST fortemente tipada para itens, declarações e expressões com spans preservados.
  - Crate `aipo-syntax`: parser recursivo descendente, parser Pratt de expressões, suporte a trailing blocks e recuperação de erros.
- **Slice S4 (P00-G04)**:
  - Crate `aipo-hir`: High-Level IR com desaçucaramento semântico de blocos em cauda (`do ... end`), pipelines (`|>`), e desestruturação em bindings.
- **Slice S5 (P00-G05)**:
  - Crate `aipo-sema`: resolução de escopos léxicos, verificação de caminhos de mutabilidade (`let` vs `var`, `!`), checagem estática de aridade e conformidade de interfaces (`satisfy`).
- **Slice S6 (P00-G06)**:
  - Crate `aipo-ir`: Core IR neutro de target com instruções sequenciais e preparação de pool de constantes.
  - Crate `aipo-bytecode`: conjunto compacto de opcodes, emissor, formato binário `aibc` v1, verificador estrutural e desassemblador.
- **Slice S7 (P00-G07)**:
  - Crate `aipo-vm`: máquina de pilha (stack machine) com despacho de instruções de bytecode, molduras de chamada (`CallFrame`), modelo de valores (`Value`), checagem estrita da faixa de inteiros ±(2^53 - 1) (`AIPO_RT_OVERFLOW`), floats finitos (`AIPO_RT_NON_FINITE_FLOAT`), divisão por zero (`AIPO_RT_DIV_ZERO`), variáveis globais e locais, e saltos condicionais/incondicionais.
- **Slice S8 (P00-G08)**:
  - Crate `aipo-vm`: coleções ordenadas `List` e `Dict` com suporte a indexação negativa, instanciação de `Struct` com imutabilidade de campos `fixed` e validação de `invariant()`, propagação automática de `Failure` (Modelo B), recuperação com `or_else` e blocos `attempt ... failed err ... end`, e diferenciação normativa de runtime faults incondicionais.
- **Slice S9 (P00-G09)**:
  - Crate `aipo-runtime`: registro de módulos (`ModuleGraph`, `ModuleRecord`, `ModuleState`), ordem de inicialização topológica determinística com desempate lexicográfico pelo caminho canônico, detecção de import circular (`AIPO_SEM_IMPORT_CYCLE`) e de dependência ausente (`AIPO_SEM_UNKNOWN_MODULE`), e catálogo de funções nativas (`NativeRegistry`).
  - Crate `aipo-stdlib`: Prelude V1 (`none`, `true`, `false`; `len`, `copy`, `same`, `some`, `fail`; conversões explícitas `Int`, `Float`, `Byte`, `String`), módulo `math` (`abs`, `min`, `max`, `floor`, `ceil`, `round` meio-para-longe-de-zero, `truncate`, `sqrt`, `pow`, `clamp`, constantes `pi`/`e`), módulo `string` (`len`, `byte_len`, `contains`, `starts_with`, `ends_with`, `find`, `lower`, `upper`, `capitalize`, `reverse` por grafemas com renormação NFC, `trim`, `split`, `join`, `replace`, `slice`, `format` com placeholders nomeados) e módulo `io` (`print`, `println` com sink capturável).
  - Regras de canon aplicadas e testadas: `split`/`replace` rejeitam padrão vazio, `join` recusa coerção textual implícita, campos vazios preservados, parsing textual estrito nas conversões, e separação estrita entre `Failure` recuperável (Modelo B) e runtime fault.
- **Completude de Backend (P00-G10)**:
  - Crate `aipo-vm`: valores de função de primeira classe e closures com upvalues, novos kinds `Byte`, `Bytes` e `Type`, testes de tipo, indexação por `Range` em `List`/`String`/`Bytes`, molduras de chamada cientes do receptor e enforcement de `AIPO_RT_MUTATION_DURING_ITERATION`.
  - Crate `aipo-bytecode`: tabela de `struct` no módulo binário, novos opcodes para parâmetros com default e prólogo callee-side, emissor, verificador e desassemblador cobrindo todo o conjunto.
  - Crate `aipo-ir`: declarações de `struct` carregadas no `Program`, construção `Type{...}` com campos em ordem canônica e resolução de argumentos nomeados no call site.
  - Crate `aipo-stdlib`: métodos de `List`/`Dict` ligados à VM (incluindo os de ordem superior `transform`, `filter` e `sort_by`, que chamam de volta em Aipo) e conversões delegando à VM para que `String(v)` e `io.print(v)` não divirjam.
  - Crate `aipo-syntax`: interpolação `f"..."` dessucarada para concatenação com `String(...)` e slices com limites omitidos (`a[..b]`, `a[a..]`, `a[..]`).
  - Crate `aipo-cli`: execução de módulos `import`/`export` com ordem topológica, init-once, privacidade por resolução de nomes, `AIPO_SEM_IMPORT_CYCLE` e `AIPO_SEM_UNKNOWN_MODULE`.
- **Slice S10 (P00-G11)**:
  - Crate `aipo-formatter`: formatação determinística e idempotente a partir do fluxo de tokens (indentação de quatro espaços, alinhamento de `end`, espaçamento de operadores, preservação de comentários).
  - Crate `aipo-cli`: superfície `aipo run <path>`, `aipo check <path>`, `aipo fmt <paths...> [--check]`, `aipo --version`, `aipo --help`, com códigos de saída `0`/`1`/`2` e diagnósticos em `--message-format=human|jsonl`.
- **Slice S11 (P00-G12)**:
  - Corpus de conformidade `docs/conformance/`: 11 programas com stdout commitado, 10 fixtures de diagnóstico com código esperado, 8 pares golden do formatador e 3 casos de módulos.
  - Suíte `aipo-cli` de conformidade (13 testes), fuzz smoke sem panic sobre o pipeline (3 testes) e rubrica de gauntlet documentada em `docs/conformance/README.md`.
  - Lacunas verificadas registradas em `docs/adp/ADP-002-construction-hooks-and-runtime-contracts.md` (`init`, `invariant()`, contratos em runtime e construção de `Bytes`) e encaminhadas para `P00-G13`.
- **Fechamento de contratos do MVP (P00-G13)**:
  - `Type{...}` passa a executar o hook `init` quando a `struct` declara um: o parser injeta o receiver implícito `self!`, o Core IR emite `BuildStruct` → chamada `Type.init` (argumentos alinhados à declaração, defaults pelo prólogo callee-side) → `Pop` → verificação de `invariant()` → `SealStruct`.
  - Campos `fixed` podem receber valor durante a construção (inclusive por `init`) e só são imutáveis depois da publicação (`StructInstance::under_construction` + opcode `SealStruct`).
  - `invariant()` é lowered para um predicado `<Type>.invariant(self) -> Bool` com as linhas combinadas por `and` e verificado ao fim da construção; `AssertInvariant` converte o resultado falso em `VmFault::InvariantViolation` (fault de contrato, conforme o error model do recorte MVP).
  - `Bytes(count)` passa a ser a forma canônica de construção de `Bytes` (bloco zerado, indexação por byte produzindo `Byte`, `len`), com limite provisório de alocação e `Failure` recuperável fora dele.
  - Corpus atualizado: `programs/12_bytes.aipo`, `programs/13_init_and_invariant.aipo` e `diagnostics/11_runtime_invariant_violation.aipo`.
- **Runtime contract enforcement (P00-G14)**:
  - `invariant()` passa a ser reavaliado nas fronteiras mutáveis estáveis (canon: `candidate -> aplicação provisória -> verificação -> commit`). Campos de instância publicada entram num *journal* por frame (`SetField` aplica provisoriamente e guarda o valor de entrada); o novo opcode `CheckMutations` verifica todas as instâncias participantes na fronteira — retorno de função/método ou statement de mutação do script de entrada — e, em falha, restaura os campos diretos de todas elas e produz uma `Failure` recuperável (capturável por `attempt`), preservando o valor de entrada.
  - O predicado compilado `<Type>.invariant` é resolvido por tipo a partir da tabela de funções do próprio módulo (`Vm::run`), então a verificação de mutação e a de construção usam o mesmo código, sem um segundo avaliador de expressões.
  - Contratos de assinatura (`name: Type`, `name!: Type`, `-> T`, `T?`) passam a ser verificados em runtime pelo novo opcode `AssertContract`: parâmetros no prólogo do callee (depois do prólogo de defaults, cobrindo também o valor de um default) e retornos antes de cada `return expr`. `none` só satisfaz `T?`; `Function` exige valor chamável; o nome de uma `struct` é comparado com o tipo da instância; uma violação descoberta em runtime é `VmFault::ContractViolation` (fault, não capturável por `attempt`); um contrato que nomeia uma interface é aceito, porque o recorte MVP não tem checagem estrutural em runtime.
  - Corpus atualizado: `programs/14_signature_contracts.aipo`, `programs/15_invariant_on_mutation.aipo`, `diagnostics/12_runtime_contract_violation.aipo`, `diagnostics/13_runtime_return_contract.aipo` e `diagnostics/14_runtime_invariant_mutation_uncaught.aipo`.
  - `docs/adp/ADP-002-construction-hooks-and-runtime-contracts.md` fica **resolved**: os cinco gaps encontrados pelo corpus de S11 (G1, G2, G2b, G3, G4) estão fechados e certificados.
- **Static contract reporting e conformidade estrutural de interfaces (P00-G15)**:
  - `aipo-sema` passa a usar as anotações escritas para um relatório **antes da execução**: um argumento literal que não pode satisfazer um contrato de parâmetro de tipo core, ou `none` contra um contrato não anulável, vira o novo diagnóstico `AIPO_SEM_CONTRACT_VIOLATION_STATIC` (também para `return` literal contra `-> T`). Um descompasso que o analyzer não consegue provar continua sendo contract fault em runtime, no limite da chamada.
  - Conformidade estrutural de interface em runtime deixa de ser aceitação cega: o contrato carrega as operações que a interface declara e checa se o valor as expõe. **Correção de defeito real**: a aridade emitida passou a ser a visível no call site (o receiver não é argumento ali), alinhada à convenção já usada por métodos nativos e `BoundMethod`; antes a interface contava o receiver e reprovava até os valores conformes. A mensagem de falha passa a nomear o tipo declarado da `struct` em vez do kind genérico do runtime.
  - NFC passa a ser garantida nas fronteiras de construção de `String` (ADP-001 Q5): decodificação de literal no lexer, conversão `String(value)`, concatenação `+` (para onde a interpolação abaixa) e as operações que podem juntar base + marca (`lower`, `upper`, `capitalize`, `replace`, `join`, `format`). Operações puramente substring preservam o invariante de graça; `==` continua igualdade estrutural.
  - ADP-001 Q3 fechada: `math.clamp` com limites invertidos permanece `Failure` recuperável, pelo critério que o próprio canon usa para separar valor fora de faixa (`Failure`) de índice fora de faixa (fault). ADP-001 Q4 fechada: slice fora da faixa é tolerante para `List`, `String` e `Bytes` (canon: "slices fora da faixa são tolerantes/clamped, ao contrário de índices exatos").
  - Correção de flakiness na suíte de conformidade: o sink de `io` é global, então a suíte passa a serializar **toda** invocação do CLI, não só a que captura — antes outra execução concorrente contaminava um snapshot regenerado.
  - Corpus atualizado: `programs/16_interface_contracts.aipo`, `programs/17_unicode_nfc.aipo`, `programs/18_tolerant_slices_and_clamp.aipo`, `diagnostics/15_sem_contract_violation.aipo`, `diagnostics/16_sem_return_contract.aipo`, `diagnostics/17_runtime_interface_contract.aipo`, `diagnostics/18_runtime_interface_arity.aipo` e `diagnostics/19_runtime_clamp_inverted_bounds.aipo` (18 programas, 19 diagnósticos, 8 formatações e 3 casos de módulo).
  - `docs/adp/ADP-001-byte-and-core-types-as-values.md` fica **resolved**: todas as perguntas (Q1–Q5) fechadas, cada uma com a fonte no canon, a decisão e o fixture que certifica.
- Bateria de testes unitários exaustivos cobrindo todo o pipeline (146 testes automatizados 100% aprovados, zero warnings em lints/clippy e `unsafe_code = forbid`).
