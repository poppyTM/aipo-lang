# AIPO-OMNI-003 — API de Embedding Rust Scoped (`AipoEngine::scope` & Safe UserData)

- **Status:** CONCLUÍDO & CERTIFICADO (Fase 1 / P0 Piloto 3)
- **Origem / Referência:** omniLua (`Lua::scope`, `Scope`, `create_userdata_ref_mut` em `crates/lua-rs-runtime/src/lib.rs`)
- **Classificação:** `ADAPT` (P0, Score +28)

---

## 1. Problema Identificado na Aipo

1. **Dependência de Singletons Globais em Hosts:**
   Em integrações de motor de jogos como `aipo-poppy`, a comunicação entre o host Rust e os scripts Aipo dependia de um mutex global de processo (`static POPPY: LazyLock<Mutex<Option<PoppyService>>>`) e serialização manual (`POPPY_LOCK`). Isso impedia a execução de múltiplas instâncias concorrentes de runtime em threads distintas.
2. **Ausência de Fachada Segura em Rust:**
   Embedders precisavam lidar diretamente com detalhes internos da `Vm` (`open_scope`, `close_scope`, registro manual de structs/métodos de bytecode, despacho de natives). Não existia uma API idiomática com garantia estática em tempo de compilação de que dados emprestados do host não sobrevivem à execução.
3. **Garantia de Não-Fuga de Referências:**
   Era necessário garantir que:
   - Em Rust, referências ou handles geracionais de escopo não possam ser armazenados fora da closure `scope`.
   - Na VM, qualquer tentativa do script de guardar o handle em variáveis globais ou coleções persistentes dispare imediatamente a falha canônica `[AIPO_RT_SCOPE_ESCAPE]`.
   - Zero código `unsafe`: manter estritamente `#![forbid(unsafe_code)]` em todas as crates produtivas.

---

## 2. Implementação Realizada

### A. Tabela Geracional de UserData no Contexto do Host (`crates/aipo-vm/src/host.rs`)
- `HostContext` estendido com `userdata: HandleTable<Box<dyn std::any::Any>>`.
- Implementadas as operações tipadas de ciclo de vida:
  - `hand_out_userdata<T: 'static>(&mut self, value: T) -> Handle`: registra o objeto na tabela geracional e o associa ao escopo ativo.
  - `resolve_userdata<T: 'static>(&self, handle: Handle) -> Result<&T, VmFault>`: resolve referência imutável, validando escape de escopo, geração e tipo (downcast).
  - `resolve_userdata_mut<T: 'static>(&mut self, handle: Handle) -> Result<&mut T, VmFault>`: resolve referência mutável exclusiva.
  - `release_userdata(&mut self, handle: Handle) -> Option<Box<dyn Any>>`: invalida o slot imediatamente.
- `close_scope(scope_id)` atualizado para remover e descartar atomicamente todos os objetos da tabela de userdata criados no escopo, marcando seus handles no conjunto `escaped`.

### B. Fachada de Embedding e Escopo RAII (`crates/aipo-vm/src/engine.rs`)
- **`AipoEngineBuilder`**: Configuração fluente de capacidades (`CapabilitySet`) e quotas de instrução (`limits`, `max_instructions`).
- **`AipoEngine`**:
  - `engine.scope(|scope| { ... })`: abre um escopo RAII. Garante que, mesmo se o corpo do escopo retornar um `Err`, a limpeza e invalidação de todos os handles de escopo é executada no fechamento.
  - `register_module_declarations(&mut self, module: &BytecodeModule)`: registra automaticamente todas as definições de structs e métodos declarados no módulo de bytecode, eliminando boilerplate nos hosts.
  - `register_host_fn(&mut self, name, arity, callback)`: registra callbacks nativos na VM.
  - `run(&mut self, module)`: executa módulos compilados.
- **`AipoScope<'scope, 'engine>`**:
  - Delimitado pelo lifetime `'scope`.
  - `insert_userdata<T: 'static>(&mut self, value: T) -> ScopedHandle<'scope>`.
  - `resolve_userdata<T>` e `resolve_userdata_mut<T>`.
  - `expose_global(&mut self, name, handle)`: define o handle no ambiente global da VM.
  - `register_host_fn`: registra funções nativas vinculadas ao escopo.
  - `sub_scope`: suporte a sub-escopos aninhados com isolamento estrito entre escopos pai e filho.
- **`ScopedHandle<'scope>`**:
  - Envolve o `Handle` geracional com a tag de lifetime `'scope`. O borrow checker do Rust rejeita estaticamente em tempo de compilação qualquer tentativa de retornar ou salvar o handle fora da closure do escopo.

---

## 3. Evidência de Qualidade e Conformidade

### A. Suíte de Testes de Integração (`crates/aipo-vm/tests/engine_scope_tests.rs`)
9 testes de integração automatizados cobrindo todos os cenários da matriz:
1. `test_engine_builder_and_limits`: validação de builder, concessão de capabilities e interrupção por fuel.
2. `test_scope_userdata_mut_happy_path`: mutação de estado de `GameWorld` via callbacks nativos dentro do escopo e invalidação pós-escopo.
3. `test_scope_userdata_invalidated_even_when_body_errors`: garantia de desmontagem RAII mesmo após erros.
4. `test_nested_scopes_isolated`: isolamento de escopos aninhados (fechar escopo interno não invalida handles do escopo externo).
5. `test_scope_type_mismatch_fault`: emissão segura de `AIPO_RT_TYPE_MISMATCH` em tentativas de downcast incompatível.
6. `test_scope_shared_rc_interior_mutability`: empréstimo de estruturas compartilhadas com `Rc<RefCell<T>>`, mantendo o estado mutado no host após descarte do clone do escopo.
7. `test_engine_register_module_declarations`: registro automático de structs e métodos.
8. `test_scope_escape_caught_by_publication_check`: barreira dinâmica da VM disparando `AIPO_RT_SCOPE_ESCAPE` quando um script tenta publicar um handle de escopo para uma variável global pós-escopo.
9. `test_scope_callback_fault_does_not_panic`: propagação controlada de `VmFault` através da fronteira nativa sem pânico.

### B. Canário de Steady-State de Memória (`crates/aipo-testkit/tests/leak_canaries.rs`)
- Adicionado o cenário 8 (`engine_scope_churn`): 32 iterações contínuas de criação de `AipoEngine`, abertura de `scope`, inserção de userdata, registro de callback nativo, execução de script e descarte.
- Resultado: **`growth = 0 bytes`** (saldo líquido de alocação rigorosamente zero).

### C. Portões de Compilação e Estilo
- `#![forbid(unsafe_code)]` 100% preservado.
- `cargo clippy -p aipo-vm -p aipo-testkit -- -D warnings`: 0 warnings.
- `cargo fmt --all --check`: 100% aderente.
