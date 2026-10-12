# AIPO-OMNI-002 — Instruction Budget & Wasm Fuel Metering (Uncatchable Sticky Fault)

- **Status:** CONCLUÍDO & CERTIFICADO (Fase 1 / P0 Piloto 2)
- **Origem / Referência:** omniLua (`crates/lua-vm/src/state.rs:SandboxLimits` e `docs/SANDBOXING_EXPLORATION.md`)
- **Classificação:** `ADAPT` (P0, Score +37)

---

## 1. Problema Identificado na Aipo

1. **Wasmtime JIT sem Medição de Fuel:** A execução de WebAssembly via `aipo-wasm` (JIT Wasmtime) não configurava `consume_fuel(true)` nem limitava combustível no `wasmtime::Store`. Loops infinitos em Wasm congelavam o processo indefinidamente.
2. **Falta de Interface Unificada no CLI:** A `aipo-cli` não expunha flags para governar o orçamento de execução (`--budget` / `--max-instructions`) de forma uniforme entre os backends.
3. **Garantia de Não-Interceptabilidade:** Conforme a semântica do Modelo B+, runtime faults (`VmFault::Overflow`) nunca devem ser capturados por blocos `attempt { ... }`, desbobinando incondicionalmente a pilha até o host.

---

## 2. Implementação Realizada

### A. Substrato Wasm (`crates/aipo-wasm`)
- Em `crates/aipo-wasm/src/runner.rs`:
  - Implementada a função `execute_wasm_with_fuel(wasm_bytes: &[u8], stdout: &mut dyn Write, fuel: Option<u64>)`.
  - Configurado `wasmtime::Config::consume_fuel(true)` e `store.set_fuel(limit)` quando `fuel.is_some()`.
  - Mapeadas mensagens de trap de esgotamento de combustível do Wasmtime (`all fuel consumed by WebAssembly`, `out of fuel`) para o diagnóstico canônico da Aipo:
    `[AIPO_RT_OVERFLOW] runtime fault [AIPO_RT_OVERFLOW]: execution budget exceeded: fuel limit reached`.
  - `execute_wasm` preservado como wrapper chamando `execute_wasm_with_fuel(..., None)`.
- Exportada em `crates/aipo-wasm/src/lib.rs`.
- Adicionado teste adversarial em `crates/aipo-wasm/tests/pipeline_tests.rs`: `test_wasm_fuel_budget_halts_infinite_loop`.

### B. Interface Unificada na CLI (`crates/aipo-cli`)
- Em `crates/aipo-cli/src/lib.rs`:
  - `USAGE` atualizado documentando `--budget=<N>` e `--max-instructions=<N>`.
  - `Command::Run` e `struct ExecutionOptions` estendidos com `budget: Option<u64>`.
  - `Command::parse` configurado para aceitar `--budget=<N>`, `--budget <N>`, `--max-instructions=<N>` e `--max-instructions <N>`, rejeitando uso fora de `run`, valores inválidos e flags duplicadas.
  - Orçamento propagado de forma idêntica para:
    1. **WebAssembly JIT:** repassado para `aipo_wasm::execute_wasm_with_fuel(&bytes, out, budget)`.
    2. **Register VM (`--engine=reg`):** repassado chamando `reg_vm.set_max_instructions(budget)`.
    3. **Stack VM (padrão):** repassado chamando `vm.set_max_instructions(budget)`.
    4. **Bytecode pré-compilado (`.aibc`):** repassado chamando `vm.set_max_instructions(budget)`.
- Em `crates/aipo-cli/tests/wasm_cli.rs`:
  - Adicionado `test_cli_instruction_budget_halts_infinite_loops`: valida interrupção de laços infinitos e emissão de `AIPO_RT_OVERFLOW` nos três motores (Wasm JIT, Stack VM e Reg VM).
  - Adicionado `test_cli_budget_happy_path_completes`: valida conclusão com código de saída 0 quando o orçamento é suficiente.
  - Adicionado `test_cli_budget_flag_validation`: valida mensagens de erro em flags inválidas, duplicadas ou comandos não suportados.

---

## 3. Evidência de Qualidade e Conformidade

- **Testes Wasm:** 101 testes passando (0 falhas) em `crates/aipo-wasm`.
- **Testes VM:** 154 testes passando (0 falhas) em `crates/aipo-vm`.
- **Testes CLI Conformance:** 23 testes passando em `crates/aipo-cli --test conformance`.
- **Testes CLI Shell:** 13 testes passando em `crates/aipo-cli --test shell_tests`.
- **Testes CLI Wasm & Budget:** 11 testes passando em `crates/aipo-cli --test wasm_cli`.
- **Clippy e Formatação:** 100% verde com `-D warnings` e `cargo fmt --all --check`.
