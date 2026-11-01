# Aipo — project organization and architecture (applications and libraries)

This document defines the canonical standards for code organization, directories, `aipo.toml`
manifests and testing strategies in Aipo V1.

---

## 1. Modularity principles in Aipo

1. **One `.aipo` file is a module:** there is no `module` or `package` keyword inside the
   code. The folder hierarchy directly reflects import paths.
2. **Strict privacy by default:** any top-level function, struct or variable is visible only
   inside that `.aipo` file. Only items explicitly listed in `export` are reachable through
   `import`.
3. **Acyclic import graphs (DAG):** import cycles (`A` imports `B` and `B` imports `A`)
   produce a compile error.
4. **No network on build/run:** `aipo run` and `aipo build` never access the network.
   External dependencies must be pinned in the `aipo.toml` manifest.

---

## 2. Project structure: applications

Applications are projects that compile to a final binary executable or a standalone
executable script with an entry point `main.aipo`.

### 2.1 Example: CLI application (`todo-cli`)

#### Directory tree

```text
todo-cli/
├── aipo.toml
├── README.md
├── src/
│   ├── main.aipo             # Entry point (CLI parser and main loop)
│   ├── domain.aipo           # Task structs, lists and business rules
│   ├── storage.aipo          # Disk persistence and JSON/text format
│   └── format.aipo           # Visual output formatting for the terminal
└── tests/
    └── integration_test.aipo # End-to-end tests of the executable
```

#### Application manifest `aipo.toml`

```toml
[package]
name = "apps.todo_cli"
version = "0.1.0"
entry = "src/main.aipo"
authors = ["Developer <dev@example.com>"]
description = "Minimalist command-line task manager"
license = "MIT"
targets = ["native"]

[dependencies]
# External dependencies require a 40-character SHA for strict reproducibility
# "aipo.terminal" = { git = "https://github.com/aipo-lang/terminal.git", rev = "3a8f9c1b2d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a" }
```

#### Application module code

**`src/domain.aipo`** (pure domain rules and entities):

```text
export Tarefa, criar_tarefa

struct Tarefa {
    id: Int
    descricao: String
    var concluida: Bool = false
}

Tarefa:invariant {
    self.id > 0
    len(self.descricao) > 0
}

Tarefa:marcar_concluida(var self) {
    self.concluida = true
}

fn criar_tarefa(id: Int, descricao: String) -> Tarefa {
    if len(descricao) == 0 {
        fail "descrição não pode ser vazia"
    }
    return Tarefa{ id: id, descricao: descricao, concluida: false }
}

#!test("task creation succeeds")
fn test_criar_tarefa() {
    let t = criar_tarefa(1, "Comprar café")
    if t.concluida {
        fail "tarefa deveria nascer pendente"
    }
}
```

> **Note (drift in this page):** the snippets below illustrate *architecture*, not the
> real V1 surface. Three constructions shown here **do not exist** in the compiler:
> `io.write_file` / `io.read_file` (the `io` module only has `print` and `println`;
> files go through `fs` — `fs.read_text` for reading and `fs.write_text` for writing,
> both async and subject to the `filesystem.read` and `filesystem.write` capabilities),
> `fn main()` as an entry point (there is no implicit `main`: the **top of the module**
> is the script) and `sys.args()` (there is no `sys` module). The blocks were adjusted
> where a canonical equivalent exists.

**`src/storage.aipo`** (persistence with journal and transaction):

```text
import domain: Tarefa
export salvar_tarefas, carregar_tarefas

fn salvar_tarefas(caminho: String, tarefas: List) {
    attempt {
        var buffer = ""
        each t in tarefas {
            let status = if t.concluida then "1" else "0"
            buffer += f"{t.id}|{status}|{t.descricao}\n"
        }
        # Real write: `await fs.write_text(caminho, buffer)` (async, requires
        # capability `filesystem.write`). *Atomic* write (tmp+rename) is NOT
        # guaranteed: `fs.write_text` replaces the file contents.
        io.println(f"persistir {len(buffer)} bytes em {caminho}")
    } failed err {
        fail f"falha ao persistir tarefas no disco: {err.message}"
    }
}

fn carregar_tarefas(caminho: String) -> List {
    # Real read: `await fs.read_text(caminho)` (async, requires capability `filesystem.read`).
    var lista = []
    io.println(f"carregar {caminho}")
    # Process lines and rebuild state
    return lista
}
```

**`src/main.aipo`** (entry point):

```text
import domain: Tarefa, criar_tarefa
import storage: salvar_tarefas, carregar_tarefas

# There is no `fn main`: the top of the module is the script. Arguments arrive as a
# parameter — V1 does not expose command-line arguments.
fn executar(args: List) {
    let arquivo = "tarefas.txt"
    var lista = carregar_tarefas(arquivo)

    if len(args) < 2 {
        io.println("Uso: todo-cli <add|list|done> [argumentos]")
        return
    }

    let comando = args[1]
    match comando {
        when "add" {
            let desc = args[2] or_else "Sem descrição"
            let nova = criar_tarefa(len(lista) + 1, desc)
            lista.push(nova)
            salvar_tarefas(arquivo, lista)
            io.println(f"Tarefa #{nova.id} adicionada.")
        }
        when "list" {
            each t in lista {
                let check = if t.concluida then "[X]" else "[ ]"
                io.println(f"{t.id}. {check} {t.descricao}")
            }
        }
        else {
            io.println(f"Comando desconhecido: {comando}")
        }
    }
}

executar(["todo-cli", "list"])
```

---

### 2.2 Example: backend / web API service (`user-service`)

#### Directory tree

```text
user-service/
├── aipo.toml
├── src/
│   ├── main.aipo              # HTTP server bootstrap and dependency injection
│   ├── config.aipo            # Environment variables and ports
│   ├── routes/
│   │   ├── auth.aipo          # Login and token endpoints
│   │   └── users.aipo         # User CRUD endpoints
│   ├── services/
│   │   └── user_service.aipo  # Business logic and registration rules
│   └── models/
│       └── user.aipo          # User structs, hashed passwords, invariants
└── tests/
    └── api_test.aipo          # Endpoint integration tests
```

#### API manifest `aipo.toml`

```toml
[package]
name = "services.user_api"
version = "1.0.0"
entry = "src/main.aipo"
description = "Authentication and user microservice"
license = "Proprietary"
targets = ["native", "js"]

[dependencies]
"aipo.http" = { path = "../../packages/aipo-http" }
```

---

## 3. Project structure: libraries, frameworks and modules

Reusable libraries and modules have no `main()` function. The default entry point is
`src/lib.aipo`, acting as the **public façade** that exposes a clean API and hides
internal details.

### 3.1 Example: algorithm / utility library (`aipo-cache`)

#### Directory tree

```text
aipo-cache/
├── aipo.toml
├── README.md
├── src/
│   ├── lib.aipo               # Public façade: re-exports stable types and functions
│   ├── memory_cache.aipo      # Concrete in-memory LRU cache implementation
│   ├── contract.aipo          # Abstract cache interfaces (CacheStore)
│   └── internal/
│       ├── hasher.aipo        # Private internal hashing utility
│       └── linked_list.aipo   # Doubly linked list for LRU control
└── tests/
    ├── cache_test.aipo        # Public API conformance tests
    └── stress_test.aipo       # Load tests and deallocation-policy tests
```

#### Library manifest `aipo.toml`

```toml
[package]
name = "aipo.cache"
version = "0.2.0"
entry = "src/lib.aipo"
authors = ["Performance Team <perf@aipo-lang.org>"]
description = "High-performance deterministic transactional LRU cache for Aipo"
license = "MIT"
targets = ["native", "js"]
```

#### Library module implementation

**`src/contract.aipo`** (pure contractual interfaces):

```text
export CacheStore

# Structural interface for any cache storage mechanism
interface CacheStore {
    obter(chave: String) -> String?
    inserir(var self, chave: String, valor: String, ttl_ms: Int)
    remover(var self, chave: String) -> Bool
    limpar(var self)
}
```

**`src/memory_cache.aipo`** (concrete implementation with structural satisfaction):

```text
import contract: CacheStore
import internal.hasher: hash_string

export MemoryCache, novo_memory_cache

#!satisfies CacheStore
struct MemoryCache {
    capacidade: Int
    var dados: Dict = {}
    var tamanho_atual: Int = 0
}

MemoryCache:invariant {
    self.capacidade > 0
    self.tamanho_atual >= 0
    self.tamanho_atual <= self.capacidade
}

fn novo_memory_cache(capacidade: Int = 1000) -> MemoryCache {
    if capacidade <= 0 {
        fail "capacidade deve ser positiva"
    }
    return MemoryCache{ capacidade: capacidade }
}

MemoryCache:obter(chave: String) -> String? {
    let k = hash_string(chave)
    return self.dados[k]
}

MemoryCache:inserir(var self, chave: String, valor: String, ttl_ms: Int) {
    let k = hash_string(chave)
    if not (k in self.dados) and self.tamanho_atual >= self.capacidade {
        # Deallocate the oldest item (simplified policy)
        self.remover_mais_antigo()
    }
    self.dados[k] = valor
    self.tamanho_atual = len(self.dados)
}

MemoryCache:remover_mais_antigo(var self) {
    # Internal deallocation logic
}

MemoryCache:remover(var self, chave: String) -> Bool {
    let k = hash_string(chave)
    if k in self.dados {
        self.dados.remove(k)
        self.tamanho_atual -= 1
        return true
    }
    return false
}

MemoryCache:limpar(var self) {
    self.dados = {}
    self.tamanho_atual = 0
}
```

**`src/lib.aipo`** (library façade):

```text
# The façade imports and re-exports public types, shielding consumers from internal modules
import contract: CacheStore
import memory_cache: MemoryCache, novo_memory_cache

export CacheStore, MemoryCache, novo_memory_cache
```

---

## 4. Testing strategies: co-located vs. `tests/` folder

Aipo offers two official ways to write tests:

### 4.1 Co-located tests with the `#!test` directive
Ideal for testing private functions, local invariants and immediate edge cases.
- Positioned **in the same file** as the function under test.
- Do not pollute the production binary: the compiler includes them only under the test
  flag (`aipo test`).
- Example:
  ```text
  fn somar(a, b) { return a + b }

  #!test("sums positive numbers")
  fn test_soma_positivos() {
      if somar(2, 3) != 5 {
          fail "soma incorreta"
      }
  }
  ```

### 4.2 Tests in separate files (`tests/*_test.aipo`)
Ideal for integration tests, end-to-end tests and public-API tests of a library.
- File convention: `test_*.aipo` or `*_test.aipo`.
- Consume the package externally through `import`, ensuring only symbols exported with
  `export` are tested.
- Example in `tests/cache_test.aipo`:
  ```text
  import aipo.cache: novo_memory_cache

  #!test("respects the cache's maximum capacity")
  fn test_capacidade_cache() {
      var c = novo_memory_cache(2)
      c.inserir("k1", "v1", 1000)
      c.inserir("k2", "v2", 1000)
      c.inserir("k3", "v3", 1000)

      # Check that k1 was evicted
      if c.obter("k1") != none {
          fail "item k1 deveria ter sido removido por excesso de capacidade"
      }
  }
  ```

---

## 5. Decision matrix: application vs. library

| Aspect | Application (`app`) | Library (`lib` / `pkg`) |
|---|---|---|
| Entry point (`entry`) | `src/main.aipo` (top of the module is the script; no implicit `main`) | `src/lib.aipo` (with re-exports) |
| Public visibility | Rarely uses `export` in `main.aipo` | Extensive use of `export` in the `lib.aipo` façade |
| Main tests | Integration in `tests/*_test.aipo` | Co-located `#!test` unit tests + contract in `tests/` |
| Dependencies | May use relative paths or external packages | Minimal external dependency surface |
| Default target | `native` | `["native", "js"]` for maximum portability |
