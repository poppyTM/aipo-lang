# Aipo — Organização e Arquitetura de Projetos (Aplicações e Bibliotecas)

Este documento define os padrões canônicos para organização de código, diretórios, manifestos `aipo.toml` e estratégias de teste em Aipo V1.

---

## 1. Princípios de Modularidade em Aipo

1. **Um arquivo `.aipo` é um módulo:** Não existe palavra-chave `module` ou `package` dentro do código. A hierarquia de pastas reflete diretamente os caminhos de importação.
2. **Privacidade estrita por padrão:** Qualquer função, struct ou variável de topo é visível apenas dentro daquele arquivo `.aipo`. Apenas itens explicitamente listados em `export` são acessíveis via `import`.
3. **Grafos de importação acíclicos (DAG):** Ciclos de importação (`A` importa `B` e `B` importa `A`) geram erro de compilação.
4. **Resolução sem rede em build/run:** Comandos `aipo run` e `aipo build` nunca acessam a rede. Dependências externas devem estar travadas no manifesto `aipo.toml`.

---

## 2. Estrutura de Projetos: Aplicações

Aplicações são projetos que compilam para um executável binário final ou script executável autônomo com um ponto de entrada `main.aipo`.

### 2.1 Exemplo: Aplicação CLI (`todo-cli`)

#### Árvore de Diretórios
```text
todo-cli/
├── aipo.toml
├── README.md
├── src/
│   ├── main.aipo             # Ponto de entrada (CLI parser e ciclo principal)
│   ├── domain.aipo           # Structs de tarefas, listas e regras de negócio
│   ├── storage.aipo          # Persistência em disco e formato JSON/texto
│   └── format.aipo           # Formatação visual de saída para o terminal
└── tests/
    └── integration_test.aipo # Testes de ponta a ponta do executável
```

#### Manifesto `aipo.toml` da Aplicação
```toml
[package]
name = "apps.todo_cli"
version = "0.1.0"
entry = "src/main.aipo"
authors = ["Desenvolvedor <dev@exemplo.com>"]
description = "Gerenciador de tarefas minimalista em linha de comando"
license = "MIT"
targets = ["native"]

[dependencies]
# Dependências externas exigem SHA de 40 caracteres para reprodutibilidade estrita
# "aipo.terminal" = { git = "https://github.com/aipo-lang/terminal.git", rev = "3a8f9c1b2d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a" }
```

#### Código dos Módulos da Aplicação

**`src/domain.aipo`** (Regras e entidades puras de domínio):
```aipo
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

#!test("criação de tarefa com sucesso")
fn test_criar_tarefa() {
    let t = criar_tarefa(1, "Comprar café")
    if t.concluida {
        fail "tarefa deveria nascer pendente"
    }
}
```

> **Atenção (drift desta página):** os trechos abaixo ilustram *arquitetura*, não a
> superfície real da V1. Três construções que aparecem aqui **não existem** no
> compilador: `io.write_file` / `io.read_file` (o módulo `io` só tem `print` e
> `println`; arquivos passam por `fs` — `fs.read_text` para leitura e `fs.write_text`
> para escrita, ambos assíncronos e sujeitos às capabilities `filesystem.read` e
> `filesystem.write`), `fn main()` como ponto de
> entrada (não existe `main` implícito: o **topo do módulo** é o script) e
> `sys.args()` (não existe módulo `sys`). Os blocos foram ajustados onde havia
> forma canônica equivalente.

**`src/storage.aipo`** (Persistência com journal e transação):
```aipo
import domain: Tarefa
export salvar_tarefas, carregar_tarefas

fn salvar_tarefas(caminho: String, tarefas: List) {
    attempt {
        var buffer = ""
        each t in tarefas {
            let status = if t.concluida then "1" else "0"
            buffer += f"{t.id}|{status}|{t.descricao}\n"
        }
        # Escrita real: `await fs.write_text(caminho, buffer)` (assíncrono, exige
        # capability `filesystem.write`). Escrita *atômica* (tmp+rename) não é
        # garantida: `fs.write_text` substitui o conteúdo do arquivo.
        io.println(f"persistir {len(buffer)} bytes em {caminho}")
    } failed err {
        fail f"falha ao persistir tarefas no disco: {err.message}"
    }
}

fn carregar_tarefas(caminho: String) -> List {
    # Leitura real: `await fs.read_text(caminho)` (assíncrono, exige capability `filesystem.read`).
    var lista = []
    io.println(f"carregar {caminho}")
    # Processa linhas e reconstrói estado
    return lista
}
```

**`src/main.aipo`** (Ponto de entrada):
```aipo
import domain: Tarefa, criar_tarefa
import storage: salvar_tarefas, carregar_tarefas

# Não existe `fn main`: o topo do módulo é o script. Os argumentos chegam como
# parâmetro — a V1 não expõe argumentos de linha de comando.
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

### 2.2 Exemplo: Serviço Backend / API Web (`user-service`)

#### Árvore de Diretórios
```text
user-service/
├── aipo.toml
├── src/
│   ├── main.aipo              # Inicialização do servidor HTTP e injeção de dependências
│   ├── config.aipo            # Leitura de variáveis de ambiente e portas
│   ├── routes/
│   │   ├── auth.aipo          # Endpoints de login e token
│   │   └── users.aipo         # Endpoints CRUD de usuários
│   ├── services/
│   │   └── user_service.aipo  # Lógica de negócio e regras de cadastro
│   └── models/
│       └── user.aipo          # Structs de usuário, senhas hash, invariantes
└── tests/
    └── api_test.aipo          # Testes de integração de endpoints
```

#### Manifesto `aipo.toml` da API
```toml
[package]
name = "services.user_api"
version = "1.0.0"
entry = "src/main.aipo"
description = "Microserviço de autenticação e usuários"
license = "Proprietary"
targets = ["native", "js"]

[dependencies]
"aipo.http" = { path = "../../packages/aipo-http" }
```

---

## 3. Estrutura de Projetos: Bibliotecas, Frameworks e Módulos

Bibliotecas e módulos reutilizáveis não possuem função `main()`. O ponto de entrada padrão é `src/lib.aipo`, atuando como a **fachada pública (façade)** que expõe uma API limpa e oculta os detalhes internos.

### 3.1 Exemplo: Biblioteca de Algoritmos / Utilitários (`aipo-cache`)

#### Árvore de Diretórios
```text
aipo-cache/
├── aipo.toml
├── README.md
├── src/
│   ├── lib.aipo               # Fachada pública: reexporta tipos e funções estáveis
│   ├── memory_cache.aipo      # Implementação concreta de cache LRU em memória
│   ├── contract.aipo          # Interfaces abstratas do cache (CacheStore)
│   └── internal/
│       ├── hasher.aipo        # Utilitário interno privado de hashing
│       └── linked_list.aipo   # Lista duplamente encadeada para controle de LRU
└── tests/
    ├── cache_test.aipo        # Testes de conformidade da API pública
    └── stress_test.aipo       # Testes de carga e política de desalocação
```

#### Manifesto `aipo.toml` da Biblioteca
```toml
[package]
name = "aipo.cache"
version = "0.2.0"
entry = "src/lib.aipo"
authors = ["Equipe de Performance <perf@aipo-lang.org>"]
description = "Cache LRU transacional determinístico de alta performance para Aipo"
license = "MIT"
targets = ["native", "js"]
```

#### Implementação dos Módulos da Biblioteca

**`src/contract.aipo`** (Interfaces contratuais puras):
```aipo
export CacheStore

# Interface estrutural para qualquer mecanismo de armazenamento de cache
interface CacheStore {
    obter(chave: String) -> String?
    inserir(var self, chave: String, valor: String, ttl_ms: Int)
    remover(var self, chave: String) -> Bool
    limpar(var self)
}
```

**`src/memory_cache.aipo`** (Implementação concreta com satisfação estrutural):
```aipo
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
        # Desaloca o item mais antigo (política simplificada)
        self.remover_mais_antigo()
    }
    self.dados[k] = valor
    self.tamanho_atual = len(self.dados)
}

MemoryCache:remover_mais_antigo(var self) {
    # Lógica interna de desalocação
}

MemoryCache:remover(var self, chave: String) -> Bool {
    let k = hash_string(chave)
    if k in self.dados {
        delete self.dados[k]
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

**`src/lib.aipo`** (Fachada da biblioteca):
```aipo
# A fachada importa e reexporta os tipos públicos, blindando o consumidor dos módulos internos
import contract: CacheStore
import memory_cache: MemoryCache, novo_memory_cache

export CacheStore, MemoryCache, novo_memory_cache
```

---

## 4. Estratégias de Teste: Co-locados vs. Pasta `tests/`

Aipo oferece duas formas oficiais para escrever testes:

### 4.1 Testes Co-locados com Diretiva `#!test`
Ideal para testar funções privadas, invariantes locais e casos de borda imediatos.
- Posicionados **no mesmo arquivo** da função que está sendo testada.
- Não poluem o binário de produção: o compilador os inclui apenas sob flag de teste (`aipo test`).
- Exemplo:
  ```aipo
  fn somar(a, b) { return a + b }

  #!test("soma números positivos")
  fn test_soma_positivos() {
      if somar(2, 3) != 5 {
          fail "soma incorreta"
      }
  }
  ```

### 4.2 Testes em Arquivos Separados (`tests/*_test.aipo`)
Ideal para testes de integração, testes end-to-end e testes de API pública da biblioteca.
- Convenção de arquivo: `test_*.aipo` ou `*_test.aipo`.
- Consomem o pacote externamente através de `import`, garantindo que apenas símbolos exportados com `export` sejam testados.
- Exemplo em `tests/cache_test.aipo`:
  ```aipo
  import aipo.cache: novo_memory_cache

  #!test("respeita a capacidade máxima do cache")
  fn test_capacidade_cache() {
      var c = novo_memory_cache(2)
      c.inserir("k1", "v1", 1000)
      c.inserir("k2", "v2", 1000)
      c.inserir("k3", "v3", 1000)
      
      # Verifica que k1 foi desalojado
      if c.obter("k1") != none {
          fail "item k1 deveria ter sido removido por excesso de capacidade"
      }
  }
  ```

---

## 5. Matriz de Decisão: Aplicação vs. Biblioteca

| Aspecto | Aplicação (`app`) | Biblioteca (`lib` / `pkg`) |
|---|---|---|
| Ponto de entrada (`entry`) | `src/main.aipo` (topo do módulo é o script; não há `main` implícito) | `src/lib.aipo` (com re-exports) |
| Visibilidade pública | Raramente usa `export` no `main.aipo` | Uso extensivo de `export` na fachada `lib.aipo` |
| Testes principais | Integração em `tests/*_test.aipo` | Unitários `#!test` co-locados + contrato em `tests/` |
| Dependências | Podem usar caminhos relativos ou pacotes externos | Mínima superfície de dependências externas |
| Alvo padrão | `native` | `["native", "js"]` para portabilidade máxima |
