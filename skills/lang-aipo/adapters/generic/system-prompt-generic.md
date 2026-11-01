# Adaptador Canônico Aipo V1 para Provedores Genéricos de LLM
(Compatível com DeepSeek, Qwen 2.5, Gemini, Mistral, Llama 3.3, Ollama, vLLM)

Use este bloco como parte do System Prompt ou Persona em qualquer inferência de modelo de linguagem:

```text
[AIPO_V1_SPECIFICATION]
Language: Aipo (Canonical V1 normative ADR-001)
Philosophy: Minimal cognitive load, explicit contracts, immutable-by-default, one way for each thing.

NEGATIVE SYNTAX RULES (DO NOT GENERATE):
1. Do NOT use '//' for comments. Comments MUST start with '#'. In Aipo, '//' is integer division.
2. Do NOT use 'end'. Blocks open with '{' and close with '}'.
3. Do NOT use 'for'. Iteration uses 'each item in list { }' or 'repeat N as i { }'.
4. Do NOT use 'try', 'catch', 'finally', 'throw', 'raise', 'defer'. Use 'fail expr' and 'attempt { } failed err { }'.
5. Do NOT use 'class' or 'impl'. Structs are data-only. Methods use 'TypeName:method_name(params)'.
6. Do NOT prefix methods with 'fn'. The 'fn' keyword is strictly reserved for free functions.
7. Do NOT mutate 'self' without declaring 'var self' as the first parameter.
8. Do NOT use '&&', '||', '!'. Use 'and', 'or', 'not'.
9. Do NOT use semicolons ';'. Newlines terminate statements.
10. Enums: Supported closed sum type ('enum State { A, B(x: T), C { y: T }, }'). Match over enum without 'else' requires exhaustive coverage.
11. Compiler directives: Supported '#!satisfies', '#!test', '#!deprecated("msg")', '#!todo("msg")' immediately preceding their target item.
12. Batch promotion 'TypeName::[fn1, fn2]' strictly requires 'self' (or 'var self') as the first parameter of promoted functions.
13. Do NOT import prelude modules ('import io', 'import math', 'import task'). 'io', 'math', 'task', 'env', 'log' and 'sh' are already visible; importing them is an unknown-module error.
14. Do NOT write an entry point. There is NO implicit 'fn main': the top level of the module IS the program, and top-level statements run in textual order.
15. Do NOT call 'sys.args()', 'sys.exit()' or 'sys.env()'. No 'sys' module exists: environment access is 'env.get(name)' / 'env.has(name)' behind a capability.
16. Do NOT use 'match' as an expression ('return match x { ... }'). 'match' is a statement: assign the arms to a variable, then return it.
17. 'await do { ... }' is a statement, never an initializer; a 'return' inside it returns from the enclosing function.
18. 'var' is a *parameter* form in a declaration ('fn f(var p: T)'). At a call site the value is passed directly, with no 'var'.

POSITIVE CANONICAL SKELETON:
```text
# Prelude modules ('io', 'math', 'task', 'env', 'log', 'sh') need no import.
# Only user modules are imported, e.g. 'import domain: Tarefa'.
export Entidade, nova_entidade, Status

# Enum (closed sum type)
enum Status {
    Ativo,
    Suspenso(motivo: String),
}

# Struct (data only; field typing is optional in grammar but STRONGLY RECOMMENDED)
struct Entidade {
    id: Int                        # Strong recommendation
    var valor: Float = 0.0
}

# Invariant hook
Entidade:invariant {
    self.id > 0
    self.valor >= 0.0
}

# Immutable method
Entidade:obter_valor() -> Float {
    return self.valor
}

# Mutating method (requires 'var self')
Entidade:adicionar(var self, incremento: Float) {
    if incremento < 0.0 { fail "incremento negativo" }
    self.valor += incremento
}

# Free function (uses 'fn')
fn nova_entidade(id: Int) -> Entidade {
    return Entidade{ id: id }
}

# Batch promotion with '::' (requires 'self' as 1st parameter)
fn calcular_dobro(self) -> Float {
    return self.valor * 2.0
}
Entidade::[calcular_dobro]   # becomes callable as entidade.calcular_dobro()

# Top level IS the program: this runs as written when the module is executed.
let entidade = nova_entidade(1)
io.println(f"valor dobrado: {entidade.calcular_dobro()}")
```
[/AIPO_V1_SPECIFICATION]
```
