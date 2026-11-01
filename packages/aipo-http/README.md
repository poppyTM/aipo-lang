# aipo.http

Pacote oficial da linguagem **Aipo** para construção de serviços HTTP, microsserviços e APIs web modernas. Projetado com roteador de árvore de segmentos sem expressões regulares (*zero-regex Radix/Segment Trie*), pipeline de middlewares estilo cebola (*onion-style pipeline*), context engine unificado e tipado, e suporte nativo a SSR e dispatchers desacoplados.

## Instalação

Adicione ao seu `aipo.toml`:

```toml
[dependencies]
"aipo.http" = { path = "packages/aipo-http" }
```

## Características

- **Roteamento Zero-Regex:** Segmentação determinística de caminhos com suporte a rotas estáticas (`/api`), parâmetros dinâmicos (`/users/:id`), múltiplos parâmetros aninhados e coringas (*catch-all* `*filepath`).
- **Middlewares Estilo Cebola (*Onion-Style*):** Execução sequencial pré/pós-handler com encadeamento de chamadas `next()`, permitindo transformações de resposta, auditoria, injeção de cabeçalhos e curto-circuito seguro (e.g. autenticação).
- **Middlewares Embutidos:**
  - `cors`: Gerenciamento completo de origens cruzadas e preflight OPTIONS automático com status 204.
  - `logger`: Auditoria contextual de método, caminho e status de resposta.
  - `recover`: Captura de falhas não tratadas na execução do handler, retornando 500 JSON sem derrubar o runtime.
- **Context Engine Unificado (`Context`):** API concisa para leitura de parâmetros (`c.param("id")`), query strings (`c.query_param("q")`), cabeçalhos (`c.header("Auth")`), parsing de corpo JSON (`c.body_json()`) e serialização de respostas (`c.json(...)`, `c.text(...)`, `c.html(...)`, `c.status(...)`).
- **Agrupamento de Rotas (*Route Groups*):** Crie submódulos com prefixos comuns (`app.group("/api/v1")`) e middlewares específicos por grupo.
- **Desacoplamento e Testabilidade Total:** Todo request pode ser processado e testado de forma pura via `app.handle_request(req_dict)`, facilitando testes unitários rápidos e integração perfeita com qualquer host (Bun/Node via backend JS, Hyper/Sockets via Host Nativo).

## Exemplo Rápido

```text
import aipo.http as http

let app = http.create()

# Middlewares globais
app.use(http.logger())
app.use(http.cors())
app.use(http.recover())

# Rota básica
app.get("/", fn (c) {
    c.text("API Aipo ativa!")
})

# Grupo de rotas com parâmetros
let v1 = app.group("/api/v1")

v1.get("/users/:id", fn (c) {
    let id = c.param("id")
    c.json({
        "id": id,
        "name": "Desenvolvedor Aipo",
        "active": true
    })
})

v1.post("/users", fn (c) {
    let body = c.body_json()
    c.status(201)
    c.json({
        "created": true,
        "user": body
    })
})
```
