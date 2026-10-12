# Runtime, embedding e ferramentas: uso e reimplementação

**Rodada:** P07-G02 · **Data:** 2026-10-09 · **Base:** `f0a0d70d176be031cd4b600fde1df6179429fb9a`.

Este guia descreve a implementação das cinco frentes solicitadas após a revisão P07-G01. A implementação está disponível para revisão; o objetivo Prumo permanece `DRAFT`. **Os testes não foram executados, por instrução explícita do usuário.** Compilar os alvos de testes confirma que eles compilam; não confirma seus resultados. Os exemplos abaixo são instruções de uso, não relatos de execução.

Leia também a [evidência desta rodada](../evidence/P07-G02/README.md), o [estudo de fontes](../studies/runtime-and-tooling-follow-up.md), os [perfis reais](../architecture/embedded-and-shell-profile.md) e o [guia anterior](runtime-hardening-guide.md). A documentação é Markdown dentro do repositório, disponível pelo GitHub.

## 1. Entrega das cinco frentes

| Frente | Implementação | Limite relevante |
|---|---|---|
| 1. Correção e segurança | Verifier Reg completo para os operandos existentes, inicialização definida e escopos; revisões estruturais de coleções; `is` conservador no Wasm | Não constitui certificação por fuzzing/Miri ou execução da conformance |
| 2. Cobertura e embedding | `ExecutionPlan` explícito; linker canônico; budgets Reg/C; execução cooperativa C; limites e cache Wasm | Closures, async, hooks e interfaces continuam nos serviços canônicos quando o Reg nativo não os suporta |
| 3. Sessão, shell e perfis | REPL persistente, entrada multilinha, histórico, completion por comando, recarga com migração e rollback; aliases de features | Shell usa o SO; não implementa job control POSIX completo; `embedded` usa `std` |
| 4. Performance | Janelas de registradores vivos, funções compartilhadas, constantes preparadas por módulo; benchmark pareado reproduzível | Ganho medido apenas nos workloads descritos na evidência |
| 5. Ferramentas e distribuição | `new`, `watch`, `profile`, `debug`, `plan`, LSP stdio, vendor offline, empacotador, instalador e workflows | LSP básico; workflows não certificam plataformas antes de rodar; nenhuma release pública publicada |

## 2. Instalação e perfis

O MSRV passa de **Rust 1.85 para 1.96**. O lockfile existente inclui Wasmtime 49.0.2 e Cranelift 0.136.2, que declaram Rust 1.96. A declaração anterior não representava esse grafo de dependências. A mudança também exigiu ajustes de estilo apontados pelo Clippy, principalmente simplificação de condicionais; os lints não foram enfraquecidos.

```sh
# Toolchain mínimo do workspace
rustup toolchain install 1.96.0 --profile minimal

# CLI completa: formatter, JS, emissor Wasm, runner Wasmtime, regex e Unicode extra
cargo +1.96.0 build --locked -p aipo-cli --release

# Shell sem JIT, com formatter, regex e Unicode extra
cargo +1.96.0 build --locked -p aipo-cli --release \
    --no-default-features --features profile-shell

# Emissores JS/Wasm, sem Wasmtime
cargo +1.96.0 build --locked -p aipo-cli --release \
    --no-default-features --features profile-web

# CLI mínima nativa, ainda com std
cargo +1.96.0 build --locked -p aipo-cli --profile nano \
    --no-default-features --features profile-embedded
```

Use `--no-default-features` para remover o perfil completo. Acrescentar `profile-shell` aos defaults não remove o JIT. `nano` é um perfil Cargo de otimização do binário; não é um backend nem um porte para microcontrolador. As normalizações fundamentais continuam seguindo o canon; a feature `unicode` habilita operações adicionais.

Não há promessa de tamanho de `Value`, RAM mínima ou startup em milissegundos. Use `crates/aipo-vm/examples/layout_probe.rs` e medições do alvo real para obter esses números.

## 3. Escolha de execução: VM e Reg

```sh
aipo run src/main.aipo
aipo run src/main.aipo --engine=reg
aipo plan src/main.aipo
```

`aipo plan` compila sem executar e escreve JSON com `engine` e `reason`. O plano usa Reg nativo somente quando o emitter e o verifier aceitam todo o IR. Construção e acesso a membros usam o plano canônico para compartilhar contratos, métodos e serviços da VM. A ausência de uma capacidade Reg gera uma razão explícita para o plano canônico; nenhuma instrução é descartada para simular suporte.

`analyze_to_reg_module` continua sendo a API estrita: retorna erro quando falta suporte nativo. `analyze_to_execution_plan` retorna a alternativa explicitamente tipada. Hosts como `--host=headless-test` usam o contexto canônico mesmo no perfil Reg, porque seus callbacks dependem desse contexto.

Isso entrega cobertura ao **perfil de execução**, preservando a semântica da linguagem. Não significa que async, closures/upvalues, interfaces operacionais, invariantes e journal foram reimplementados no interpretador Reg nativo. A VM canônica continua sendo a referência dessas capacidades.

### Verificação Reg

`crates/aipo-bytecode/src/reg_verifier.rs` faz duas análises:

1. Valida todos os operandos, inclusive instruções inalcançáveis: índices de registrador, janelas de argumentos, pools, constantes String exigidas, referências de função, saltos e projeções de iteração. Também rejeita constantes fora dos números seguros da linguagem.
2. Percorre o CFG com um estado de inicialização por registrador e escopos de handlers/iterações. Parâmetros começam inicializados; joins usam interseção. Um registrador lido deve estar inicializado em todos os caminhos. As entradas excepcionais são conservadoras e recuperam o escopo instalado pelo handler. Underflow de escopo e fallthrough alcançável sem retorno são erros.

`RegEmitter::compile_module` verifica o resultado. `RegVm::run_module`, `run_function`, `invoke_function` e a entrada raw verificam suas respectivas fronteiras antes de efeitos guest. Na entrada raw os registradores do host começam inicializados, inclusive os que contêm `none`. A prova de um módulo é reutilizada apenas durante sua execução; um guard RAII restaura esse estado também durante unwind.

Para adicionar um opcode: atualize o formato, emitter, operandos do verifier, efeitos sobre o CFG, execução e casos de conformance. Não confunda posição em bytes do bytecode de pilha com índice de instrução Reg. Cada instrução Reg ocupa `u32`; o formato usa opcode de 7 bits, A de 8 bits, B de 9 bits e C de 8 bits, ou Bx/sBx de 17 bits conforme a instrução.

## 4. Coleções e mutação durante iteração

Antes, comparar somente comprimento permitia que remoção seguida de inserção passasse despercebida em uma fronteira de callback. Agora `Collection<T>` acompanha revisão estrutural. `DictMap` possui revisão equivalente.

| Operação | Revisão |
|---|---|
| Inserir/remover elemento ou chave, limpar coleção, reorganizar sua estrutura | Incrementa |
| Substituir valor em posição existente ou valor de uma chave já presente | Preserva |
| Comparar duas coleções com os mesmos elementos e ordem | Ignora a revisão |

Os guards guardam identidade e revisão. A VM canônica preserva o estado por task e remove guards nos retornos e unwinds; os métodos de ordem superior verificam também o retorno de callbacks nativos. O Reg preserva a profundidade de guards nos handlers. Elementos já existentes continuam podendo ser substituídos conforme o canon.

**Migração da API Rust:** `List`, `Set` e `Bytes` passaram de `Rc<RefCell<Vec<_>>>` para `Rc<RefCell<Collection<_>>>`. Use os construtores ou converta um `Vec` com `.into()`:

```rust
use aipo_vm::Value;
let items = Value::list(vec![Value::Int(1), Value::Int(2)]);
let bytes = Value::bytes(vec![1, 2, 3]);
let text = Value::string("Aipo");
```

`Value::set` recebe elementos já deduplicados. O acesso mutável expõe elementos de uma slice, sem permitir redimensionar por um `DerefMut<Vec<_>>`. Extensões do host devem usar os métodos estruturais de `Collection`. A tabela pública de funções Reg usa `Vec<Rc<RegCompiledFunction>>`; adapte clientes que a construíam manualmente.

## 5. Sessão persistente e REPL

```sh
aipo-sh
```

```aipo
var count = 0
fn next() {
    count += 1
    return count
}
next()
next()
```

A sessão compila a unidade nova, mantém um catálogo de declarações, reloca seu bytecode e executa somente a nova entrada. Corpos antigos permanecem disponíveis para valores Function/Closure que ainda os referenciam. Declarações antigas podem ser compiladas para o catálogo atual, mas statements antigos não são repetidos; o REPL não reproduz todo o histórico para reconstruir estado.

`run_persistent_at` preserva Tasks, resultados, filas, waiters, joins, grupos, IDs e tempo virtual entre unidades. Assim, uma tarefa criada em uma unidade pode ser aguardada em outra. `run_at` continua sendo a entrada independente que reinicializa o scheduler. A entrada persistente exige bytecode ligado append-only, mantendo os offsets antigos válidos.

O snapshot de definições inclui o scheduler. O heap snapshot também acompanha resultados, argumentos de tarefas de host, journals e células de upvalue retidas por tarefas, preservando aliases no rollback. Resultados/tarefas e código antigo ainda podem crescer em sessões longas; coleta de alcance e ciclos Rc permanece trabalho futuro.

| Comando REPL | Uso |
|---|---|
| `:help` | Exibe comandos |
| `:history` | Exibe unidades anteriores |
| `:complete PREFIXO` | Lista globals com esse prefixo |
| `:load CAMINHO` | Avalia uma unidade |
| `:reload CAMINHO` | Substitui declarações desse arquivo e preserva estado compatível |
| `:reset` | Limpa a sessão |
| `:quit` | Encerra |

O lexer controla balanceamento de delimitadores, ignorando strings e comentários, para entrada multilinha. Em entrada redirecionada não há prompts; falhas resultam em exit code diferente de zero. `aipo-sh --engine=reg` usa a sessão canônica persistente e identifica essa escolha no banner.

O histórico usa JSONL, até 1.000 unidades e limite de leitura de 8 MiB. Arquivos maiores são ignorados. O caminho padrão usa `XDG_STATE_HOME`, ou `HOME/.local/state`, seguido de `aipo/history.jsonl`. `AIPO_HISTORY_FILE` troca o caminho; valor vazio desliga a persistência. A gravação usa temporário, sync e rename; em Unix o arquivo nasce com modo `0600`. A interface ainda não inclui editor de linha com navegação por setas e completion por Tab.

### Linker

`append_unit`, em `aipo-bytecode/src/linker.rs`, verifica o novo módulo, trabalha em uma cópia candidata, deduplica pools e reloca índices de nomes, constantes, funções, closures e contratos. Atualiza entradas de funções e spans. Saltos relativos internos não mudam. Verifica limites do formato e o módulo ligado antes de substituir a imagem original. `function_by_name` busca a definição mais recente.

Para reimplementar, preserve índices de código publicados: compactar/remover unidades antigas invalida referências ainda vivas. A imagem ligada cresce durante a sessão; recolher unidades antigas requer uma política de alcance/identidade futura. A deduplicação atual percorre pools linearmente, podendo ser melhorada com índices de interning após medição.

## 6. Hot reload e migração

```sh
aipo watch src/main.aipo
```

`watch` acompanha arquivos `.aipo`, `.toml`, `.lock` e `.ahs` na árvore do pacote que contém a entrada, ou no diretório da entrada sem manifest. Ignora symlinks, `.git`, `target` e `node_modules`; limita profundidade a 64 e quantidade a 10.000 arquivos. Usa hashes de conteúdo e polling de 250 ms. Dependências de caminho fora dessa árvore exigem outro watcher ou chamada explícita à API de recarga.

`Session::reload_with` executa estas etapas:

1. Captura definições, scheduler e heap guest alcançável antes de remover declarações do arquivo.
2. Compila, liga e inicializa a candidata. Remoção de função/tipo deixa de expor a declaração antiga.
3. Mantém globals não chamáveis existentes que continuam declarados.
4. Chama a função de migração do host e verifica layouts de instâncias retidas, incluindo nomes, ordem e campos `fixed`.
5. Atualiza caches e confirma a geração, ou restaura código, catálogo, globals, scheduler e heap.

```rust
use aipo_cli::Session;
use aipo_vm::Value;
use std::path::Path;

let mut session = Session::new();
session.set_instruction_budget(Some(100_000));
let status = session.reload_with(
    "var count = 0\n", Path::new("counter.aipo"),
    &mut std::io::stdout(), &mut std::io::stderr(),
    |globals| {
        globals.entry("count".into()).or_insert(Value::Int(0));
        Ok(())
    },
);
assert_eq!(status, 0);
```

`HeapSnapshot` percorre o grafo uma vez por alocação: coleções, Dict, Bytes, structs, células de upvalue, receivers de métodos, payloads de Failure e dados/caches de Sequence. O rollback restaura **as próprias alocações Rc**, preservando aliases mantidos pelo host. Clonar somente o mapa de globals não bastava.

Uma migração incompatível precisa substituir ou adaptar instâncias alcançáveis, inclusive os seus campos `fixed`. Tipo removido com instância ainda viva é rejeitado. Inicializadores podem mutar estado antigo; isso é revertido se a candidata falhar. Em uma recarga bem-sucedida essas mutações participam do estado confirmado.

**Limite da transação:** arquivos, processos, rede, callbacks nativos e recursos opacos do host não são desfeitos por `HeapSnapshot`. Inicializadores devem evitar efeitos externos irreversíveis, ou o host deve oferecer sua própria transação. Não há coleta de ciclos Rc; snapshot não é garbage collector.

## 7. Embedding e budgets

Um runtime pertence a uma thread. `Value` usa Rc/RefCell e não é `Send`; não compartilhe uma instância mutavelmente entre threads. O host registra capacidades explicitamente. A fronteira C recusa reentrada durante callbacks.

### Rust

- `Session` oferece avaliação persistente, limite por unidade, leitura de globals, completion e migração.
- `Vm` oferece `run_at`, `start_at`, `debug_step`, `take_completion`, métricas e budget.
- `RegVm::set_instruction_budget(Some(n))` configura orçamento cumulativo e zera seu contador; `None` desliga a contagem. `reset_instruction_count` reinicia explicitamente.
- Na VM canônica, execuções budgetadas preservam consumo entre entradas; `reset_instruction_count` reinicia. `Session` reinicia antes de cada avaliação, oferecendo orçamento por unidade.

O contador C canônico acompanha execução quando budget ou métricas estão habilitados. Execução sem budget pode reiniciar o contador e não contabiliza opcodes com métricas desligadas; não é um contador universal de toda a vida do runtime.

Budget de instruções mede trabalho guest, não memória total, duração de callback nativo ou wall-clock. O host deve limitar operações externas separadamente.

### C: execução síncrona e cooperativa

O header é `crates/aipo-c-abi/include/aipo.h`. A ABI canônica mantém `aipo_runtime_load_module` e `aipo_runtime_call`; o runtime liga unidades e usa offsets relocados para evitar chamar código do módulo errado. Uma carga com erro recuperável restaura definições e heap. Layouts retidos incompatíveis são rejeitados.

As APIs adicionais permitem dividir a execução:

```c
bool completed = false;
aipo_value_t result;
aipo_status_t status = aipo_runtime_begin(rt, "demo", "main", NULL, 0);
while (status == AIPO_OK && !completed) {
    status = aipo_runtime_pump(rt, 1000, &completed, &result);
    /* Devolva controle ao event loop do host entre pumps. */
}
if (status == AIPO_OK && completed) {
    aipo_value_release(rt, result);
}
```

`begin` prepara a função como computação principal cooperativa, exige argumentos compatíveis e ausência de captures na entrada selecionada. `pump` executa no máximo a quantidade de quanta pedida; callbacks ainda podem bloquear. O valor só é válido quando `completed` é true. Load/call/begin recusam runtime ocupado. `aipo_runtime_abort` cancela essa computação e mantém mutações e I/O já concluídos; não faz rollback. Falhas e cancelamento mantêm sua distinção canônica. Panics unwind são contidos na fronteira; builds com `panic=abort` não podem ser recuperados por `catch_unwind`.

Para Reg raw, o header agora cobre lifecycle, int set/get, run, budget, contador/reset e `aipo_reg_vm_copy_last_error`. O último devolve o tamanho necessário incluindo NUL e permite consultar com buffer nulo/capacidade zero. Um buffer menor recebe uma cópia truncada com NUL. Int fora do intervalo seguro ou retorno diferente de Int quando `out_int` é solicitado gera erro; não há coerção silenciosa para zero.

Strings/Bytes retornados pela ABI canônica são snapshots do runtime. Libere com `aipo_value_release`. Não libere com `free`, não retenha após destruir o runtime e não passe pointers expirados, buffers curtos ou desalinhados. Checagens de null e tamanho não comprovam validade arbitrária de pointers C.

## 8. Wasm: semântica e recursos

O teste `is` usa tipo semântico conhecido, não o tipo de armazenamento Wasm. `i32` pode armazenar Bool ou pointer; `i64` não prova Int. None em `is T?` corresponde; demais valores usam comparação fundamental/nominal. Expressões, parâmetros sem prova, nullable, chamadas ambíguas ou bindings de elementos com tipo desconhecido geram diagnóstico orientando usar a VM.

Foi removido um ramo antecipado legado que usava tipos físicos e tornava a checagem semântica posterior inalcançável. Agora há um único caminho: provar o valor, resolver um destino estático não sombreado e avaliar o lado esquerdo uma vez, preservando seus efeitos. O catálogo de funções acompanha bindings globais e tipos declarados para não confundir nomes com o prelude.

Aliases como `let T = Int`, parâmetros/globals/funções que sombreiam `Int`, destinos desconhecidos e expressões dinâmicas no lado direito são rejeitados no Wasm com orientação para usar `--engine=vm`. `Function` não foi inventado como TypeTag: o nome de diagnóstico de um valor chamável não é um tipo fundamental disponível no prelude. Provas de chamadas `String`/`task.spawn` também exigem nomes não sombreados. Operações numéricas com operandos desconhecidos e concatenação de strings não recebem fatos de tipo falsos.

Para ampliar suporte, propague fatos de tipo pelo HIR e seus joins ou implemente tags dinâmicas reais. Não volte ao fallback que assume Int para qualquer expressão. A rejeição de um caso sem prova é uma limitação explícita, não um resultado `true` inventado.

Com `wasmtime`, `WasmExecutionOptions` oferece `fuel`, `memory_bytes` por memória linear e `output_bytes` para saída bufferizada. O excesso de saída é rejeitado antes de estender o buffer; memory grow excessivo gera trap. A execução entrega saída com checagem do writer. Esses limites não cobrem recursos externos do host ou toda a memória do processo JIT.

`WasmRunner` reutiliza Engine e módulos compilados em cache LRU limitado por quantidade. Cada execução cria um Store independente para memória, globals, fuel e saída. `set_cache_capacity(0)` desliga cache; `execute_wasm_with_options` permanece a entrada de conveniência. O runner opcional é Wasmtime nesta entrega; não houve troca por WAMR/wasmi nem certificação em MCU.

## 9. Ferramentas de desenvolvimento

| Comando | O que faz |
|---|---|
| `aipo new demo` | Cria manifest, `src/main.aipo` e README; não sobrescreve diretório existente |
| `aipo profile src/main.aipo --budget 100000 --json` | Executa na VM e informa tempo, instruções, calls, native calls e field cache |
| `aipo debug src/main.aipo` | Depurador terminal: `step`, `continue`, `break BYTE_OFFSET`, `globals`, `quit` |
| `aipo plan src/main.aipo` | Mostra engine e razão do plano, sem executar |
| `aipo watch src/main.aipo` | Recompila/recarga candidata mantendo a última geração válida |
| `aipo lsp` | Servidor LSP em stdin/stdout |

O depurador usa offsets de bytecode e um quantum do scheduler por passo, podendo alternar tasks. Mostra IP, span e stack. Uma falha de execução ou Failure não capturada resulta em exit code 1. Não implementa ainda DAP, inspeção lexical de todos os locals, stepping por linha ou avaliação de expressões no frame.

O LSP oferece `initialize`, shutdown/exit, full document sync, diagnósticos com posições UTF-16, completion de palavras/prelude/declarações e formatação quando a feature existe. Analisa o texto aberto, não apenas o arquivo salvo, e respeita versões de documento. Imports usam o resolvedor do projeto. Diagnósticos de outro arquivo são contextualizados por caminho/linha, sem fingir offsets do documento aberto.

O transporte verifica Content-Length, limita headers a 8 KiB e mensagens a 8 MiB. JSON inválido e request inválido têm respostas distintas. Métodos não suportados retornam erro explícito. Hover, rename, go-to-definition, semantic tokens, alterações incrementais e cancelamento de trabalho em andamento permanecem evolução futura. Configure o editor para iniciar `aipo lsp`; nunca envie logs pela stdout do protocolo.

## 10. Vendor offline e distribuição

```sh
aipo package lock caminho/do/pacote
aipo package vendor caminho/do/pacote --out vendor-demo
# Dependências GitHub já verificadas no cache:
aipo package vendor caminho/do/pacote --cache caminho/do/cache --out vendor-demo
aipo run vendor-demo/root/src/main.aipo
```

Vendor exige lock existente e grafo verificável. Não busca rede. Copia root/dependências para diretório novo, recusa symlinks, reescreve dependências para caminhos locais e produz lock local. `vendor-manifest.json` conserva o lock original e um inventário SHA-256. Ignora arquivos ocultos, `target` e `node_modules`; projetos que dependem desses assets devem empacotá-los por outro processo explícito. O stage vira destino por rename; erro remove o stage criado pela operação.

O resolvedor existente continua usando versões exatas e fontes pinadas. Vendor não acrescenta registry, publicação de pacotes ou solver de ranges SemVer.

```sh
# Execute em checkout limpo, depois do commit
python3 scripts/release/package.py --profile shell

# Verifique o SHA-256 e instale o arquivo local em prefixo explícito
python3 scripts/release/install.py target/dist/ARQUIVO.tar.gz --prefix /caminho/aipo
```

O empacotador constrói CLI, inclui licença e `build.json` com commit, target, compiler e perfil; `full` inclui header e bibliotecas C. Produz `.tar.gz` e checksum. Timestamp e ordem dos membros são controlados por `SOURCE_DATE_EPOCH`; isso torna a montagem do arquivo determinística para os mesmos payloads, sem prometer binários bit a bit idênticos entre máquinas.

O instalador verifica SHA-256 antes de extrair, aceita somente arquivos regulares em uma raiz, rejeita traversal, entradas duplicadas, symlinks e destinos symlink. Prepara arquivos, preserva backups e usa substituição por arquivo com rollback de falhas. `--force` permite substituir arquivos existentes. Não baixa nem executa scripts do pacote. Isso não é uma transação contra crash do SO entre várias substituições.

`profiles.yml` define compilação em Linux/macOS/Windows, Rust mínimo e stable, para shell/web/embedded. `package-artifacts.yml` é manual, gera artifacts por SO/perfil e não publica uma release. A execução desses workflows não foi observada nesta entrega; o resultado local não certifica as outras plataformas.

## 11. Performance: o que mudou e como reproduzir

O gargalo Reg era copiar todos os 256 registradores e clonar corpos de função/pools repetidamente. A ativação agora move e restaura somente a janela necessária ao caller e callee. Corpos usam Rc e constantes guest são preparadas ao carregar o módulo. Chamadas internas reaproveitam a prova do módulo; fronteiras externas continuam verificadas.

A entrada raw mantém janela completa para preservar registradores do host. O limite de profundidade permanece para proteger stack/RAM. A mudança não implementa uma arena compacta, NaN boxing ou dispatch threaded.

Use [os dados brutos e o procedimento](../evidence/P07-G02/README.md). O exemplo `reg_calls_bench` executa 20 mil chamadas com caller de nove instruções e um registrador no callee; casos Int e String realmente usados. O driver alterna baseline/candidata, fixa CPU quando solicitado, remove warmup e registra pelo menos dois lotes, compiler e hashes. Não extrapole o resultado para toda a linguagem, cold start, memória ou workloads de structs/async.

## 12. O que ainda melhorar e ordem sugerida

| Prioridade | Trabalho | Por que e critério de conclusão |
|---|---|---|
| P0, antes de certificar | Executar conformance, regressões, fuzz, Miri/ASan e matriz multiplataforma quando autorizado | Compilação não comprova efeitos, unwind, aliases, callbacks ou protocolo; registrar resultados reais por cenário |
| P1 | Reg nativo: captures, scheduler e hooks via contratos/serviços compartilhados | Reduzir fallback sem duplicar semântica; cada capacidade exige diferencial com VM e erro fechado enquanto incompleta |
| P1 | Ciclos Rc e retenção de unidades antigas | Sessões longas ainda podem acumular heap/código; definir identidade, alcance e lifecycle antes de compactar |
| P1 | Hot reload de recursos externos e migrações estruturais ergonomicamente tipadas | Guest snapshot não desfaz I/O; coordenar transaction hooks do host e imports fora da árvore |
| P2 | LSP/DAP completos e editor de linha | Navegação, scopes, locations, locals e cancelamento real tornam as ferramentas adequadas a IDEs grandes |
| P2 | Limites totais de heap/compilação e callback deadline | Budget de opcodes não é sandbox de memória ou CPU externa; instrumentar o allocator/host por perfil |
| P2 | Registry, publicação e resolução SemVer | O pacote atual é exato/pinado/offline; ranges e distribuição pública precisam política própria |
| P3 | `no_std`, allocator/arena real, HAL e XIP | Embedded atual é nativo com std; medir RAM/Flash e adaptar serviços em um alvo concreto antes de prometer MCU |
| P3 | Otimizações adicionais e backend alternativo Wasm | Exigem baseline ociosa, critérios funcionais e evidência; não escolher engine apenas pelo nome ou benchmark de outro projeto |

## 13. Reimplementação e revisão

Comece pelas fronteiras observáveis: resultado/diagnóstico, ordem de efeitos, identidade e lifespan dos valores, budget e comportamento em falha. Reutilize a semântica de `Value` e os serviços canônicos. Implemente na ordem: verifier/coleções → linker/snapshot → Session/reload → embedding → ferramentas → distribuição.

Confira os casos adicionados em `reg_verifier.rs`, `guest_transactions.rs`, `persistent_session.rs`, as fixtures 35/36 e as provas Wasm. Eles foram escritos para futuras execuções, **não executados nesta rodada**. Não reescreva snapshots para encobrir divergência. Os comandos efetivamente realizados e suas limitações estão na evidência; o objetivo não deve passar a `DONE` sem os gates e aprovações reais.
