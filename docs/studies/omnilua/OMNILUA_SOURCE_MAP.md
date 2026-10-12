# omniLua — Mapeamento de Fontes e Arquitetura de Referência

- **Repositório:** [ianm199/omnilua](https://github.com/ianm199/omnilua)
- **Commit Fixado:** `90dcf85a8cfb95305b77056af00a114fd0df30a1`
- **Licença:** MIT (Copyright (c) 2026 lua-rs-port contributors) com NOTICE da PUC-Rio (Lua 5.4.7).
- **Invariante Declarada:** `#![forbid(unsafe_code)]` na raiz do workspace, com oráculo automatizado contra Lua 5.1–5.5.

---

## 1. Mapeamento de Crates e Módulos de Interesse

| Crate omniLua | Caminho dos Fontes | Símbolos / Mecanismos Centrais | Problema que Resolve | Relevância / Potencial para Aipo |
|---|---|---|---|---|
| `lua-gc` | `crates/lua-gc/src/heap.rs` | `Heap`, `Gc<T>`, `HeapGuard`, `Marker`, `StepBudget`, `StepOutcome`, `detached_allocations` | Coletor mark-sweep incremental em Pure Rust com pacer de alocação e garantias estritas contra alocações destacadas (`HeapGuard`). | **INSPIRE / ADAPT:** Modelo de contabilidade de alocações e leak canaries para testes de ciclo de vida de VM. |
| `lua-rs-runtime` | `crates/lua-rs-runtime/src/lib.rs` | `Lua`, `Lua::scope`, `Scope`, `create_userdata_ref_mut`, `SandboxConfig` | API de embedding em Rust de alto nível com empréstimos de lifetime limitado (`Lua::scope`), impedindo use-after-free sem FFI. | **ADAPT:** Padrão ergonômico de empréstimos delimitados por escopo para a API de host em Rust (`AipoEngine::scope`). |
| `lua-rs-runtime` (Sandbox) | `crates/lua-rs-runtime/tests/sandbox.rs`, `docs/SANDBOXING_EXPLORATION.md` | `SandboxLimits`, `install_sandbox_limits`, `sandbox_charge_interval`, `aborting` flag | Sandbox contra código adversário: instruction budget spanning corrotinas, memory ceiling amostrado, aborto ininterceptável por `pcall`. | **ADAPT (Prioridade P0):** Orçamento compartilhado persistente e flag de aborto não capturável por blocos `attempt { ... }` na Aipo. |
| `lua-vm` | `crates/lua-vm/src/state.rs`, `vm.rs` | `GlobalState`, `trace_exec`, `trap`, `hook_mask` | Dispatcher em registrador com verificação de gancho sem custo no caminho quente (`hook_mask == 0`). | **ADAPT:** Estratégia de tarifação periódica de fuel (por exemplo, a cada $N$ instruções) em vez de branch a cada opcode. |
| `lua-types` | `crates/lua-types/src/lib.rs`, `value.rs` | `LuaValue`, `LuaString`, tagged representation, interned strings | Representação compacta de valores dinâmicos e manipulação de strings em Pure Rust. | **INFORMATIVO:** Aipo opera sobre UTF-8 rigoroso; strings de Lua são sequências de bytes arbitrárias. |
| `lua-stdlib` | `crates/lua-stdlib/src/lib.rs`, `base.rs` | `HostHooks`, capability stripping, isolamento de IO e OS | Isolamento de funções com efeitos colaterais através de remoção de capabilities na inicialização. | **VERIFICADO:** Aipo já possui `aipo-host::CapabilitySet` superior (árvore hierárquica deny-by-default). |
| `lua-wasm` | `crates/lua-wasm/src/lib.rs` | Execução Wasm via navegador e Node sem Wasmtime runtime pesado | Runner Wasm leve para ambiente browser/web. | **INSPIRE:** Alternativa para o perfil `nano` da Aipo sem arrastar dependência de compilação do Wasmtime. |
| `harness` | `harness/canaries/`, `tests/leak_canaries.rs` | Alocador global com contagem atômica de bytes (`CountingAllocator`) | Validação empírica de net-zero live bytes após churn de criação e destruição de VMs e scripts. | **ADAPT (Prioridade P0):** Canários de vazamento de memória para testes de ciclo de vida do runtime Aipo. |

---

## 2. Destaques Técnicos para a Aipo

### 2.1 Sandboxing Uncatchable (Ininterceptável)
No omniLua (`docs/SANDBOXING_EXPLORATION.md`), o maior desafio resolvido foi impedir que um script adversário anulasse o estouro do budget capturando o erro em um loop de `pcall`:
```lua
while true do
  pcall(function() while true do end end)
end
```
**A solução:** Uma flag sticky `aborting` no estado global da VM. Quando o budget de instruções estoura, o despachante entra em estado de aborto irrevogável; funções protegidas re-lançam o erro imediatamente em vez de retornar status de captura.
- **Aplicação na Aipo:** O modelo de tratamento de falhas da Aipo (Modelo B+) distingue `Failure` (recuperável com `attempt`) de `Fault` (invariante violado). O esgotamento de budget deve disparar um `Fault` específico (`AIPO_RT_BUDGET_EXHAUSTED`) que seja blindado contra absorção por `attempt`.

### 2.2 Scope API e Empréstimos Temporários de Host
O `Lua::scope` permite emprestar uma estrutura complexa do host Rust (por exemplo, `&mut World` em um jogo) para ser manipulada por um script Aipo durante a duração da função de escopo, garantindo via RAII e lifetimes que referências residuais guardadas em variáveis globais ou closures falhem com segurança (`AIPO_RT_SCOPE_ESCAPE` ou stale handle) caso sejam acessadas após o encerramento do escopo.
- **Aplicação na Aipo:** Conectar com a infraestrutura já existente de `crates/aipo-host` (`HandleTable`) para fornecer uma API nativa Rust `engine.scope(|scope| { ... })`.

### 2.3 Counting Global Allocator (Leak Canaries)
O omniLua valida que o runtime descarta 100% de sua memória através de um wrapper de alocador global em testes:
```rust
#[global_allocator]
static ALLOC: CountingAllocator = CountingAllocator::new();
```
Ele afere que, ao término de `create_vm -> run_scripts -> destroy_vm`, a contagem de bytes vivos adicionais alocados na heap retorna rigorosamente a zero.
- **Aplicação na Aipo:** Integrar canários equivalentes na suíte de testes de integração (`crates/aipo-testkit`) para comprovar ausência de leaks cíclicos de `Rc<RefCell<...>>`.
