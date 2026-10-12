# Aipo-lang × omniLua — Diretriz técnica para auditoria, reaproveitamento e evolução

> **Documento operacional para code agents**  
> **Data:** 11 de outubro de 2026  
> **Alvo:** [Aipo-lang](https://github.com/poppy-team/aipo-lang)  
> **Referência principal:** [omniLua](https://github.com/ianm199/omnilua)  
> **Modalidade:** pesquisa de código, auditoria comparativa, experimentos isolados e incorporação incremental  
> **Estado:** proposta de execução; nenhuma substituição de arquitetura está previamente aprovada.

---

## 0. Instrução principal ao agente

Você é responsável por **compreender profundamente a implementação atual da Aipo-lang e a implementação do omniLua**, identificar partes que possam melhorar a Aipo, demonstrar tecnicamente os benefícios e, quando seguro e justificável, implementar melhorias **sem alterar acidentalmente a semântica, a sintaxe, a filosofia, a API ou a paridade dos backends da Aipo**.

**Não transforme a Aipo em um dialeto de Lua. Não faça fork da arquitetura inteira do omniLua. Não reescreva módulos funcionais só porque o código de referência parece mais elegante.** O objetivo é uma Aipo melhor, não uma Aipo que imita Lua.

Conduza o trabalho por **evidências reprodutíveis**: código exato, contratos, testes, medições, comportamento observável, riscos, licenças e custo de manutenção. Seja assertivo na pesquisa e no preparo de experimentos; seja conservador ao tocar contratos públicos e semântica. Não trate este documento como comprovação de que qualquer componente da Aipo esteja ausente, lento ou defeituoso — **verifique primeiro**.

### Resultado final esperado

1. Inventário comprovado da Aipo atual, incluindo o que existe, o que está parcial e o que falta.
2. Inventário das técnicas relevantes do omniLua, com arquivos, símbolos, commits e invariantes.
3. Matriz de compatibilidade e oportunidades, com decisão **COPIAR / ADAPTAR / INSPIRAR / DESCARTAR / ADIAR** por item.
4. Baseline de testes, performance, memória, build, binário e comportamento de embedding.
5. Provas de conceito pequenas para as oportunidades de maior retorno e menor risco.
6. Implementações incrementais somente após passar pelos gates definidos neste documento.
7. Documentação de cada decisão, mudança, benchmark, regressão, trade-off e crédito de terceiros.
8. Relatório final com o que foi comprovado, implementado, rejeitado e ainda recomendado.

### Fontes e grau de confiança

A arquitetura do omniLua foi verificada em documentos públicos do projeto em 11/10/2026. **Não foi possível inspecionar integralmente o HEAD da Aipo no contexto de preparação deste documento.** Portanto, as descrições da Aipo abaixo são *hipóteses de inventário* ou objetivos declarados, a serem confirmados pelo agente contra código, testes e documentação atuais. Nem o README de qualquer projeto nem este documento têm precedência sobre o código e os testes existentes quando se trata de determinar *o que está implementado*; para determinar *o que deve significar*, use a especificação canônica da Aipo.

Marque fatos relevantes como:

- `VERIFICADO-AIPO`: confirmado no commit identificado da Aipo.
- `VERIFICADO-OMNI`: confirmado no commit identificado do omniLua.
- `DECLARADO`: apenas documentação/README, ainda sem reprodução.
- `HIPÓTESE`: inferência não testada.
- `DIVERGENTE`: especificação, implementação e/ou testes discordam.
- `NÃO APLICÁVEL`: diferença de semântica ou custo superior ao benefício.

Nunca converta `DECLARADO` ou `HIPÓTESE` em fato sem prova.

---

## 1. Princípios e limites inegociáveis da Aipo

### 1.1 Identidade da linguagem

Na auditoria inicial, identifique e documente precisamente as decisões **vigentes**, sem pressupor que informações antigas continuem corretas:

- Sintaxe, gramática, precedência, módulos, funções, blocos, expressões e fluxo de controle.
- Filosofia de legibilidade, simplicidade, composição, acessibilidade e ergonomia; **não introduzir OOP obrigatório**.
- Tipagem dinâmica/forte, contratos opcionais de assinatura e coerção, conforme o estado real.
- Semântica de valores, identidade, igualdade, nulidade, coleções e mutabilidade.
- Unicode, UTF-8, normalização e representação pública de strings, quando especificados.
- Modelo de falhas, rollback/transações, propagação de erros e cancelamento, se aplicável.
- Async, corrotinas, scheduling, relógio/tempo e determinismo, se aplicável.
- Compilação para bytecode, execução na VM e quaisquer backends JS, Wasm ou outros realmente presentes.
- Perfil embarcado (*embedded*), execução de plugins, hot reload e sandbox, distinguindo implementado de planejado.
- Compatibilidade de programas antigos, estabilidade de bytecode, C ABI/FFI e APIs públicas, se houver.

**Regra:** a linguagem hospedeira Rust não define a semântica da linguagem Aipo. Tampouco Lua a define. Técnicas de representação e execução só podem ser incorporadas por trás de uma interface compatível com a especificação da Aipo.

### 1.2 Hierarquia de autoridade

1. ADRs aprovados, especificação canônica e decisões vigentes da Aipo.
2. Testes de conformidade aprovados que concretizam essas especificações.
3. Interfaces públicas e compromissos de compatibilidade documentados.
4. Implementação atual e comportamento observado — úteis para detectar desvios, não para legitimar bugs.
5. Documentação técnica e exemplos.
6. omniLua e outras referências externas — **material comparativo, nunca autoridade semântica da Aipo**.

Se houver conflito, **registre a divergência e faça uma proposta isolada**. Não altere silenciosamente a linguagem para fazê-la passar num teste do omniLua.

### 1.3 O que NÃO fazer

- Substituir parser/gramática da Aipo pelo parser Lua.
- Trocar a representação de strings por byte strings Lua quando a Aipo exige texto Unicode.
- Importar coercões, tabelas/metatables, escopo, funções ou erros com semântica Lua.
- Mudar VM de pilha para registradores por preferência teórica.
- Fundir GC, VM e API pública em uma alteração gigante.
- Adicionar `unsafe` ou dependências novas para contornar problemas de desenho sem justificativa.
- Diminuir testes, ignorar regressões, ajustar testes para esconder falhas ou declarar benchmarks não reproduzidos.
- Introduzir fallback silencioso, `unwrap()` ou `panic!()` em fronteiras de código não confiável.
- Publicar, fazer merge ou modificar `main` automaticamente sem autorização compatível com o fluxo vigente do repositório.
- Forçar compatibilidade com ABI binária da `liblua`, algo separado da API nativa de embedding.

---

## 2. Preparação: congelar o estado e mapear os dois projetos

### 2.1 Preparar um ambiente reprodutível

- Trabalhe em clone limpo da Aipo; anote branch, `HEAD`, ferramenta Rust, SO, arquitetura, flags, lockfile, cargo features e artefatos gerados.
- Faça clone do omniLua **em diretório externo à árvore produtiva da Aipo**, somente para leitura inicial; anote `HEAD`, tag e licença; fixe o commit de referência.
- Não presuma que `main` seja a versão estável nem que `PORTING.md` descreva o projeto atual. Leia primeiro o `CLAUDE.md` do omniLua e os guias de cada crate.
- Confira a licença **da Aipo atual** e seus arquivos `LICENSE`/`NOTICE`; nunca suponha que é MIT só porque o omniLua é.
- Inspecione `git status` e trabalhos não commitados antes de executar qualquer alteração.

Comandos de descoberta (adapte à estrutura existente):

```bash
# Na Aipo
pwd
git status --short --branch
git rev-parse HEAD
find . -maxdepth 3 -type f \( -name 'Cargo.toml' -o -name 'AGENTS.md' -o -name 'CLAUDE.md' -o -name '*SPEC*' \) -print
cargo metadata --no-deps --format-version 1 > /tmp/aipo-cargo-metadata.json
rg -n 'enum .*Value|struct .*Vm|struct .*Heap|opcode|bytecode|sandbox|fuel|allocator|coroutine|embed|wasm' --glob '*.rs' .

# No clone da referência
cd ../omnilua
git rev-parse HEAD
cat Cargo.toml
cat CLAUDE.md
cat PORTING.md  # histórico: interpretar à luz das orientações atuais
```

Se o repositório Aipo estiver inacessível, relate explicitamente o bloqueio e **não invente caminhos, módulos ou resultados**. A próxima fase exige acesso real ao código.

### 2.2 Inventário obrigatório da Aipo

Produza uma tabela com `subsistema | crate/pasta | API | algoritmos | dependências | semântica | testes | limitações | evidência (path:linha, commit)`. Abranja ao menos:

1. Lexer, parser, AST, diagnósticos e recuperação de erros.
2. Name resolution, símbolos, escopo, closures e verificação de contratos.
3. Lowering/IR, compilador, otimizações, bytecode e formato serializado.
4. VM, dispatch, chamadas, stack frames, exceções, native calls e caches.
5. Valores, objetos, strings, arrays, mapas e identidade.
6. Heap, alocação, gerenciamento de memória, roots e finalizadores.
7. Async, scheduler, corrotinas, cancelamento, tempo e hot reload.
8. Standard library, módulos, import, loaders e capabilities.
9. API do host, integração com Rust e FFI/ABI onde aplicável.
10. Backends secundários, JS/ESM, Wasm, Wasmtime e testes de paridade.
11. CLI, tooling, docs, benchmark, fuzz, CI e distribuição.
12. Observabilidade, profiler, trace, debug, diagnósticos e relatórios de falha.

Investigue explicitamente **código morto, TODOs antigos, feature flags inoperantes, duplicação semântica entre backends, alocações desnecessárias, crescimento de dependências e hot paths**. Separe oportunidades de limpeza comum das oportunidades influenciadas pelo omniLua, para não atribuir benefícios indevidos à referência.

### 2.3 Snapshot / baseline obrigatório

Registre antes de alterar qualquer coisa:

- Comandos que compilam e respectivos resultados.
- Quantidade de testes por categoria, aprovação/falha/skip e ambiente necessário.
- Testes de conformidade VM ↔ JS ↔ Wasm, se existentes.
- Benchmarks (versão release) e uso de memória com variância.
- Tamanho do artefato de execução, tempo de build, inicialização e warmup.
- Limites e falhas conhecidos, com caso mínimo reproduzível.
- Perfis de GC: frequência de coleta, pause, bytes vivos, picos e fragmentação, se mensuráveis.
- Flags de segurança de scripts, comportamento diante de input hostil e exceções.

Se não há medição, documente **SEM BASELINE**. Não invente números.

---

## 3. Leitura profunda do omniLua: ordem e arquivos prioritários

**Atenção:** nomes abaixo foram observados nos documentos/manifesto do omniLua; confirme no commit fixado os caminhos, versões internas, funções e contratos. A organização evolui.

| Prioridade | Área / localização de referência | Perguntas obrigatórias | Possível retorno à Aipo |
|---|---|---|---|
| P0 | `CLAUDE.md`, `PORTING.md`, `ANALYSES/`, `specs/`, `harness/` | Como o projeto coordena agentes, oráculos e regressões? Que instruções são históricas? | Metodologia, disciplina e automação |
| P0 | `crates/lua-rs-runtime/` | Como constrói o estado, converte valores, registra callbacks, delimita escopos? | API de embedding |
| P0 | `docs/SANDBOXING_EXPLORATION.md`, `crates/lua-vm/src/state.rs`, `debug.rs`, `crates/lua-stdlib/` | Como metering, memória e permissões chegam a todos os fluxos? | Sandbox e limites de recursos |
| P0 | `crates/lua-gc/` | Heap, roots, handles, barriers, weak refs, accounting, ciclo de vida? | Segurança de memória e GC |
| P1 | `crates/lua-vm/` | Dispatch, representação de stack, upvalues, calls, erros, metamétodos? | Performance e organização da VM |
| P1 | `crates/lua-types/` | `LuaValue`, strings, identidade, referências e layout? | Redução de custos de valores |
| P1 | `crates/lua-code/` | Geração de instruções, constantes, patching, escopos? | Otimizações do compilador |
| P1 | `crates/lua-lex/`, `crates/lua-parse/` | Parsing incremental, recuperação, custo de tokenização? | Técnicas de parser e diagnóstico (não gramática) |
| P1 | `crates/lua-coro/` | Contextos, pilhas, suspensão, ownership, plataformas? | Ideias para tarefas/async, sob contrato próprio |
| P1 | `crates/lua-stdlib/` | Capability hooks, segurança de funções, bibliotecas puras? | Isolamento host/stdlib |
| P1 | `crates/lua-wasm/`, `crates/lua-wasm-smoke/` | Build e testes *reais* em navegador/Wasm? | Portabilidade |
| P2 | `crates/lua-rs-derive/`, `crates/lua-rs-lfs/` | Derive de conversões, extensibilidade e dependências? | Ergonomia da API, sob demanda |
| P2 | `crates/lua-cli/`, `examples/`, `packages/omnilua/` | REPL, DX, distribuição, integração JS? | Ferramentas e empacotamento |
| P2 | `harness/bench/`, documentos de medição | Como o baseline é congelado e comparado? | Benchmark reproduzível |

O manifesto raiz do omniLua lista `lua-types`, `lua-lex`, `lua-parse`, `lua-code`, `lua-vm`, `lua-stdlib`, `lua-gc`, `lua-coro`, `lua-cli`, `lua-rs-runtime`, `lua-rs-derive`, `lua-wasm` e outros. **Não conclua que essa decomposição exata seja desejável para a Aipo.** Avalie separação real de responsabilidades, acoplamento e tempo de compilação.

### 3.1 Como analisar cada módulo

Para **cada** módulo relevante, responda:

1. Qual problema resolve, como funciona, quais invariantes protege e qual dependência exige?
2. Quais operações são hot path e quais podem ser refatoradas sem custo significativo?
3. Qual a estratégia atual, não somente a descrita historicamente?
4. Qual a semântica de Lua que está embutida na técnica?
5. Existe funcionalidade equivalente na Aipo? Onde? Qual sua maturidade comprovada?
6. É possível extrair o algoritmo sem transferir semântica de Lua?
7. Melhor copiar, adaptar, inspirar ou não adotar? Por quê?
8. Qual o novo conjunto de testes, perfil de performance e risco de regressão?
9. O que acontece se o código de referência evoluir ou for abandonado?
10. Onde ficam as obrigações de licença/NOTICE caso haja cópia substancial?

Registre símbolos e intervalos de linha **do commit fixado**. Evite links de arquivo sem SHA para evidência permanente.

---

## 4. Análise detalhada por subsistema

### 4.1 Modelo de valores, strings e coleções

**Investigar no omniLua:** `LuaValue`, heap references, boxing/unboxing, representações numéricas, interning, tabelas híbridas array/hash, hashing, equality, layouts, conversão entre chamadas Rust/Lua, uso de `Rc`, handles e lifetime.

**Investigar na Aipo:** quais tipos existem; inteiros/float/decimal, NaN/±0, overflow e comparação; semântica de coleções, ordenação, identidade e mutabilidade; tamanho (`size_of`) e alocações por valor; coerência entre backends; UTF-8 e normalização; cópia versus compartilhamento; comportamento determinístico.

**Oportunidades candidatas:** tagged representations, índices estáveis, interning seletivo de identificadores, compactação de objetos, `SmallVec` ou otimizações para pequenas coleções (somente se perfil justificar), hashing resistente a DoS, redução de clone, estratégias de reuso de buffers, mapas com especialização de chaves.

**Risco semântico:** strings de Lua são sequências arbitrárias de bytes. Se a Aipo prevê texto UTF-8/NFC, manter uma distinção `Text` versus `Bytes` (se `Bytes` existir na especificação); **jamais trocar `String` por byte strings indiscriminadamente**. Tabelas e metatables de Lua também não equivalem automaticamente a mapas/records Aipo.

**Testes essenciais:** round-trip de texto com Unicode combinante, emoji e inválidos; igualdade/identidade de objetos; hashing com colisões; ordem (se contratual); valores extremos de números; serialização nos backends; pressão de alocação.

**Saída:** proposta comparativa de representação com layout de memória, custos e casos incompatíveis explícitos. Não trocar estrutura principal sem benchmark e testes.

### 4.2 Garbage collector, heap e ownership

**Investigar no omniLua:** `crates/lua-gc/`, gerenciamento de `GcRef`, alocações, roots permanentes/temporários, barreiras de escrita, fases de marcação/varredura, weak refs, ephemerons, finalização, thresholds, pacer, tracing e contagem de bytes. Confira se há trabalho posterior de GC geracional e seu status **no commit fixado**, não no `PORTING.md` histórico.

**Investigar na Aipo:** se existe GC próprio, `Rc`/`Arc`, arenas, contagem de referências, ownership host, objetos cíclicos; localização de roots em pilhas, closures, constantes, módulos, objetos expostos ao host, callbacks, scheduler e cache de scripts; descarte de VM, rollback, hot reload, módulos e handles órfãos.

**Experimentos admissíveis, em ordem:**

1. Medir live heap / heap peak / alocações / pausas (instrumentação não invasiva).
2. Identificar leaks e ciclos, com testes de desmontagem do runtime.
3. Corrigir casos isolados de rooting, double-drop, retenção e finalização se existirem.
4. Explorar estratégias de GC em protótipo **sem substituir imediatamente o coletor atual**.
5. Só discutir migração de algoritmo se houver benefício comprovado de throughput, pico e latência com compatibilidade.

**Pontos críticos:** invariantes de write barrier, roots durante chamada de host, mutação em callbacks, GC recursivo, weak handles durante sweep, finalizadores que reentram a VM, objetos ressuscitados, memória externa ao heap, limite que falha no meio de alocação, exceção/cancelamento no meio da criação de objeto e rollback.

**Critério especial de aprovação:** toda alteração em GC exige testes adversariais, leaks/lifecycle, estresse com minúsculo threshold de coleta, benchmark de pausas, e comprovação de que a Aipo permanece utilizável após erros recuperáveis.

### 4.3 Máquina virtual, bytecode e otimizações

**Investigar no omniLua:** dispatch (`match`/loops), `CallInfo`, stack indices versus referências, registradores, constantes, caches, fechamento de upvalues, return/tail call, operadores, exception propagation, metering e debug hooks. Verificar `unsafe`, stack growth, profundidade de chamadas e reentrância.

**Investigar na Aipo:** representação real da VM; se for baseada em pilha, suas invariantes; encoding de instruções; bytecode versioning; call frames; caches; resolução de nomes; inline caches; variáveis capturadas; tail calls; hot reload; fronteiras entre VM e biblioteca padrão.

**Oportunidades candidatas:** reduzir branches/alocações no dispatch, especializar opcodes frequentes, otimizar constante e acesso a variáveis locais, índices estáveis, caching com invalidação correta, predecodificação parcial, verificações de stack centralizadas, melhorar layout de arrays de instrução.

**Não adotar automaticamente:** VM baseada em registradores, metatables, semântica de chamadas Lua e bytecode binariamente compatível com Lua. Uma mudança de arquitetura da VM só entra após RFC com trade-offs de compilador, debug, async, distribuição e compatibilidade entre backends.

**Testes essenciais:** instruções inválidas, stack under/overflow, recursão profunda, closure após retorno, captura por referência/valor segundo a Aipo, callbacks que reentram VM, metering, debug locations, execução repetida e divergência VM × JS/Wasm.

**Segurança:** se bytecode externo puder ser carregado, incluir **validador de bytecode** (versão, bounds de registradores/pilha, índice de constantes, jump targets, profundidade) antes de execução; caso contrário, definir como formato interno não confiável por padrão até existir verificador.

### 4.4 Compiler frontend e geração de código

**Investigar no omniLua:** tokenização, parser, resolução de escopos, recuperação de erro, constant folding, emissão de instruções, jump patching, closures e tabelas de constantes.

**Aplicar na Aipo:** estudar qualidade de algoritmos, não reproduzir gramática. Auditar repetição de parsing, AST allocations, qualidade do diagnóstico, span preservation, erros de UTF-8, recursion depth, panic safety e fidelidade do compilador aos contratos opcionais.

**Experimentar:** testes baseados em propriedades para lexer/parser, minimização de casos falhos, AST/IR invariants, fuzzing de entradas arbitrárias, implementação de algoritmos locais com testes mínimos, cold/hot compilation benchmarks, tratamentos seguros de input malformado.

**Não mudar:** sintaxe, escolha de palavras-chave, regras de escopo ou mensagens públicas de erros sem decisão explicitamente documentada.

### 4.5 API de embedding (prioridade máxima)

**Estudo específico no omniLua:** `Lua`, `Lua::new_versioned`, `Lua::scope`, `create_function`, `create_userdata_ref_mut`, `globals().set`, `load(...).exec`, `SandboxConfig`, callbacks tipados, scope-bounded borrows, conversão `FromLua`/`IntoLua` (confirmar símbolos atuais). Observe como um handle que escapou do escopo resulta em erro seguro, e como a API não depende de uma `liblua` C.

**Avaliar na Aipo:** interface para criar e destruir runtime, compilar uma vez/executar muitas, chamadas Aipo↔Rust, binding de structs/objetos, registro de funções nativas, userdata, handles de valores persistentes, lifetimes, lifetime de closures, callbacks recursivos, acesso a host state, thread safety, erro estruturado, memória de recursos externos, integração com motores e editores.

**Proposta de API conceitual, NÃO assinatura já aprovada:**

```rust
// Apenas desenho para discussão. Ajustar aos tipos e contratos existentes.
let engine = AipoEngine::builder()
    .capabilities(CapabilitySet::empty())
    .limits(ExecutionLimits::for_plugins())
    .build()?;

engine.scope(|scope| {
    let world = scope.borrow_mut(&mut game_world)?;
    scope.expose("world", world)?;
    engine.run("examples/update.aipo")
})?;
```

O mecanismo deve garantir: (a) referências emprestadas não sobrevivem ao escopo; (b) callbacks podem reportar erros sem panic através da fronteira; (c) não há aliasing mutável indevido; (d) handle obsoleto reporta erro claro; (e) reentrada e invalidação são explícitas; (f) as APIs podem ser usadas para hot reload se autorizado.

**Matriz de testes:** empréstimo mutável e compartilhado, retorno normal, erro, panic do host convertido/isolation policy, callback aninhado, captura de objeto, uso depois do scope, GC entre ida e volta, script com timeout/cancelamento e objeto host que é descartado.

**Decisão inicial preferida:** **ADAPTAR padrões** e, quando aplicável, copiar helpers isolados de conversão com licença preservada. **Não copiar o facade Lua inteiro**: nomes, semântica, tipos e lifetimes devem respeitar Aipo.

### 4.6 Sandbox, capacidade e limites de recursos (prioridade máxima)

**Estudo específico no omniLua:** limites globais de instruções que abrangem corrotinas, estado de aborto persistente que não pode ser suprimido indefinidamente por protected calls, budget de memória amostrado, remoção/omissão de capabilities, `HostHooks` e limites de preempção documentados. Leia `docs/SANDBOXING_EXPLORATION.md` e verifique a implementação atual.

**Desenho preferido para Aipo:**

- **Deny by default** com concessão explícita de capacidades: FS, rede, processo, relógio real, aleatoriedade, carregamento de módulos, acesso a recursos de host, debug, introspecção e FFI.
- **Separação por runtime / plugin:** ambientes, módulos e objetos host isolados; sem singletons globais que atravessem runtimes.
- **Resource limits:** fuel/instruções, limite de memória observável e idealmente imposto no caminho de alocação, profundidade de chamadas, saída/log, tamanho de módulo, carga inicial, tamanho de string/coleção e limite de callbacks custosos.
- **Abort sticky ou erro fatal não capturável** para limite de segurança. Se a Aipo possuir modelo próprio de falha recuperável/fatal, integrar ao contrato dela, não ao `pcall` de Lua.
- **Metering em todos os caminhos:** dispatch, callbacks, loops intrínsecos, padrões/regex, sort customizado, corrotinas/tarefas, loaders, operações nativas.
- **Capacidades transmitidas explicitamente** a funções nativas/host; impedir escalonamento indireto de privilégio.
- **Tempo e RNG controláveis:** relógio virtual/semeado para determinismo de testes, quando isso fizer parte dos objetivos Aipo.

**Limites e advertências:** medição de memória por amostragem não constitui hard cap exato; fuel por opcode não impede automaticamente bloqueio dentro de callback nativo; cancelamento cooperativo não é preempção da thread; sandbox dentro do mesmo processo não é uma garantia suficiente contra vulnerabilidades de implementação nativa. Para scripts realmente adversariais com exigência de isolamento forte, considerar processo separado / sandbox de SO ou Wasm com capabilities explícitas.

**Ataques de regressão a construir (testes com timeout externo):**

```text
loop infinito simples
recursão infinita e profunda
explosão de alocação em pequenos e grandes objetos
callback host bloqueante ou excessivamente custoso
loop tentando capturar/suprimir erro de budget
caminho de tarefa/corrotina não contabilizado
regex/pattern com crescimento catastrófico
função de sort com callback adversarial
import/load/dynamic evaluation tentando alcançar filesystem/process/network
objeto host emprestado após fim de scope
GC/finalizador executando chamada reentrante durante aborto
```

**Aceitação:** o runtime deve sair com erro classificável, sem travar processo host nem deixar estado parcialmente válido quando o contrato prevê recuperação. Medir overhead de sandbox ligado/desligado. O orçamento deve ser compartilhado pelo domínio de isolamento correto e não resetar em função protegida ou tarefa descendente.

### 4.7 Corrotinas, async e scheduler

**Não equipare `coroutine.yield` Lua à semântica async da Aipo.** Primeiro mapear tarefas, scheduler, cancelamento, frame budget, relógio e garantias de execução Aipo. omniLua declara que seu budget pode abortar execução, mas não proporciona *pause/resume* arbitrário do dispatch mediante fuel; sua arquitetura limita essa forma de scheduling.

Avalie se a Aipo já dispõe de scheduler cooperativo; nesse caso, integrar orçamento por tarefa e por runtime, pontos de suspensão estáveis e propagação do cancelamento. Estudar o omniLua como **referência de limites e gerenciamento de contexto**, não como modelo de concorrência a ser transplantado.

**Testes:** cancelamento durante função nativa, suspensão em nested call, resumption após hot reload (quando suportado), cleanup RAII, finalização de handles, deadlocks/reentrância, starvation, 10 mil tarefas curtas, timeout e transição de estados.

### 4.8 Standard library, módulos e host hooks

Classificar funções da stdlib por *puras*, *determinísticas*, *dependentes de host*, *privilegiadas* e *potencialmente caras*. Verificar no omniLua que operações de FS, processos, carregamento e tempo passam por hooks/boundaries; verificar se alguma contorna esse sistema.

Na Aipo, preferir `HostServices`/capabilities explícitos a chamadas dispersas a `std::fs`, `std::net`, `std::process`, relógio ou variáveis de ambiente. Evitar trazer funções Lua que colidam com semântica ou nomenclatura Aipo. Algoritmos matemáticos e utilitários podem ser candidatos a **COPIAR** somente quando os contratos forem efetivamente equivalentes.

**Testes:** nenhuma capability concede outra implicitamente, import com caminho malicioso, symlink/traversal (se FS existir), módulos cíclicos, cache/reload, logs e saídas sob orçamento, Unicode em paths, acesso concorrente, pacote não confiável.

### 4.9 Backends JS / Wasm / paridade semântica

Qualquer alteração do runtime Rust precisa passar por uma destas análises:

- É puramente interna e não muda resultados? Se sim, verificar suite e casos-limite.
- Afeta semântica pública? Então propor atualização na especificação e adaptar **todos** os backends, ou rejeitar.
- Afeta apenas limites/segurança de execução? Definir equivalência de capacidades e documentar diferenças de plataformas. Não afirmar paridade que não existe.

Para Wasm, `cargo check --target ...` **não prova execução**. Exigir smoke test real com operações, strings, closures, erros e callbacks. Distinguir `wasm32-unknown-unknown`, WASI e Wasmtime/browser, incluindo limitações de FS/rede/tempo.

### 4.10 Observabilidade e diagnóstico

Estudar o harness, trace, debug hooks e suites do omniLua. Na Aipo, instrumentar por feature (desligável em release quando impactar hot path): contagem de instruções, alocações por tipo, live heap, pause p95/p99, tempo de compilação, cache hit, número de callbacks host, mudanças de módulos e violações de budget. Preservar source spans e erros legíveis — priorizar mensagens úteis e acessibilidade no uso por desenvolvedores.

---

## 5. Política de reaproveitamento: copiar, adaptar, inspirar ou rejeitar

### 5.1 Classificações vinculantes

| Código | Decisão | Permissão / requisito |
|---|---|---|
| `COPY` | Copiar trecho/função isolada | Somente equivalência contratual demonstrada, origem exata, licença e tests; não copiar dependências sem necessidade |
| `ADAPT` | Transpor algoritmo para tipos/contratos Aipo | Preferencial quando a ideia é transferível, mas a semântica exige mudanças |
| `INSPIRE` | Aplicar princípio arquitetural/metodologia | Padrão para VM, API, sandbox e organização de testes |
| `DEFER` | Estudar depois | Benefício plausível, evidência/tempo insuficiente ou dependências não preparadas |
| `REJECT` | Não reutilizar | Incompatibilidade semântica, regressão, custo ou risco injustificável |

**Atenção jurídica:** omniLua é MIT e se declara *trabalho derivado* da Lua 5.4.7. Ao copiar material substancial, cumprir avisos e obrigações das licenças de ambos, preservando os respectivos notices e copyright. Revisar `omnilua/LICENSE`, `omnilua/NOTICE`, as licenças da Lua original e **a licença da Aipo**. Registrar provenance e dependências de código copiado. Evitar extrair blocos grandes que tragam semântica Lua indesejada. `MIT` não significa ausência de obrigações.

### 5.2 Matriz inicial de oportunidades — a confirmar

| Oportunidade | Via inicial | Prioridade | Benefício buscado | Bloqueio principal |
|---|---|---|---|---|
| Oráculo / harness / canários | INSPIRE | P0 | Correção e regressão | Adaptar oráculo à Aipo |
| Benchmark intercalado e baseline fixo | INSPIRE | P0 | Medições confiáveis | Infraestrutura e ruído |
| Limites de CPU/instruções | ADAPT | P0 | Execução controlada | Cobrir async, native calls e fatal errors |
| Host capabilities / stdlib profiles | ADAPT | P0 | Plugins seguros | Garantir enforcement no host |
| Scope API e userdata emprestado | ADAPT | P0 | Embedding Rust ergonômico | Lifetimes, aliases e reentrância |
| Registro explícito de erros e abortos | INSPIRE/ADAPT | P0 | Segurança e recuperação | Modelo próprio de erros Aipo |
| Instrumentação de GC e leak canaries | INSPIRE/ADAPT | P0 | Diagnóstico de memória | Acesso real ao heap |
| Write barriers / GC incremental | ADAPT/DEFER | P1 | Latência/uso de memória | Ownership e GC atual |
| Representação de valor/handle | ADAPT/DEFER | P1 | Menos alocações/cópias | Semântica de identidade |
| Otimização de dispatch/calls | ADAPT | P1 | Throughput VM | Benchmark micro e macro |
| Aproveitamento de helper matemático | COPY/ADAPT | P2 | Tempo de implementação | Equivalência de contrato/licença |
| Design e erros de parser | INSPIRE | P2 | Ergonomia de diagnóstico | Gramática Aipo distinta |
| VM registradores em vez de pilha | REJECT por padrão | P3 | Hipótese não demonstrada | Reescrita ampla, bytecode, debug, async |
| Parser / sintaxe Lua | REJECT | — | Nenhum para identidade Aipo | Incompatibilidade fundamental |
| Tabelas/metatables de Lua | REJECT por padrão | — | Modelo estranho à Aipo | Semântica e mutabilidade |
| ABI `liblua` | DEFER / projeto separado | P3 | Compatibilidade Lua externa | Não é objetivo da linguagem |

### 5.3 Score de decisão (0 a 5 por dimensão)

Preencha para cada candidato, com links para dados reais:

- `B` benefício técnico medido ou verificável (0–5).
- `F` aderência filosófica/semântica Aipo (0–5).
- `R` redução de risco/segurança (0–5).
- `M` melhoria de manutenção, simplicidade e qualidade (0–5).
- `C` custo de implementação e migração (0–5, maior = pior).
- `G` risco de regressão / complexidade de integração (0–5, maior = pior).
- `U` incerteza (0–5, maior = pior).

Fórmula **apenas para ordenar discussão**, não autoriza merge:

`pontuação = 3B + 3F + 2R + 2M - 2C - 3G - 2U`

Dê prioridade a ganhos de segurança comprováveis antes de ganhos marginais de throughput. Sempre apresente pontuação **e justificativa textual**. Resultados `COPY` demandam revisão de licença além do score.

---

## 6. Método de implementação orientado por evidências

### 6.1 Pipeline obrigatório de cada mudança

1. **Problema reproduzido:** evidência do bug, custo, lacuna ou ameaça na Aipo atual.
2. **Contrato:** trecho de especificação e casos de teste que devem continuar válidos.
3. **Referência omniLua:** commit, arquivo, função, invariantes, mecanismos, limitações e licença.
4. **Alternativas:** manter atual, mudança mínima própria, adaptar, copiar, dependência externa.
5. **Hipótese mensurável:** descrever o ganho, risco, métrica e ambiente.
6. **Experimento isolado:** branch/worktree e escopo pequeno, sem tocar API pública se desnecessário.
7. **Testes antes ou junto à alteração:** unidade, regressão, conformidade, fuzz/adversariais.
8. **Benchmark A/B:** baseline congelado, métricas e ruído documentados.
9. **Revisão independente:** arquitetura, semântica, segurança, licença, performance e legibilidade.
10. **Decisão:** integrar, revisar, adiar ou reverter; registrar por escrito.

Se não é possível medir o ganho agora, **não vender como otimização**; classificar como melhoria de correção/segurança/manutenção ou hipótese não comprovada.

### 6.2 Regra de diffs pequenos

Uma alteração de cada vez por PR: por exemplo, instrução-budget não vem junto de redesenho do GC; nova scope API não vem com alteração de parser. Corrigir a causa mínima, evitar refatoração oportunista, e registrar dívidas técnicas separadas. A ideia de tradução *literal first* do omniLua é útil **para ports de algoritmos específicos**; não é diretriz para portar o projeto inteiro ou sacrificar legibilidade da Aipo.

### 6.3 Segurança de Rust

- `unsafe` deve ficar restrito a módulos com invariantes documentadas e testes específicos.
- Exigir comentário `// SAFETY:` junto de cada bloco `unsafe`, precondições e responsáveis.
- Preferir índices/handles estáveis a empréstimos vivos sobre estruturas sujeitas a realocação.
- Evitar `transmute`, self-referential structures, raw pointers e FFI quando não necessários.
- Não aplicar `Arc<Mutex<...>>` ou `Rc<RefCell<...>>` de forma indiscriminada; primeiro medir a necessidade de compartilhamento e as consequências para o GC e paralelismo.
- Nunca deixar panic atravessar fronteira FFI/host. Definir política para unwinding/panic em callbacks internos.
- Usar `cargo fmt`, `cargo clippy`, `cargo test`, ferramentas de fuzz/sanitizers/Miri (quando aplicáveis) e auditoria de dependências.

### 6.4 Estrutura de isolamento para vários agentes

**Nunca dois agentes editando a mesma worktree ou os mesmos arquivos simultaneamente.** Use branches/worktrees separadas, responsabilidade e ownership de arquivos explícitos, contrato de integração, fila de PRs e coordenação de mudanças compartilhadas.

Papeis recomendados:

| Agente | Missão | Pode editar |
|---|---|---|
| Coordenador/arquiteto | Inventário, ADRs, dependências e priorização | Documentos de decisão, não código crítico em paralelo |
| Auditor de semântica | Contratos, corpus e paridade entre backends | Testes de conformidade, com revisão |
| Auditor de runtime/VM | Profiling, chamadas, bytecode e dispatch | Branch/área de VM exclusiva |
| Auditor de memória | Heap, GC, rooting e leaks | Branch/área de GC exclusiva |
| Engenheiro de embedding/sandbox | APIs host e limites de execução | Branch/área de API/sandbox exclusiva |
| Validador independente | Benchmarks, fuzz, licença e regressão | Harness/relatórios; não aprovar próprio código |

Ordem de integração: contratos → testes/harness → instrumentação → segurança/embedding → otimização, sempre por PR. Se um experimento exigir API comum, **negociar o contrato antes da implementação paralela**.

### 6.5 Política de continuidade para code agents

Ao final de cada sessão, registrar:

- Contexto e decisão anterior.
- Branch/worktree/commit e estado de trabalho.
- Tarefas concluídas, com evidências.
- Comandos executados e seus resultados reais.
- Bloqueios e incertezas.
- Próxima tarefa atômica, arquivos autorizados e critérios de término.

Não confundir **gerar código** com **terminar uma tarefa**: uma tarefa só está concluída após validações aprovadas ou após registrar explicitamente que não puderam ser executadas e por quê.

---

## 7. Plano de testes e oráculos

### 7.1 Uma regra fundamental: Lua não é oráculo de semântica Aipo

O omniLua utiliza a Lua C original como referência porque é um port de Lua. A Aipo **não é um port de Lua**. Seu oráculo primário será a especificação da Aipo, materializada no corpus de conformidade, testes e versões aprovadas. O omniLua pode ser oráculo **do algoritmo/implementação que está sendo estudado** quando as operações forem semanticamente equivalentes, nunca do comportamento linguístico completo.

Utilize três famílias separadas:

- **Conformidade Aipo:** especificação ↔ VM ↔ JS ↔ Wasm e demais backends.
- **Regressão histórica Aipo:** baseline aprovado ↔ implementação candidata, distinguindo correção de bug de quebra contratual.
- **Teste de conceito omniLua:** pequenas rotinas equivalentes (GC invariants, manipulação de handles, metering), comparadas somente quando contratos coincidem.

### 7.2 Pirâmide obrigatória

| Categoria | Exemplos | Quando rodar |
|---|---|---|
| Unidade | lexer, heap, valores, opcodes | Cada mudança |
| Golden/semântica | programas curtos e erros esperados | Cada mudança |
| Diferencial entre backends | valores, efeitos, exceções e saídas | Mudanças de semântica/runtime |
| Property-based | parse/serialize, comparadores, hashing | Por subsistema |
| Fuzz | source malformado, bytecode, host boundary | Segurança/VM/parser |
| Adversarial | loops, recursão, GC, escapes, capabilities | Sandbox/GC/embedding |
| Leak/lifecycle | criar e destruir VM, callback, module, tasks | GC/embedding/hot reload |
| Integration smoke | CLI, Rust host, plugin, Wasm real | Cada release candidata |
| Benchmark | cold/warm, throughput, latência e memória | Cada mudança de desempenho |

### 7.3 Comparação diferencial responsável

Comparar **resultados tipados**, saída, exceções, efeitos e estado final, não só `stdout`. Normalizar apenas propriedades explicitamente não determinísticas pela especificação; **não normalizar mensagens/valores só para esconder divergências**. Para reproduzir condições, congelar seeds, relógio, locale, corpus, builds e capacidades. Registrar casos reduzidos e reproduzíveis.

### 7.4 Gates de qualidade

Antes de considerar uma alteração pronta:

- Compilação (`cargo check` ou equivalente no workspace real) aprovada.
- Testes unitários relevantes e suíte completa aprovada; divergências pré-existentes destacadas.
- Corpus de conformidade da Aipo e paridade de backends afetados aprovados.
- Casos adversariais de segurança/memória adicionados e aprovados quando aplicáveis.
- Release benchmark comparado a baseline fixo com variância e ambiente registrados.
- Nenhum panic novo em input não confiável nem escape de memória/alocação sem tratamento.
- Documentação, notas de proveniência e ADR atualizados.
- Revisão independente aprovada para GC, `unsafe`, embedding e sandbox.

Se algum gate não puder rodar, entregue como **NÃO VERIFICADO**, com risco residual, em PR isolado; nunca apresente como pronto para produção.

---

## 8. Protocolo de benchmarks

Não medir apenas Fibonacci ou loops numéricos. A Aipo será usada como linguagem embarcada, de plugins e ferramentas; a distribuição de cargas importa mais que um pico artificial de throughput.

**Conjunto mínimo, com dados de entrada versionados:**

1. Lexer/parser de arquivos pequenos, médios e grandes.
2. Compilação cold e warm, imports e cache.
3. Loop aritmético, branches, dispatch de opcodes.
4. Funções curtas, nested calls, closures e recursão.
5. Strings Unicode, concatenação, slicing, normalização (quando aplicáveis).
6. Arrays/maps: construção, iteração, lookup, atualização e churn.
7. Alocação intensa e GC com alta proporção de objetos vivos.
8. Chamadas host↔Aipo com payloads pequenos/grandes.
9. Execução de script por frame (micro-tarefas em game/editor).
10. Startup de runtime, criação de 1000 estados, destruição e vazamentos.
11. Plugins não confiáveis com metering ligado versus desligado.
12. Wasm real: instância, execução, callbacks e memória.
13. Cross-backend JS/Wasm/VM para mesma carga sem assumir throughput equivalente.

**Medições:** tempo total, instruções/seg, p50/p95/p99 quando houver latência, CPU, RSS/peak, bytes vivos, volume de alocação, GC pauses, tamanho de binário/Wasm, tempo de inicialização, tempo de build; se possível contadores de CPU com perfil/atribuição.

**Método:** build release otimizada, pin de commit e rustc, warmup especificado, amostras repetidas, pares A/B intercalados para reduzir drift, nenhum processo competidor deliberado, apresentar mediana/intervalo de dispersão e workloads longos. Não prometer ganho quando resultados estiverem dentro da margem de ruído. Reverter otimizações com ganho incerto e custo de manutenção elevado.

**Critérios de performance:** estabelecer limites de regressão após baseline. Padrão sugerido para revisão, não regra universal: regressão consistente >5% em workload crítico requer bloqueio e análise; ganho <3% isolado não justifica complexidade substancial sem outros benefícios. Ajustar conforme ruído, perfil do produto e testes reais.

---

## 9. Ordem de execução: etapas, tarefas e decisões

### Fase 0 — Estado real e riscos existentes (obrigatória, sem refatorações)

**Entregas:** `AIPO_CURRENT_STATE.md`, `OMNILUA_SOURCE_MAP.md`, `BASELINE.md`, `SEMANTIC_COMPATIBILITY.md`, `RISKS.md`. Mapear commits e licenças. Rodar checks disponíveis; inventariar implementado / parcial / planejado. Não executar mudanças extensas ainda.

**Gate:** inventário com evidência por afirmação, baseline reproduzível e conflitos de especificação explicitados.

### Fase 1 — Ensinar os agentes a validar corretamente (P0)

Estudar `CLAUDE.md`, `PORTING.md`, `harness/`, `specs/` e `ANALYSES/` do omniLua. Incorporar à Aipo:

- Manual de leitura e precedência de especificação.
- Workflow de bugs: reproduzir → minimizar → fix mínimo → validar adjacentes.
- Registros de evidência por commit.
- Separação de ambientes e worktrees.
- Oráculo da Aipo e corpus de conformidade; não usar oráculo Lua.
- Integração de testes de GC, leaks, sandbox e semântica.
- Relatórios e gates de PR.

**Gate:** um agente novo consegue reproduzir os checks e saber onde encontrar a fonte canônica, sem depender de conversa.

### Fase 2 — Instrumentação e hardening de fronteiras (P0)

Implementar somente pontos de observabilidade comprovadamente ausentes; montar harness de benchmark; criar testes de host embedding, módulos, leaks e recursos; separar host services da VM se houver acesso ambiente indevido. Medir overhead dos contadores.

**Gate:** métricas reproduzíveis, controles desligáveis sem custo grande de execução normal e nenhuma regressão semântica.

### Fase 3 — Primeiro piloto: limites de execução e capabilities (P0)

**Hipótese:** arquitetura de metering e sandbox baseada em mecanismos demonstrados no omniLua pode tornar a Aipo mais segura em jogos, ferramentas e plugins.

**Escopo mínimo:** orçamento de instruções ou passos no interpretador, erro fatal/abort com identidade própria, teste de loop infinito e protected error/reentrada se presentes. Em outro PR, capability profiles/host hooks. Depois orçamento de memória com enforcement proporcional às garantias reais do heap. Sem prometer hard cap se apenas sampling.

**Gate:** escape tests, cancelamento correto, sem bloqueio permanente do host nas cargas cobertas, overhead medido, diferenças da plataforma documentadas.

### Fase 4 — Segundo piloto: embedding scoped (P0)

**Hipótese:** adaptar lifetime-scoped host userdata e typed conversions melhorará segurança/ergonomia da API Aipo.

**Escopo mínimo:** construir exemplo real `Host World → script → Host World`, callback e objeto emprestado, uso após scope deve falhar sem dangling, erro estruturado. Preservar interfaces atuais ou fornecer migração versionada, nunca quebrar usuários sem ADR.

**Gate:** suíte de empréstimo/GC/reentrância e exemplo documentado funcionando com testes.

### Fase 5 — GC e representação: experimentos só após profiling (P1)

Escolher **um** problema medido: leak, rooting, elevada pressão de GC, muitas clones, pausas, alocação por valor ou alto RSS. Formular duas alternativas e testar em branch isolada. Só copiar/adaptar algoritmo de omniLua se continuar equivalente aos contratos Aipo.

**Gate:** melhorias e riscos medidos, invariantes documentadas, nenhum ciclo/finalizer/handle comprometido.

### Fase 6 — VM e compilador: otimizações incrementais (P1)

Identificar hotspots reais; otimizar isoladamente dispatch, string/map ops, closures, call frames ou cache, mantendo bytecode e debug/contract parity. Não redesenhar instruções globalmente sem RFC. Tratar mudanças de bytecode versionadas e testadas.

**Gate:** ganho consistente em workloads relevantes + ausência de regressão de compilação, memória e todos os backends afetados.

### Fase 7 — Wasm, distribuição e documentação (P1/P2)

Reproduzir uso em Wasm com runtime real, integrar exemplo de plugin, instruções de embed e modelo de capabilities; documentar dependências, limitações e API. Auditar se tecnologia do omniLua para CLI/npm realmente é útil à Aipo; não criar nova superfície pública sem necessidade.

**Gate:** smoke real em target suportado; guias de migração, exemplos e CI funcionando.

### Fase 8 — Consolidação e backlog (P2)

Auditar resultados, remover experiências abandonadas, revisar dependências e área de `unsafe`; completar relatórios e priorizar backlog pelo risco/benefício. Registrar o que **não** foi aproveitado e por quê.

---

## 10. Primeiros experimentos específicos recomendados

### EXP-001 — Oráculo e conformidade

- **Objetivo:** tornar conclusões de agentes verificáveis.
- **Fonte omniLua:** `CLAUDE.md`, `harness/`, `specs/oracle/`, `ANALYSES/`.
- **Intervenção Aipo:** harness para corpus canônico da Aipo e matrix de backends.
- **Métrica:** reproduzibilidade, taxa de divergência detectada e tempo até repro mínima.
- **Não fazer:** comparar resultados Aipo diretamente com Lua.
- **Decisão prevista:** `INSPIRE`.

### EXP-002 — Budget à prova de evasão

- **Objetivo:** limitar scripts não confiáveis nos caminhos efetivamente suportados.
- **Fonte omniLua:** sandbox exploration, `state.rs`, `debug.rs`, protected calls.
- **Intervenção Aipo:** contador/fuel integrado à execução, propagação de aborto para chamadas protegidas e tasks/async conforme a arquitetura real.
- **Métrica:** scripts adversariais abortam, tempo de detecção e custo no hot path.
- **Não fazer:** declarar preempção de callbacks nativos sem enforcement específico.
- **Decisão prevista:** `ADAPT`.

### EXP-003 — Empréstimos de host com lifetime explícito

- **Objetivo:** prevenir uso de objeto do host após seu escopo.
- **Fonte omniLua:** `Lua::scope`, callback/userdata APIs.
- **Intervenção Aipo:** API scoped ou mecanismo equivalente adequado à API atual.
- **Métrica:** cobertura de use-after-scope, complexidade da API e overhead por chamada.
- **Não fazer:** transplantar a interface nominal Lua à força.
- **Decisão prevista:** `ADAPT`.

### EXP-004 — Live-heap e desmontagem sem leaks

- **Objetivo:** saber se o runtime atual retém memória indevidamente.
- **Fonte omniLua:** `crates/lua-gc/`, leak canaries, alocador de contagem.
- **Intervenção Aipo:** testes de lifecycle sem alterar algoritmo de GC.
- **Métrica:** bytes vivos após criar/destruir VMs, objetos, callbacks e plugins; GC pause e RSS.
- **Não fazer:** substituir GC antes do diagnóstico.
- **Decisão prevista:** `INSPIRE/ADAPT`.

### EXP-005 — Custos reais de dispatch

- **Objetivo:** identificar um hotspot legítimo da VM.
- **Fonte omniLua:** `crates/lua-vm/`, `harness/bench/`.
- **Intervenção Aipo:** micro-otimização isolada (se medição justificar).
- **Métrica:** throughput VM, chamada/retorno, p95, code size, cross-backend semantics.
- **Não fazer:** reescrever VM em registradores.
- **Decisão prevista:** `ADAPT/DEFER`.

---

## 11. Modelo de relatório por candidato a reaproveitamento

Copie e preencha para **cada** técnica estudada:

```markdown
# Opportunity: AIPO-OMNI-XXX — [Nome]

- Status: HIPÓTESE | VERIFICADO | PROTOTIPADO | INTEGRADO | REJEITADO
- Aipo commit: [SHA]
- omniLua commit: [SHA]
- Decisão proposta: COPY | ADAPT | INSPIRE | DEFER | REJECT
- Prioridade: P0 | P1 | P2 | P3
- Responsável: [agente/pessoa]
- Fonte Aipo: [path:linhas + link commit]
- Fonte omniLua: [path:linhas + link commit]
- Semântica/ADR canônico Aipo: [links]

## Problema real
[Comportamento atual, reprodução, frequência, evidência]

## Implementação de referência
[Algoritmo, invariantes, dependências, API, edge cases]

## Compatibilidade
[O que coincide, diverge e jamais pode ser copiado]

## Alternativas
1. Não alterar.
2. Refatoração mínima independente.
3. Copiar (com licença).
4. Adaptar/inspirar.

## Prova e métricas
[Comandos executados, entrada, ambiente, medidas com dispersão]

## Riscos e mitigação
[Semântica, GC, async, API, segurança, Wasm, manutenção]

## Testes novos e regressões
[Arquivos, comandos, resultados]

## Licenças e provenance
[MIT/NOTICE/trechos derivados/dependências]

## Decisão e justificativa
[Integrar / Não integrar / Rever com condições]
```

---

## 12. Estrutura sugerida dos artefatos dentro da Aipo

**Não crie estas pastas se já existir organização equivalente.** Primeiro descubra a estrutura documental atual; integre a ela. Uma sugestão, para facilitar coordenação:

```text
docs/
  studies/
    omnilua/
      README.md                     # mapa e estado do estudo
      AIPO_CURRENT_STATE.md        # inventário auditado
      OMNILUA_SOURCE_MAP.md        # inventário do projeto de referência
      SEMANTIC_COMPATIBILITY.md    # matriz Aipo vs Lua
      BASELINE.md                  # comandos, hashes e métricas
      OPPORTUNITIES.md             # matriz COPY/ADAPT/INSPIRE/etc.
      SECURITY_THREAT_MODEL.md     # limites do sandbox
      RISKS.md                     # riscos e mitigação
      DECISIONS.md                 # índice de decisões
      FINAL_REPORT.md              # resultado cumulativo
      opportunities/
        AIPO-OMNI-001.md
        AIPO-OMNI-002.md
      experiments/
        EXP-001.md
        EXP-002.md
        EXP-003.md
      provenance/
        THIRD_PARTY_CODE.md         # código copiado/derivado e obrigações
      benchmarks/
        README.md
        baseline.json
        results/                  # artefatos versionados apenas se apropriado
```

**Nunca** adicionar código do omniLua indiscriminadamente ao repositório Aipo como `vendor/` sem necessidade e decisão explícita. Para pesquisa, use checkout externo fixado em commit.

---

## 13. Checklist de auditoria completo para o agente

### Arquitetura, semântica e design

- [ ] Identifiquei todas as crates e fronteiras da Aipo atual e seus owners lógicos.
- [ ] Localizei especificação e ADRs vigentes.
- [ ] Comparei features documentadas versus realmente executadas.
- [ ] Mapeei dependências circulares e APIs internas excessivamente públicas.
- [ ] Identifiquei quais decisões Aipo divergem estruturalmente de Lua.
- [ ] Registrei que **parser/gramática/tabelas/metatables Lua não são importação padrão**.
- [ ] Mantive projeto acessível, legível e não orientado a objetos por imposição.

### VM, compiler e bytecode

- [ ] Inventariei dispatch, stack frames, closures, upvalues e captura.
- [ ] Cobri bytecode malformado e invalidação de caches.
- [ ] Cobri limites de recursão e stack growth.
- [ ] Medi hot paths antes de otimizar.
- [ ] Testei VM e backends alternativos para casos semanticamente afetados.

### Memória e GC

- [ ] Identifiquei todos os roots e handles de objeto host.
- [ ] Analisei ciclos, weak references, finalização e mutação durante coleta.
- [ ] Verifiquei leaking de runtimes descartados, plugins e hot reload.
- [ ] Produzi testes de pressão, cleanup e alocação falha.
- [ ] Justifiquei qualquer `unsafe` introduzido.

### Segurança, embedding e async

- [ ] Documentei threat model por perfil de uso.
- [ ] Verifiquei todas as rotas de IO, rede, processo e ambiente.
- [ ] Entendi se metering abrange funções protegidas, tasks e callbacks.
- [ ] Distingui aborto, suspensão e cancelamento cooperativo.
- [ ] Testei lifetimes da API de host e reentrância.
- [ ] Não anunciei proteção absoluta de sandbox in-process.
- [ ] Reconheci os casos em que isolamento de processo/Wasm seria necessário.

### QA, performance, licenças e entrega

- [ ] Fixei SHA da Aipo e do omniLua utilizados.
- [ ] Classifiquei toda afirmação como verificada/declarada/hipótese.
- [ ] Criei baseline e oráculos adequados à Aipo.
- [ ] Avaliei todos os candidatos por `COPY/ADAPT/INSPIRE/DEFER/REJECT`.
- [ ] Registrei testes executados, falhas e skips sem ocultação.
- [ ] Apresentei benchmarks com ruído/ambiente e sem ganhos inventados.
- [ ] Verifiquei a licença da Aipo e notices obrigatórios do código derivado.
- [ ] Produzi PRs pequenos e independentes.
- [ ] Atualizei documentação operacional e final_report.

---

## 14. Instrução de trabalho para usar este documento como prompt principal

> **Agente, execute esta diretriz por etapas.** Comece pela Fase 0, lendo integralmente o código e as especificações atuais da Aipo e o código do omniLua no commit fixado. Não invente a arquitetura da Aipo. Produza o inventário fundamentado e a matriz de oportunidades; em seguida implemente primeiro infraestrutura de validação e experimentos mínimos de maior impacto. Para cada mudança, apresente contrato, referência, risco, diffs pequenos, testes, benchmarks e proveniência. Preserve a semântica Aipo, a paridade de backends, a filosofia de legibilidade, a segurança do host e os fluxos atuais do repositório. Rejeite importações desnecessárias do modelo Lua. Não declare resultados sem executá-los. Mantenha status e próximos passos em arquivos persistentes e, quando uma decisão arquitetural for necessária, registre ADR e escolha justificada antes de grandes refatorações. **Não faça merge/publicação sem autorização.**

### Ordem operacional da primeira execução

1. Ler `AGENTS.md`/`CLAUDE.md`, README, specs/ADRs da Aipo, `Cargo.toml` e testes.
2. Fixar commits da Aipo e omniLua; validar licenças e NOTICE.
3. Rodar (ou registrar impedimento) baseline e suíte atual da Aipo.
4. Preencher `AIPO_CURRENT_STATE.md` e `SEMANTIC_COMPATIBILITY.md`.
5. Ler o `CLAUDE.md` vigente do omniLua, o `PORTING.md` histórico e guias relevantes das crates.
6. Preencher `OMNILUA_SOURCE_MAP.md` e `OPPORTUNITIES.md` com paths/SHAs.
7. Identificar **três candidatos P0** com teste e métrica definidos.
8. Implementar primeiro o harness/oráculo, caso não exista equivalente suficiente.
9. Fazer piloto pequeno de sandbox **ou** embedding, conforme o inventário demonstrar maior lacuna.
10. Encerrar a rodada com PR isolado (sem merge), relatório de testes, resultados e backlog.

**Critério de sucesso da missão:** não é o número de linhas reutilizadas, nem a porcentagem de código parecida com omniLua. É a melhoria **comprovada** de Aipo em confiabilidade, desempenho, segurança e facilidade de integração, com manutenção de seus próprios contratos.

---

## 15. Fontes e referências obrigatórias

As referências abaixo foram verificadas em páginas públicas em **11/10/2026**; links para diretórios podem exigir acesso diretamente por Git/GitHub. O agente deve fixar os respectivos SHAs e confirmar se guias ou arquivos mudaram.

### omniLua — implementação, processos e fundamentos

- Repositório principal e README: <https://github.com/ianm199/omnilua>
- Manifesto de workspace e lista de crates: <https://github.com/ianm199/omnilua/blob/main/Cargo.toml>
- Guia operacional atual para agentes: <https://github.com/ianm199/omnilua/blob/main/CLAUDE.md>
- Guia histórico de tradução Lua C→Rust, a ler com ressalvas: <https://github.com/ianm199/omnilua/blob/main/PORTING.md>
- Licença: <https://github.com/ianm199/omnilua/blob/main/LICENSE>
- NOTICE e proveniência da Lua original: <https://github.com/ianm199/omnilua/blob/main/NOTICE>
- Estudo detalhado do sandbox (metering, memória, erro e limitações): <https://github.com/ianm199/omnilua/blob/main/docs/SANDBOXING_EXPLORATION.md>
- Issue/design de embedding: <https://github.com/ianm199/omnilua/issues/2>
- Issue/design de sandbox: <https://github.com/ianm199/omnilua/issues/18>
- Issue sobre Wasm e diferenças entre `cargo check` e execução real: <https://github.com/ianm199/omnilua/issues/1>
- Harness e benchmark: <https://github.com/ianm199/omnilua/tree/main/harness>
- Especificações e oracle: <https://github.com/ianm199/omnilua/tree/main/specs>
- Crates: <https://github.com/ianm199/omnilua/tree/main/crates>
- Análises e tabelas de referência: <https://github.com/ianm199/omnilua/tree/main/ANALYSES>

### Aipo — fonte de verdade a inspecionar no checkout real

- Repositório: <https://github.com/poppy-team/aipo-lang>
- Também conferir eventual redirecionamento de organização/espelho: <https://github.com/poppyTM/aipo-lang>
- Specs, ADRs, guias de implementação, testes de conformidade, Cargo workspace, CI e guias de agentes: **descobrir pela estrutura real; não assumir caminhos**.

### Fonte de semântica Lua, apenas quando relevante para interpretar o omniLua

- Site Lua: <https://www.lua.org/>
- Manual de referência da Lua: <https://www.lua.org/manual/>

---

## 16. Registro explícito de incertezas a resolver na auditoria

- Qual é o commit atual e o repositório canônico da Aipo? O clone é acessível?
- Que backends estão completos, parciais ou experimentais? Há realmente parity VM↔JS↔Wasm em toda a linguagem?
- Qual é o gerenciamento de memória vigente? GC, Rc/Arc, arenas ou combinação?
- Qual o comportamento normativo de strings, inteiros, overflow, arredondamento e igualdade?
- Quais erros são capturáveis, quais são fatais e que rollback realmente existe?
- Há execução async, agendamento cooperativo e limites de execução em produção?
- Existe API de embedding documentada? Há suporte seguro a userdata/callbacks emprestados?
- Quais capacidades de host são permitidas implicitamente hoje?
- Quais regressões e gargalos foram medidos, não apenas intuídos?
- Qual é a licença/NOTICE da Aipo, sua política de dependências e proveniência?
- Que arquivos do omniLua continuam válidos após as mudanças de 2026, especialmente GC, runtime e sandbox?

**Até responder a essas questões, nenhuma decisão de copiar ou substituir componentes internos é definitiva.**

---

*Fim da diretriz — versão 1.0, criada como instrução técnica de execução, não como relatório de auditoria já concluída.*
