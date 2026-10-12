# AIPO-OMNI-001 — Canários de Vazamento de Memória (`CountingAllocator` & Steady-State Lifecycle)

- **Status:** CONCLUÍDO & CERTIFICADO (Fase 1 / P0 Piloto 1)
- **Origem / Referência:** omniLua (`crates/lua-rs-runtime/tests/leak_canaries.rs` e `CountingAllocator`)
- **Classificação:** `ADAPT` (P0, Score +38)

---

## 1. Problema Identificado na Aipo

1. **Ausência de Canários de Steady-State:** A Aipo utiliza contagem de referência com `Rc<RefCell<...>>` para gerenciar objetos (closures, structs, coleções). Apesar de proteções locais como o `CycleGuard`, a base de código não possuía nenhum teste automatizado com instrumentação de alocador global capaz de verificar se a criação, execução e descarte contínuo de VMs e compiladores resultam em retenção zero de memória no heap.
2. **Descoberta de Ciclo `Rc` Oculto (160 bytes por iteração):**
   Ao ativar o canário de vazamento de memória em `aipo-testkit`, foi imediatamente detectado um vazamento constante de exatamente **160 bytes por iteração** (5.120 bytes em 32 iterações) em closures locais.
   - **Causa Raiz:** Em `crates/aipo-ir/src/builder.rs` (antiga linha 1046), toda função local declarada (`Stmt::Function`) era incondicionalmente envolvida com `self_capture`, emitindo um `Opcode::FillSelfCapture`. Isso forçava a injeção da própria closure em uma célula de upvalue dentro do seu próprio vetor de capturas (`ClosureData -> upvalue cell -> ClosureData`), criando um ciclo forte de `Rc` mesmo para funções puras ou não-recursivas!

---

## 2. Implementação Realizada

### A. Correção do Ciclo de Upvalues no Compilador IR (`crates/aipo-ir`)
- Em `crates/aipo-ir/src/builder.rs`:
  - Implementada a função estática `references_name(expr: &Expr, target: &str) -> bool`, inspecionando a AST da função para determinar se o corpo realmente faz auto-referência ao seu identificador.
  - A auto-captura (`self_capture`) e a instrução `FillSelfCapture` agora são geradas **exclusivamente quando a função é comprovadamente recursiva**.
  - Funções locais não recursivas têm suas upvalues desalocadas imediatamente e limpas sem qualquer ciclo de `Rc`.
  - **Resultado:** O vazamento de 160 bytes/iteração caiu para **exatamente 0 bytes**!

### B. Canários de Vazamento de Memória (`crates/aipo-testkit`)
- Em `crates/aipo-testkit/Cargo.toml`:
  - Adicionadas dependências de `aipo-host` e `aipo-wasm`.
  - Configurado lint `unsafe_code = "allow"` no nível de testes/binário auxiliar (necessário para a implementação da trait `std::alloc::GlobalAlloc`), preservando estritamente `#![forbid(unsafe_code)]` na biblioteca de produção `src/lib.rs`.
- Em `crates/aipo-testkit/tests/leak_canaries.rs`:
  - Implementado `CountingAllocator` com contadores atômicos `ALLOCATED_BYTES` e `DEALLOCATED_BYTES`, rastreando `live_bytes()` com precisão de byte único.
  - Implementado harness `assert_steady_state(name, scenario)` executando 4 iterações de aquecimento (warmup) seguidas de 32 iterações de churn contínuo com verificação `growth <= TOLERANCE` (tolerância = 0 bytes).
  - Cobertura de 7 cenários completos de ciclo de vida:
    1. **`vm_churn`:** instanciação de `Vm`, execução de scripts com dicionários, strings e coleções, e descarte.
    2. **`closure_churn`:** instanciação de closures de alta ordem capturando variáveis locais em escopos aninhados, execução e descarte.
    3. **`reg_vm_churn`:** compilação para bytecode da Register VM, execução na `RegVm` e descarte de instâncias e módulos.
    4. **`frontend_churn`:** análise sintática, rebaixamento para HIR e verificação semântica (`aipo-sema`) de interfaces e métodos.
    5. **`compiler_churn`:** compilação completa de código-fonte para módulo bytecode verificado e seu descarte.
    6. **`wasm_compile_churn`:** compilação de código HIR para binário WebAssembly e desmontagem para WAT via `aipo-wasm`.
    7. **`handle_table_churn`:** alocação e desalocação massiva de handles geracionais em `aipo-host::HandleTable`.

---

## 3. Evidência de Qualidade e Resultados

Execução do teste de canários:
```bash
$ cargo test -p aipo-testkit --test leak_canaries -- --nocapture
vm_churn: baseline=5516, final=5516, growth=0
closure_churn: baseline=6859, final=6859, growth=0
reg_vm_churn: baseline=6859, final=6859, growth=0
frontend_churn: baseline=6859, final=6859, growth=0
compiler_churn: baseline=6859, final=6859, growth=0
wasm_compile_churn: baseline=6859, final=6859, growth=0
handle_table_churn: baseline=6859, final=6859, growth=0
test test_embedding_lifecycle_is_steady_state ... ok
```

- **Crescimento Líquido de Memória:** **0 bytes** em todos os 7 subsistemas testados.
- **Detecção Prática:** Comprovou a eficácia do padrão omniLua ao capturar e erradicar um vazamento real de `Rc` na base de código da Aipo.
- **Portões de Qualidade:** Clippy aprovado sem warnings (`-D warnings`), `cargo fmt` 100% aderente, `#![forbid(unsafe_code)]` intacto nas crates de produção.
