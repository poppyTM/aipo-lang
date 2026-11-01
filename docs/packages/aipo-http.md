# aipo.http — Framework HTTP e Roteador de Microsserviços

`aipo.http` é o framework oficial da linguagem Aipo para construção de APIs REST, microsserviços e aplicações web backend de alta performance.

O framework é desenhado seguindo a filosofia minimalista e determinística do Aipo:
1. **Roteador Zero-Regex em Árvore de Segmentos (*Segment/Radix Trie*):** busca estática, parâmetros dinâmicos (`:param`) e coringas (`*wildcard`) sem compilar ou executar expressões regulares em runtime.
2. **Pipeline de Middlewares Estilo Cebola (*Onion-Style*):** execução de middlewares antes e depois dos handlers com encadeamento de `next()`, permitindo transformações de resposta e controle de fluxo.
3. **Middlewares Canônicos Embutidos:**
   - `cors`: suporte completo a Cross-Origin Resource Sharing com preflight OPTIONS (204).
   - `logger`: auditoria estruturada de requisições e respostas.
   - `recover`: interceptação segura de erros e exceções não tratadas retornando status 500 JSON.
4. **Context Engine Tipado (`Context`):** métodos ergonômicos para leitura de query strings, parâmetros de rota, cabeçalhos, corpo JSON e respostas imediatas (`json`, `text`, `html`, `status`).
5. **Agrupamento de Rotas (*Route Groups*):** prefixos aninhados (`/api/v1`) com middlewares específicos por grupo.
6. **Desacoplamento e Testabilidade Pura:** o dispatcher `app.handle_request(req_dict)` aceita dicionários e devolve dicionários sem acoplamento a sockets do sistema operacional, tornando testes unitários ultrarrápidos e facilitando adaptadores para qualquer runtime (Node/Bun via backend JS ou Sockets nativos via Host).

---

## 1. Instalação e Configuração

No arquivo `aipo.toml` do seu projeto:

```toml
[dependencies]
"aipo.http" = { path = "packages/aipo-http" }
```

---

## 2. Inicialização e Roteamento Básico

```text
import aipo.http as http

let app = http.create()

# Resposta em texto puro
app.get("/", fn (c) {
    c.text("Bem-vindo à API Aipo!")
})

# Resposta em JSON
app.get("/api/health", fn (c) {
    c.json({
        "status": "healthy",
        "uptime": 3600
    })
})

# Resposta em HTML
app.get("/welcome", fn (c) {
    c.html("<h1>Portal Aipo HTTP</h1>")
})
```

---

## 3. Parâmetros de Rota e Coringas

O roteador suporta parâmetros dinâmicos iniciados por `:` e coringas iniciados por `*`:

```text
# Parâmetro simples
app.get("/users/:id", fn (c) {
    let user_id = c.param("id")
    c.json({ "id": user_id, "name": "Desenvolvedor" })
})

# Parâmetros múltiplos aninhados
app.get("/orgs/:org/repos/:repo", fn (c) {
    let org = c.param("org")
    let repo = c.param("repo")
    c.text(f"Repositório: {org}/{repo}")
})

# Rota coringa (catch-all) para arquivos estáticos
app.get("/static/*filepath", fn (c) {
    let file = c.param("filepath")
    c.text(f"Servindo arquivo: {file}")
})
```

---

## 4. Query Strings e Corpo da Requisição (JSON)

```text
# Leitura de query parameters (?q=busca&page=2)
app.get("/search", fn (c) {
    let query = c.query_param("q")
    let page = c.query_param("page")
    c.json({ "query": query, "page": page })
})

# Criação de recursos com corpo JSON
app.post("/users", fn (c) {
    let payload = c.body_json()
    c.status(201)
    c.set_header("x-created-by", "aipo-http")
    c.json({
        "created": true,
        "username": payload["username"]
    })
})
```

---

## 5. Pipeline de Middlewares Estilo Cebola

Middlewares são funções `fn (ctx, next)` que podem inspecionar ou alterar a requisição antes e depois da execução:

```text
let app = http.create()

# Middleware de auditoria de tempo e cabeçalho
app.use(fn (c, next) {
    c.set_header("x-server", "aipo-engine")
    let res = next()
    # Executado no retorno (fase de saída da cebola)
    return res
})

# Middleware de proteção com curto-circuito (Auth)
app.use(fn (c, next) {
    let token = c.header("Authorization")
    if token != "Bearer meu-token-secreto" {
        c.status(401)
        c.json({ "error": "Não autorizado" })
        return none # Interrompe a cadeia imediatamente
    }
    return next()
})
```

### Middlewares Embutidos

```text
# CORS configurável com suporte a preflight OPTIONS
app.use(http.cors({
    "origin": "https://meudominio.com",
    "methods": "GET, POST, PUT, DELETE"
}))

# Logger padrão
app.use(http.logger())

# Recuperação de pânico/falhas com status 500 JSON
app.use(http.recover())
```

---

## 6. Agrupamento de Rotas (`group`)

Organize rotas em submódulos com prefixos comuns e regras dedicadas:

```text
let api_v1 = app.group("/api/v1")

# Rota registrada como /api/v1/status
api_v1.get("/status", fn (c) {
    c.json({ "version": "1.0.0" })
})

# Rota registrada como /api/v1/users
api_v1.get("/users", fn (c) {
    c.json([])
})
```

---

## 7. Testabilidade Pura sem Portas ou Rede

Qualquer aplicação pode ser testada em milissegundos sem abrir portas locais:

```text
let app = http.create()

app.get("/ping", fn (c) {
    c.text("pong")
})

let response = app.handle_request({
    "method": "GET",
    "path": "/ping"
})

expect.equal(response["status"], 200)
expect.equal(response["body"], "pong")
```
