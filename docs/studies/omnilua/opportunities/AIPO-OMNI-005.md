# AIPO-OMNI-005 — Janelas de Registrador na `RegVm` (Otimização de Call Frames)

- **Status:** CONCLUÍDO & CERTIFICADO (Fase 2 / P1 Piloto 1)
- **Origem / Referência:** omniLua & Lua 5.x (`CallInfo` e registradores mapeados por janela contígua em pilha de execução)
- **Classificação:** `ADAPT` (P1, Score +18)

---

## 1. Problema Identificado na Aipo

1. **Cópia Cega de 4 KiB por Ativação de Função:**
   Em `crates/aipo-vm/src/reg_vm.rs:invoke_with_upvalues`, cada chamada de função do usuário na máquina de registradores executava:
   ```rust
   let saved_registers =
       std::mem::replace(&mut self.registers, std::array::from_fn(|_| Value::None));
   ```
   e no retorno:
   ```rust
   self.registers = saved_registers;
   ```
   Como `self.registers` é um array fixo de `[Value; 256]` (onde cada `Value` ocupa 16 bytes), isso gerava:
   - 4.096 bytes (4 KiB) copiados para a pilha do Rust na chamada;
   - 256 iterações escrevendo `Value::None` para zerar a máquina;
   - 4.096 bytes (4 KiB) movidos de volta no retorno;
   - Uma pegada excessiva na pilha do Rust ($64 \times 4\text{ KiB} = 256\text{ KiB}$ no limite de profundidade de recursão).
2. **Desperdício em Funções de Baixa Aridade:**
   Na vasta maioria dos programas reais (e funções recursivas como Fibonacci ou mergesort), as funções compiladas utilizam apenas 2 a 6 registradores locais (`num_registers` registrado em `RegCompiledFunction`). Copiar 256 registradores quando apenas 2 ou 3 foram modificados representava um overhead inútil de mais de 98% por frame de ativação.

---

## 2. Implementação Realizada

### A. Pilha de Janelas de Registrador (`register_stack: Vec<Value>`)
Em vez de salvar e zerar o array completo de 256 registradores, a `RegVm` agora utiliza uma pilha dinâmica de janelas preservadas:
- Cada função compilada declara seu número exato de registradores exigidos (`num_registers = max_reg + 1`).
- O tamanho da janela ativa a ser preservada para o chamador é calculado precisamente:
  ```rust
  let callee_regs = func.num_registers.max(func.arity).max(args.len()).min(256);
  ```
- Apenas os primeiros `callee_regs` registradores de `self.registers` são empilhados em `self.register_stack`:
  ```rust
  let window_start = self.register_stack.len();
  self.register_stack.extend_from_slice(&self.registers[..callee_regs]);
  ```
- O callee é inicializado apenas nos registradores que realmente utiliza:
  - `0..args.len()`: recebem os argumentos;
  - `args.len()..func.arity`: recebem `Value::Unset` para parâmetros ausentes;
  - `func.arity..callee_regs`: zerados para `Value::None` (impedindo vazamento de temporários residuais do chamador).
- Os registradores acima de `callee_regs` até 255 permanecem intocados em `self.registers`.

### B. Restauração Cirúrgica e Suporte Total a Suspensão Cooperativa
- **No Retorno Normal:**
  Apenas a janela de `callee_regs` é restaurada da pilha para `self.registers`:
  ```rust
  let saved_slice = &self.register_stack[window_start..];
  self.registers[..callee_regs].clone_from_slice(saved_slice);
  self.register_stack.truncate(window_start);
  ```
  Zero alocações adicionais no heap durante execuções subsequentes (o buffer interno de `Vec` é reutilizado), e zero cópias de 4 KiB.
- **Em Casos de Runtime Fault:**
  A janela do chamador é restaurada com a mesma garantia, mantendo a consistência do estado da máquina mesmo se o callee disparar divisão por zero ou violação de contrato.
- **Em Suspensão Cooperativa de Tarefa (`RegSuspendCapture`):**
  Como a janela do chamador é restaurada antes de inspecionar `self.pending_capture`, o snapshot de `self.registers` inserido na cadeia de `RegFrameSlot` contém 100% dos registradores do chamador exatamente como esperado pelo escalonador cooperativo, preservando a semântica de `await`, `sleep` e `yield`.

### C. Segurança Estrita (`#![forbid(unsafe_code)]`)
- A otimização foi implementada utilizando Rust 100% seguro com slices indexados e limites verificados estaticamente.
- Nenhuma linha de `unsafe` foi adicionada.

---

## 3. Evidência de Qualidade e Conformidade

### A. Bateria de Testes Dedicada (`crates/aipo-vm/tests/reg_window_tests.rs`)
6 cenários de teste automatizados validam a corretude da janela:
1. `test_reg_window_caller_registers_preserved_outside_callee_window`: caller com 10 registradores chama callee de 2 registradores; registradores fora da janela preservados intactos.
2. `test_reg_window_asymmetric_nested_call_cascade`: cadeia de chamadas com janelas assimétricas (caller: 8 regs $\to$ fn1: 3 regs $\to$ fn2: 6 regs $\to$ fn3: 2 regs) desbobina e restaura com precisão.
3. `test_reg_window_recursive_fibonacci`: Fibonacci recursivo `fib(10) = 55` na `RegVm` executa dezenas de frames de recursão sem corromper registradores e drena 100% da `register_stack`.
4. `test_reg_window_uninitialized_locals_isolation`: garante que temporários do callee não inicializados recebam `Value::None` e não resíduos do chamador.
5. `test_reg_window_restores_cleanly_on_callee_runtime_fault`: divisão por zero dentro do callee restaura os registradores do chamador e drena a pilha.
6. `test_reg_window_cooperative_task_suspend_and_resume`: ativação assíncrona com `await` suspende e retoma perfeitamente sobre a máquina de registradores.

### B. Canários de Vazamento de Memória (`leak_canaries.rs`)
- O canário de ciclo de vida da `RegVm` em `crates/aipo-testkit/tests/leak_canaries.rs` passou com **`growth = 0 bytes`**.

---

## 4. Ganhos Mensuráveis

| Métrica | Antes (Cópia Cega) | Depois (Janela de Registrador) | Melhoria |
|---|---|---|---|
| Cópia de memória por ativação (callee com 3 regs) | 4.096 B (4 KiB) | 48 B | **-98.8%** |
| Inicializações de `Value::None` por ativação | 256 valores | 0 a 3 valores | **-98.8%** |
| Pegada de pilha Rust em recursão máxima (64 níveis) | 262.144 B (256 KiB) | ~3 KiB | **-98.8%** |
| Overhead de alocação no heap pós-warmup | 0 alocações | 0 alocações | Idêntico (Zero) |
