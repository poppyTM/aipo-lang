# Matriz de Compatibilidade Semântica: Aipo-lang vs omniLua (Lua 5.4)

> **Regra Cardinal da Diretriz Técnica:**  
> A semântica da Aipo é definida canonicamente por seus ADRs (ADR-001, ADP-011, ADP-012), suas especificações em `docs/canon/` e seu corpus de conformidade. **omniLua e a especificação de Lua NÃO possuem autoridade semântica sobre a Aipo.** Nenhuma divergência deve ser resolvida alterando o comportamento linguístico da Aipo para se adequar a Lua.

---

## 1. Matriz Comparativa de Domínios Fundamentais

| Dimensão Semântica | Aipo-lang (Canônico) | omniLua (Lua 5.4.7) | Grau de Compatibilidade | Decisão Arquitetural |
|---|---|---|---|---|
| **Sintaxe & Estrutura de Blocos** | Blocos universais em `{ ... }`, sem parênteses obrigatórios em `if`/`while`, palavras completas, divisão inteira com `//`. | Palavras-chave delimitadoras `then ... end`, `do ... end`. | **INCOMPATÍVEL** | **REJECT:** Manter 100% a gramática canônica da Aipo (ADR-001, ADP-012). |
| **Modelo de Strings** | Texto puro Unicode (UTF-8) com normalização canônica NFC preservada e garantida em fronteiras de concatenação. | Sequência opaca de bytes arbitrários (`&[u8]`). Pode conter sequências inválidas de UTF-8. | **INCOMPATÍVEL** | **REJECT:** Jamais substituir `String` da Aipo por byte-strings sem validação Unicode. |
| **Coleções & Estruturas de Dados** | Tipos explícitos e separados: `List`, `Dict`, `Set` (ordem de inserção), `Sequence` (lazy), `Bytes` (empacotamento binário), `Struct` tipada nominal. | Única estrutura unificada: `Table` (híbrido array/hash indexado em 1) com despacho dinâmico via `Metatable`. | **INCOMPATÍVEL** | **REJECT:** Não introduzir metatables nem tabelas universais estilo Lua na Aipo. |
| **Imutabilidade e Orientação a Objetos** | Structs imutáveis por padrão (mutabilidade requer prefixo explícito `var`). Subtipagem estrutural estática e em tempo de execução via interfaces. **Sem OOP clássico obrigatório.** | Mutabilidade total por padrão em tabelas; prototipagem ad-hoc via metatables (`__index`). | **INCOMPATÍVEL** | **REJECT:** Preservar a imutabilidade por padrão e subtipagem estrutural da Aipo. |
| **Modelo de Erros e Falhas** | **Modelo B+:** Distinção estrita entre `Failure` recuperável (com payload estruturado, tratado via `attempt { ... }` ou `or_else`) e `Fault` de integridade (fatal). | Erros dinâmicos arbitrários capturados universalmente por `pcall` / `xpcall`. | **DIVERGENTE** | **INSPIRE / ADAPT:** Usar a técnica de `aborting` do omniLua para garantir que `Faults` de exaustão de limites (instruções/memória) sejam ininterceptáveis por `attempt`. |
| **Concorrência e Assincronismo** | `async fn` nativo, expressão `await`, scheduler determinístico cooperativo de `Task`s com tempo virtual e combinadores (`task.all`, `task.race`, etc.). | Corrotinas assimétricas manuais com suspensão explícita via `coroutine.yield()` e reativação via `coroutine.resume()`. | **DIVERGENTE** | **ADAPT:** Estudar o isolamento de contexto e pilhas de execução do omniLua, mas mantendo a semântica canônica do scheduler de Aipo. |
| **Valores Numéricos** | `Int` (64-bit assinado com verificação de overflow), `Float` (64-bit IEEE 754 finito; NaN e infinito são `Faults`), `Byte` (u8). | Inteiro 64-bit e Float 64-bit com semântica padrão C (NaN e Infinitos propagados como valores normais). | **DIVERGENTE** | **REJECT:** Aipo rejeita NaN/Infinito no modelo de valores com falha ativa. Manter invariants de `Value`. |
| **Segurança e Capabilities** | Modelo deny-by-default; `CapabilitySet` hierárquico validado no host antes do despacho nativo. | Hook de `SandboxLimits` para instruções/memória; remoção opcional de campos globais em `_G` via `Capability stripping`. | **COMPATÍVEL EM PROPÓSITO** | **ADAPT:** Aipo já possui capacidades superiores; adaptar a medição periódica de fuel do despachante do omniLua. |
| **API de Embedding Rust** | Atualmente baseada em C ABI (`aipo-c-abi`) e estruturas internas da VM. | API de alto nível `Lua::scope` com handles RAII seguros e empréstimos delimitados por lifetime. | **ALTAMENTE ADAPTÁVEL** | **ADAPT (Prioridade P0):** Desenvolver a camada equivalente `AipoEngine::scope` para embedding nativo seguro em Rust. |

---

## 2. O que JAMAIS pode ser importado do omniLua

1. **Parser e Léxico de Lua:** A sintaxe da Aipo foi projetada sob diretrizes cognitivas neurodivergentes (ADR-001/ND-First) para máxima clareza, previsibilidade e baixa sobrecarga mental. Não usar convenções de Lua (`~=`, `end`, `then`, indexação base-1).
2. **Byte Strings Arbitrárias como Texto:** Aipo garante texto UTF-8 normalizado NFC. O modelo de strings de Lua corromperia contratos de internacionalização da Aipo.
3. **Metatables e Prototipagem Aberta:** Aipo utiliza structs tipadas com checagem de invariantes (`invariant()`), validação estática de campos e conformidade de interfaces. Metatables introduziriam dinamismo não-determinístico incompatível.
4. **Captura Irrestrita de Erros Fatais:** Um script não pode ter o poder de capturar a violação de segurança do host ou o término forçado por cota de CPU.
