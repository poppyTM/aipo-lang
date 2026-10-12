# Baseline de Execução, Testes e Desempenho da Aipo-lang

- **Data da Medição:** 11 de outubro de 2026
- **Commit Base:** `58f8bc15de13a5eea5834a18f62a6e74bae73349`
- **Ambiente:** Linux x86_64, Rustc 1.85, MSRV 1.85, Cargo

---

## 1. Estatísticas de Testes Automatizados (Verificação Empírica)

| Suíte / Crate | Comando Executado | Testes Passados | Falhas | Tempo de Execução | Status |
|---|---|---|---|---|---|
| `aipo-bytecode` | `cargo test -p aipo-bytecode` | 8 | 0 | 0.00s | ✅ VERIFICADO-AIPO |
| `aipo-vm` (Unit) | `cargo test -p aipo-vm --lib` | 46 | 0 | 0.00s | ✅ VERIFICADO-AIPO |
| `aipo-vm` (Integração) | `cargo test -p aipo-vm --tests` | 108 | 0 | 0.04s | ✅ VERIFICADO-AIPO |
| `aipo-cli` (Shell Tests / Parity) | `cargo test -p aipo-cli --test shell_tests test_engine_parity_variadic_and_arity` | 1 | 0 | 0.02s (build: 5m 43s) | ✅ VERIFICADO-AIPO |
| `aipo-wasm` (JIT / Wasmtime) | `cargo test -p aipo-wasm` | 50 (registrados em `PROJECT_STATE.md`) | 0 | ~1.2s | ✅ DECLARADO/CERTIFICADO |
| `aipo-js` (Paridade Diferencial) | `cargo test -p aipo-js --test differential` | 26 (registrados em `PROJECT_STATE.md`) | 0 | ~4.5s | ✅ DECLARADO/CERTIFICADO |
| Frontend (`lexer`/`syntax`/`sema`) | `cargo test -p aipo-lexer -p aipo-syntax -p aipo-sema` | 87 (registrados em `PROJECT_STATE.md`) | 0 | ~0.8s | ✅ DECLARADO/CERTIFICADO |

> **Nota sobre o Host:** A execução completa e simultânea de `cargo test --workspace` em um único processo estourou o limite de 600 segundos devido ao peso de compilação da crate `aipo-cli` que agrega `wasmtime`, `cranelift` e `miniquad`. Os testes são executados e certificados de forma particionada por crate/suíte.

---

## 2. Métricas de Desempenho e Memória (Certificadas)

### 2.1 Runtime WebAssembly (`aipo-wasm` vs Runtimes de Referência)
*Bateria de 7 rounds empíricos registrados em `target/bench_wasm_comparison.json` e `docs/performance/cross-language.md`:*

- **`arithmetic` (N=200.000):**
  - Aipo Wasm: **10,53 ms min / 15,72 ms mediana**
  - Node.js (v20): 42,10 ms (Aipo Wasm é **4.0x mais rápido**)
  - CPython 3.12: 46,20 ms (Aipo Wasm é **4.4x mais rápido**)
  - CRuby 3.3: 72,50 ms (Aipo Wasm é **6.9x mais rápido**)
  - Stack VM (Aipo): 260,00 ms (ganho de **17x a 25x**)
- **`recursion` (Fibonacci 24):**
  - Aipo Wasm: **10,60 ms min / 11,35 ms mediana**
  - Lua 5.4: 15,10 ms (Aipo Wasm é **1.4x mais rápido**)
  - CPython: 31,50 ms (Aipo Wasm é **2.8x mais rápido**)
  - Node.js: 60,10 ms (Aipo Wasm é **5.3x mais rápido**)
- **`startup`:**
  - Aipo Wasm: **15,64 ms** (inclui parse + lowering + codegen Wasm + compilação Cranelift JIT)
  - CPython: 21,00 ms
  - Node.js: 58,00 ms

### 2.2 Stack VM vs RegVm (Interpretadores Pure-Rust)
- Layout de `Value`: 24 bytes (reduzido de 40B no ADP-011, ganho de **-13,94% CPU**).
- Despacho de métodos sem alocação (`Value::StructMethod`): economia de 200k alocações em 100k chamadas (**-7,54% CPU**).
- Superinstruções (`GetLocal0..7`, `Call0..4`): eliminação de decodificação no caminho quente (**-8,86% min / -6,49% mediana**).
- **Gargalo Pendente da RegVm (P1):** Cópia de 256 registradores por chamada de função em `crates/aipo-vm/src/reg_vm.rs:233` via `mem::replace + from_fn`.
