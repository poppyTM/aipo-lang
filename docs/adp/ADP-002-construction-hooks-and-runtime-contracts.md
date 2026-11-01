# ADP-002 — Hooks de construção, invariantes e contratos em runtime

**Status:** resolved — G1, G2 (verificação na construção) e G4 fechados por `P00-G13`; G2b (pontos de mutação) e G3 (contratos em runtime, incluindo conformidade estrutural de interfaces) fechados por `P00-G14` e corrigidos/certificados por `P00-G15`
**Origem:** Slice S11 (`P00-G12`), corpus `docs/conformance/`
**Resolvido em:** `docs/evidence/P00-G13-construction-hooks-and-bytes.md` e
`docs/evidence/P00-G14-runtime-contract-enforcement.md`
**Autoridade:** subordinada a `docs/canon/Aipo V1 — Language Reference…`,
`docs/canon/Aipo V1 — Sintaxe Canônica Consolidada…` e
`docs/canon/Interlúdio — Chamadas, Construção, Impl e Módulos…`
**Complementa:** `docs/conformance/README.md`, `docs/evidence/P00-G12-conformance-and-mvp-gate.md`

A política de não invenção (`prumo.json` → `risk`) exige que toda lacuna entre canon e
implementação seja **registrada**, não silenciada. O slice S11 montou o corpus de conformidade
e, ao exercitar o recorte MVP, encontrou quatro lacunas verificáveis. Cada uma abaixo tem um
programa mínimo que a demonstra hoje, o comportamento que canon exige, e a pergunta que precisa
ser fechada antes de implementar. Nenhuma delas invalida o que o corpus já cobre; todas estão
listadas como critérios não satisfeitos no registro de evidência do gate.

## G1 — `init` não é executado na construção `Type{...}`

**Canon exige.** `Type{...}` usa o `init` quando ele existe, e `init` não produz valor
diretamente — a construção produz a nova instância
(`docs/canon/Aipo V1 — Sintaxe Canônica Consolidada…`, linhas 46–49). Defaults de parâmetros
de `init` definem ergonomicamente valores opcionais da construção.

**Comportamento atual.** O hook `init` é parseado, analisado e compilado como função
(`Type.init`), mas a construção nunca o invoca: `BuildStruct` inicializa apenas os campos
nomeados no literal. Demonstração:

```text
struct P
    name
end

impl P
    init(self!, name)
        self.name = name + "!"
    end
end

io.println(P{name = "z"}.name)   // hoje: "z"  · canon: "z!"
io.println(P{}.name)             // hoje: "none" · canon: "anon" com init(self!, name = "anon")
```

**Pergunta.** Como a construção passa a chamar `init` sem perder a construção automática de
canon (linha 49, usada quando a `struct` não declara `init`): `init` vira um alvo de chamada
do próprio `BuildStruct`, ou a construção é reescrita em HIR para uma chamada `Type.init`
seguida de retorno da instância? A resposta precisa dizer também quem avalia os defaults de
parâmetros de `init` — o prologue callee-side já implementado (S11) ou um caminho próprio da
construção.

**Resolução (`P00-G13`).** A construção chama `Type.init` explicitamente. O parser injeta o
receiver implícito `self!` quando o autor não o declara (canon escreve `init(id, ...)` sem
`self`), então todas as etapas seguintes veem a mesma forma de uma função com receiver. O
builder de Core IR emite, nesta ordem: `BuildStruct` (com `defer_fixed`), `MakeFunction
Type.init`, o argumento receiver, os argumentos alinhados à declaração de `init`,
`Call(1 + n)`, `Pop` da cópia de construção e — depois da verificação de `invariant()` —
`SealStruct`. Os **defaults continuam sendo avaliados pelo prologue callee-side já existente**
(cada default é uma expressão no prólogo da própria função, guardado por `JumpIfSetLocal`), o
que preserva a regra de canon de que um default é reavaliado a cada chamada e pode referenciar
parâmetros anteriores. O retorno implícito do `init` passa a ser a própria instância (o hook
"não produz valor diretamente"; a construção produz a nova instância), para que o valor de
`Type{...}` seja a instância publicada. `fixed` deixa de ser imutável durante a construção: o
`StructInstance` carrega `under_construction`, e `set_field` só rejeita campo `fixed` depois de
`SealStruct`. Ver `docs/conformance/programs/13_init_and_invariant.aipo`.

## G2 — `invariant()` não é avaliado em runtime

**Canon exige.** O hook `invariant()` declara condições que devem permanecer verdadeiras; suas
linhas equivalem conceitualmente a condições combinadas por `and`
(`docs/canon/Aipo V1 — Sintaxe Canônica Consolidada…`, linhas 50–56).

**Comportamento atual.** O hook é parseado, type-checked e a instância é publicada sem que
nenhuma condição seja avaliada. Demonstração:

```text
struct Rn
    lo
    hi
end

impl Rn
    init(self!, lo, hi)
        self.lo = lo
        self.hi = hi
    end

    invariant()
        self.lo < self.hi
    end
end

io.println(String(Rn{lo = 2, hi = 1}.hi))   // hoje: 1 · canon: falha de contrato
```

**Pergunta.** Onde o invariante é avaliado: no fim de `init` (uma vez por construção), ou em
toda publicação de instância? E qual é o modelo de erro — `Failure` recuperável ou fault? O
`StructInvariant` hook (`crates/aipo-vm::InvariantValidator`) já existe e aceita uma
closure de validação, mas nenhum caminho o alimenta a partir do bytecode: falta definir como
as condições compiladas viram esse validador.

**Resolução parcial (`P00-G13`) — verificação na construção.** O hook é lowered para uma
função `<Type>.invariant(self) -> Bool` cujas linhas são combinadas por `and` (exatamente o
"conceitualmente `and`" de canon). A construção emite, depois de `init` (ou logo depois de
`BuildStruct`, quando não há `init`), uma chamada ao predicado seguida do novo opcode
`AssertInvariant`, que consome o `Bool` e falha o contrato com `VmFault::InvariantViolation`
quando ele é `false`. O canal é **fault**, decidido pelo próprio recorte MVP de
`docs/waves/wave-1-mvp.md`, que classifica "contract violations at runtime" como runtime
fault, e coerente com o error model já documentado em `docs/stdlib/mvp-subset.md`.
O `BuildStruct` deixa de avaliar o invariante quando a construção é diferida (`defer_fixed`):
o `SealStruct` é que publica a instância, então o invariante observa os campos já atribuídos
por `init`. Ajustar o invariant para os **pontos de mutação posteriores** (canon:
`candidate -> aplicação provisória -> verificação -> commit`) ficou como **G2b** e foi fechado
por `P00-G14` — ver a seção *G2b — `invariant()` nos pontos de mutação* abaixo.

## G3 — Contratos de assinatura não são verificados em runtime

**Canon exige.** Contratos opcionais aparecem em parâmetros e retornos de funções/assinaturas e
são verificados no limite da chamada ("runtime check at boundaries", recorte MVP em
`docs/waves/wave-1-mvp.md`).

**Comportamento atual.** Anotações são parseadas e parte da análise semântica as usa, mas nenhum
opcode de verificação de tipo é emitido: o valor atravessa a fronteira sem checagem.

```text
fn f(x: Int)
    return x
end

io.println(String(f("no")))   // hoje: "no" · canon: erro de contrato no limite
```

**Pergunta.** `TypeIs` já existe como opcode (teste de tipo contra um type value). O contrato
deve reusar `TypeIs` com uma tabela de tipos por parâmetro compilada no prólogo da função
(seguindo o padrão do prologue de defaults de S11), ou exige um opcode dedicado que também
carregue o nome do parâmetro para a mensagem? E `T?` — `none` é aceito e não participa da
checagem?

**Resolução (`P00-G14`).** Opcode dedicado (`AssertContract`), emitido no prólogo do callee para
parâmetros e antes de cada `return expr` para retornos — ver a seção *G3 — contratos de assinatura
em runtime* abaixo, que responde também por que `TypeIs` não foi reusado.

## G4 — `Bytes` não tem forma de construção

**Canon exige.** O recorte MVP exclui "APIs de empacotamento de `Bytes` além de construção e
indexação", o que implica que construção e indexação estão **dentro** do escopo
(`docs/waves/wave-1-mvp.md`).

**Comportamento atual.** `Bytes` existe como type value e como valor (`Value::Bytes`), indexação
e `len` funcionam, mas não há conversão chamável: `Bytes([1, 2, 3])` falha em runtime com
`AIPO_RT_NOT_CALLABLE` (`Bytes (no conversion form in V1)`). Registrado originalmente como Q2 de
`docs/adp/ADP-001-byte-and-core-types-as-values.md`; este ADP o reafirma como critério de gate.

**Pergunta.** Qual é a forma canônica de construir `Bytes` — `Bytes(list_of_ints)`,
`Bytes(string)`, ou ambas? O ADP-001 Q2 permanece aberto e é o que precisa ser fechado primeiro.

**Resolução (`P00-G13`).** Canon responde: a Language Reference §6 mostra `let data = Bytes(32)`
e descreve `Bytes` como "bloco binário mutável e gerenciado". A forma canônica é
**`Bytes(count: Int)`**, que aloca `count` bytes zerados; indexação por byte produz `Byte`
(`Value::Bytes` + `Value::Byte`, ambos já existentes) e `len` reporta a contagem. `Bytes(list)`
e `Bytes(string)` **não** são formas canônicas e permanecem fora. O limite superior de alocação é
uma decisão provisória de implementação (`BYTES_MAX_ALLOCATION`, 64 MiB) para que um tamanho
vindo do código-fonte não esgote a memória; acima do limite o resultado é `Failure` recuperável.
API de packing (`read_i32`/`write_f32`/…) continua fora do recorte MVP.

## Consequência para o gate

Os itens abaixo eram critérios do recorte MVP (`docs/waves/wave-1-mvp.md`) que o corpus não
conseguia certificar; por isso o gate do MVP ficou registrado como **parcial** em
`docs/evidence/P00-G12-conformance-and-mvp-gate.md`. `P00-G13` fechou três deles e `P00-G14`
fechou os dois restantes, e o corpus certifica todos:

| Gap | Estado | Certificação |
|---|---|---|
| G1 — `init` na construção `Type{...}` | **fechado** (`P00-G13`) | `docs/conformance/programs/13_init_and_invariant.aipo` |
| G2 — `invariant()` verificado no fim da construção | **fechado** (`P00-G13`) | `programs/13_init_and_invariant.aipo` + `diagnostics/11_runtime_invariant_violation.aipo` |
| G4 — forma canônica de construção de `Bytes` | **fechado** (`P00-G13`) | `programs/12_bytes.aipo` |
| G2b — `invariant()` nas fronteiras de mutação | **fechado** (`P00-G14`) | `programs/15_invariant_on_mutation.aipo` + `diagnostics/14_runtime_invariant_mutation_uncaught.aipo` |
| G3 — contratos de assinatura em runtime | **fechado** (`P00-G14`) | `programs/14_signature_contracts.aipo` + `diagnostics/12_runtime_contract_violation.aipo`, `13_runtime_return_contract.aipo` |

Com os cinco itens fechados, os critérios do recorte MVP que dependiam destes gaps estão
certificados pelo corpus. O que resta fora do MVP continua sendo o declarado em
`docs/waves/wave-1-mvp.md` (backend JS, APIs de empacotamento de `Bytes`, `Set`, `Sequence`, LSP,
REPL). A conformidade estrutural de interfaces em runtime, antes listada aqui como limite residual,
foi fechada e certificada por `P00-G15`.

## G2b — `invariant()` nos pontos de mutação — fechado por `P00-G14`

**Canon exige.** "Atualização de campo sujeita a invariant segue
`candidate -> aplicação provisória -> verificação -> commit`; em falha recuperável, o valor
anterior é preservado" (`docs/canon/Aipo V1 — Sintaxe Canônica Consolidada…`). O
`Interlúdio — Chamadas, Construção, Impl e Módulos` fecha o modelo no §3: "`invariant()` é
verificado após construção automática, após `init()` concluído com sucesso e ao término
bem-sucedido de uma fronteira que recebeu capacidade mutável sobre a instância"; "A validação
ocorre em estado estável, não após cada assignment interno"; "Quando a validação falha, campos
diretos protegidos da instância retornam ao estado de entrada e a operação produz uma
`Failure`".

**Resolução.** Três decisões, todas retiradas do canon:

1. **Canal de erro — `Failure` recuperável.** O §3 diz literalmente "a operação produz uma
   `Failure`", em contraste com o contrato de assinatura, que a Language Reference §4 classifica
   como *contract fault* não capturável por `attempt`.
2. **Momento — fronteira mutável estável, não cada assignment.** O VM mantém um *journal*: uma
   atribuição a campo protegido de instância **publicada** é provisória — a primeira escrita por
   frame/statement guarda o valor de entrada — e a verificação acontece no fim da fronteira.
   Assim canon continua permitindo estado temporariamente inválido dentro de uma operação.
3. **Como o predicado é alcançado.** O módulo já registra o hook compilado como a função
   `Type.invariant`, então `Vm::run` resolve o `entry_ip` por tipo e invoca o próprio predicado:
   não há segundo avaliador de expressões, e a verificação de mutação e a de construção usam o
   mesmo código compilado.

Fronteiras emitidas pelo builder como `CoreInst::CheckMutations` (opcode `CheckMutations`): o
retorno de cada função/método, incluindo o epílogo implícito, e cada statement de mutação do
script de entrada — que não tem operação envolvente para onde adiar, e cujo `attempt` precisa
capturar a falha do próprio statement.

Quando a validação falha, o VM restaura os campos diretos de **todas** as instâncias
participantes (não só a que falhou, como o §3 exige quando uma operação recebe múltiplas
instâncias mutáveis) e propaga a `Failure` pelo modelo B, de modo que `attempt ... failed` a
captura e o valor de entrada permanece observável. `fixed` continua sendo erro de mutação: o
rollback escreve de volta o valor que o campo já tinha, então a imutabilidade não é afrouxada.

Certificação: `docs/conformance/programs/15_invariant_on_mutation.aipo` (commit, rollback pelo
método, rollback pelo statement direto, rollback de duas instâncias na mesma operação) e
`docs/conformance/diagnostics/14_runtime_invariant_mutation_uncaught.aipo` (a `Failure` não
capturada encerra o programa como qualquer falha recuperável). No nível do VM,
`test_guarded_mutation_rolls_back_at_boundary` (`crates/aipo-vm/tests/data_and_errors.rs`)
exercita a mesma regra com um validador de host.

**Nota sobre o critério original.** O goal `P00-G14` descrevia a verificação "em `SetField`". A
implementação manteve `SetField` como o ponto de mutação guardado (é ele que decide se a
atribuição é provisória e registra o valor de entrada), mas moveu a **verificação** para a
fronteira, porque canon proíbe validar "após cada assignment interno". Verificar na atribuição
rejeitaria programas canônicos que passam por estado transitório dentro de uma operação.

## G3 — contratos de assinatura em runtime — fechado por `P00-G14`

**Canon exige.** `name: Type` restringe um parâmetro, `name!: Type` concede mutação e restringe o
valor, `-> T` restringe o resultado, `T?` significa exatamente `T` ou `none`, e o contrato
simples `Function` verifica apenas que o valor é chamável (`docs/canon/Aipo V1 — Language
Reference…` §4 e §7). A checagem acontece no limite da chamada, e "uma violação descoberta
somente em runtime é **contract fault** de programação e não é capturável por `attempt`" (§4).

**Resolução.** O parser e o HIR já carregavam `TypeAnnotation` (nome + `?`) em parâmetros e
retornos, mas nenhum caminho de runtime a usava. Agora o Core IR emite
`AssertContract { type_name, nullable, position, operations }` e o opcode `AssertContract` carrega
`u16` com o nome do contrato, `u8` com a flag de nulabilidade, `u16` com a posição e a lista de
operações exigidas (`u8` com a contagem, depois `u16`+`u8` por operação; vazia para um contrato de
tipo core ou de `struct`):

- **parâmetros**, no prólogo do próprio callee e depois do prólogo de defaults — assim o valor
  de um default também é verificado, e `init`, métodos, funções e closures ganham a checagem pelo
  mesmo caminho;
- **retornos**, imediatamente antes de cada `return expr`, sobre o valor já empilhado (a
  instrução inspeciona o topo sem consumi-lo, então serve aos dois casos).

Um valor ausente (`Unset`, parâmetro omitido sem default) e um `Failure` (`return fail(...)`)
não são violações: o primeiro é erro de argumento ausente e o segundo encerra o caminho sem
precisar satisfazer o contrato de retorno. `none` só satisfaz um contrato escrito com `?`. Nomes
`Int`/`Float`/`Byte`/`String`/`Bool`/`List`/`Dict`/`Bytes`/`Range` são checados contra `TypeTag`,
`Function` exige um valor chamável, e um nome que o módulo declara como `struct` é comparado com
`StructInstance::type_name`.

**Interface como contrato — conformidade estrutural.** Quando o nome do contrato é uma interface,
o `AssertContract` também carrega as operações que ela declara (`u16` com o nome e `u8` com a
aridade de cada uma), porque canon mantém interfaces **estruturais**: usar uma como contrato
pergunta se o valor expõe as operações declaradas. O arity comparado é o **visível no call site**
— o receiver não é argumento ali, e é assim que as aridades de métodos nativos e de
`BoundMethod` já são declaradas. O valor que não expõe a operação (ou expõe outra aridade) é
*contract fault*, e a mensagem nomeia o tipo declarado da `struct` em vez do kind genérico do
runtime. `none` satisfaz `T?` sem expor nada, que é exatamente o significado de `T?`.

A primeira versão publicada desta checagem continha um defeito real, encontrado e corrigido por
`P00-G15`: o builder contava a aridade da interface **incluindo o receiver** (`fn draw(self)` =
1) enquanto o runtime comparava a aridade visível (0), então o contrato reprovava também os
valores conformes. A convenção foi unificada no builder, que agora emite a aridade visível.

**Pergunta respondida.** `TypeIs` não foi reusado. Ele testa um valor contra um *type value* que
o programa empilha em tempo de execução, enquanto o contrato é uma promessa da assinatura que
precisa viajar no bytecode junto do nome do parâmetro para a mensagem de falha. O reuso relevante
é a tabela de categorias (`TypeTag`), compartilhada pelos dois.

Certificação: `docs/conformance/programs/14_signature_contracts.aipo` (parâmetro, default
verificado, `T?`, `Function`, contrato de `struct`, receptor mutável e retorno) e
`docs/conformance/diagnostics/12_runtime_contract_violation.aipo` /
`13_runtime_return_contract.aipo` (fault não capturável por `attempt`). Conformidade estrutural de
interface: `docs/conformance/programs/16_interface_contracts.aipo` (valor conforme, `satisfy`,
`T?` com `none`, operação com argumento) e `docs/conformance/diagnostics/17` e `18`
(operação ausente e aridade divergente), além dos testes
`test_interface_contract_accepts_a_conforming_operation` e
`test_interface_contract_names_the_failing_struct` em `crates/aipo-vm/tests/data_and_errors.rs`.
