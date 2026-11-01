# Aipo — Clean code, cognitive ergonomics and design patterns

Aipo is designed under the principle of **low cognitive overhead** and neurodivergent accessibility (W3C COGA). In Aipo, "clean code" does not mean creating unnecessary layers of abstraction (strict YAGNI), but linear, predictable code with explicit contracts and zero ambiguity.

---

## 1. Fundamental design principles

### 1.1 Linearity and noiseless reading
- Avoid excessive block nesting (maximum 3 levels). Prefer guard clauses (early return / early fail).
- Eliminate "clever code" or syntactic tricks. Immediate clarity beats cryptic brevity.
- One clear responsibility per module (`1 .aipo file = 1 cohesive domain unit`).

### 1.2 Immutability by default
In Aipo, every `let` declaration and every struct field not annotated with `var` is strictly immutable.
- **Prefer functional transformations:** use the `with` operator to produce new states from existing structs.
- **Mutability must be surgical and visible:** declare `var` in local variables and struct fields only when in-place mutation is strictly necessary for performance or semantics.
- **Method receivers:** most methods should be read-only (implicit `self`). Only use `Tipo:metodo(var self)` when in-place mutation is the clear design intent.

### 1.3 Explicit contracts and invariants
- Essential business rules belong in `Tipo:invariant { ... }`.
- If a bank account can never have a negative balance, do not rely only on manual checks in every method: declare the rule in the `invariant` hook.
- Aipo's runtime validates the hook after initialization (`init`) and before/after mutable methods. On violation inside an `attempt` block, mutations are rolled back automatically.

---

## 2. Naming and typing conventions

| Element | Convention | Examples | Rationale |
|---|---|---|---|
| Structs | `PascalCase` | `Usuario`, `ItemPedido`, `ResultadoHttp` | Immediate differentiation from ordinary values. |
| Interfaces | `PascalCase` | `Serializavel`, `Autenticador`, `Repositorio` | Indicates structural contracts. |
| Free functions | `snake_case` | `calcular_total`, `buscar_por_id` | Fluid reading without visual friction. |
| Methods | `snake_case` | `adicionar_item`, `esta_ativo`, `serializar` | Consistency with free functions. |
| Variables and fields | `snake_case` | `limite_maximo`, `taxa_juros`, `var contador` | Clarity and readability. |
| Directives | `#!nome` | `#!satisfies`, `#!test`, `#!deprecated`, `#!todo` | Metadata read by the compiler and analyzer. |

### 2.1 Type annotation and data contracts
- **Struct fields:** type annotation in structs (`campo: Tipo`) is formally **optional**, but **EXTREMELY RECOMMENDED**. Declaring struct field types shields the domain against type corruption at runtime, improves compiler diagnostics and lets an LLM understand the data model with surgical precision.
- **Public functions and interfaces:** explicit typing is strongly recommended in parameters and return types of:
  - Exported public functions (`export`).
  - Interface signatures (`interface`).
  - Domain method signatures.

### 2.2 Code evolution with directives
In Aipo clean code, code evolution and technical debt should not be left as loose comments (`# TODO: fix`). Use directives tracked by the compiler:
- **`#!deprecated("use novo_metodo()")`**: precisely signals to consumers that the API will be discontinued, emitting controlled warnings instead of silent breaking changes.
- **`#!todo("implement LRU cache")`**: annotates the item without polluting execution logic, letting the CLI and agents audit structured pending work.

```text
# High-quality contractual model: typed fields, immutable by default
struct Pedido {
    id: Int
    cliente_id: Int
    total: Float
    var status: String = "pendente"
}
```

```text
# Recommended at public boundaries:
export calcular_frete

fn calcular_frete(peso_kg: Float, distancia_km: Float) -> Float {
    if peso_kg <= 0.0 or distancia_km <= 0.0 {
        fail "peso e distância devem ser positivos"
    }
    return (peso_kg * 1.5) + (distancia_km * 0.08)
}
```

---

## 3. Modeling and architecture patterns

### 3.1 Entity modeling: data vs. behavior
In Aipo, a struct defines **only the data format**. Behaviors, validations and constructors are declared through hooks and associated methods right below the struct.

```text
# 1. Pure data structure
struct Carrinho {
    id: Int
    usuario_id: Int
    var itens: List = []
    var total: Float = 0.0
}

# 2. Custom initializer
Carrinho:init(id: Int, usuario_id: Int) {
    self.id = id
    self.usuario_id = usuario_id
    self.itens = []
    self.total = 0.0
}

# 3. Integrity invariant
Carrinho:invariant {
    self.id > 0
    self.usuario_id > 0
    self.total >= 0.0
}

# 4. Immutable (read) method
Carrinho:quantidade_itens() -> Int {
    return len(self.itens)
}

# 5. Mutable method (in-place state change)
Carrinho:adicionar(var self, item_nome: String, preco: Float) {
    if preco <= 0.0 {
        fail "preço do item deve ser maior que zero"
    }
    self.itens.push({ "nome": item_nome, "preco": preco })
    self.total += preco
}
```

### 3.2 Functional update with `with`
To avoid unnecessary mutability, use the functional update `with`:

```text
struct Configuracao {
    host: String = "localhost"
    porta: Int = 8080
    ssl: Bool = false
}

let padrao = Configuracao{}
# Produces a new instance with port and SSL changed; 'padrao' stays untouched
let prod = padrao with { porta: 443, ssl: true }
```

### 3.3 Functional reuse and promotion with `::`
One of the clean-code principles in Aipo is the separation between pure logic and ergonomic convenience.
Instead of encapsulating complex algorithms inside coupled methods, write functions that declare `self` as the first parameter and promote them to methods in batch using `::`:

```text
# Pure functions declaring 'self' as the first parameter:
fn calcular_perimetro(self) -> Float {
    return 2.0 * (self.largura + self.altura)
}

fn calcular_area(self) -> Float {
    return self.largura * self.altura
}

# Clean promotion: makes both accessible as methods of Retangulo
Retangulo::[calcular_perimetro, calcular_area]
```
This pattern eliminates code duplication and preserves architectural clarity.

### 3.4 State and variant modeling with `enum`
Avoid modeling finite states or alternatives with magic strings (`status: "ativo" | "inativo"`). In Aipo V1, use `enum`:
- **Exhaustiveness guarantee:** the compiler prevents forgetting cases in `match`.
- **Structured payloads:** use variants with named or positional payloads only where each case carries specific data.
- **Associated behavior:** enums support associated methods (`Estado:metodo()`) and hooks (`Estado:invariant`).

```text
enum ResultadoOperacao {
    Sucesso { dados: String },
    Pendente,
    Falha(codigo: Int),
}
```

### 3.5 Error and transactional failure handling
Aipo forbids `try / catch / throw` and `defer`. Error handling uses a small vocabulary with transactional semantics:

1. **Direct failure:** use `fail "mensagem"` to signal recoverable business-logic failures.
2. **Immediate fallback:** use `or_else` for safe alternative values.
   ```text
   let timeout = obter_parametro("timeout") or_else 3000
   ```
3. **Propagation with `?`:** like modern languages, propagates `Failure` to the upper layer immediately:
   ```text
   let usuario = buscar_usuario(id)?
   let saldo = consultar_saldo(usuario.conta_id)?
   ```
4. **Transactional boundary with `attempt / failed`:**
   Wrap atomic side-effect blocks in `attempt`:
   ```text
   attempt {
       estoque.reservar(produto_id, qtd)
       pagamento.processar(cliente_id, valor)
       notificacao.enviar_confirmacao(cliente_id)
   } failed erro {
       # If payment fails, stock reservations are undone by the journal
       log.error(f"Falha ao concluir pedido: {erro.message}")
       return fail(f"Não foi possível processar o pedido: {erro.message}")
   }
   ```

---

## 4. What to avoid (clean-code anti-patterns in Aipo)

1. **Avoid simulating classes or inheritance:** Aipo has no `class` and no inheritance. Use struct composition and structural interfaces.
2. **Avoid trying to guess utility methods on primitive numbers:** `3.somar(4)` is an error. Call `somar(3, 4)` or define explicitly.
3. **Avoid obvious explanatory comments:**
   ```text
   # BAD: comment that echoes the code
   # increments the counter
   contador += 1

   # GOOD: comment that explains the business motivation
   # Needed to respect the external API's hourly quota
   taxa_requisições += 1
   ```
4. **Avoid giant functions:** if a function has more than 40 lines, extract local functions (`fn local() { ... }`) or auxiliary free functions in the module.
5. **Avoid unnecessary `var`:** if a variable does not need reassignment, always use `let`.
