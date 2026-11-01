# aipo.http — HTTP Microservices Framework & Router

`aipo.http` is the official backend framework for the Aipo programming language, designed for high-performance REST APIs, microservices, and web servers.

It is built following Aipo's core minimalist and deterministic philosophy:
1. **Zero-Regex Segment/Radix Router:** Static paths, dynamic parameters (`:param`), and wildcards (`*wildcard`) without runtime regular expression parsing.
2. **Onion-Style Middleware Pipeline:** Pre- and post-processing with chained `next()` calls, enabling response transformations, audit trails, and short-circuit controls.
3. **Built-in Canonical Middlewares:**
   - `cors`: Cross-Origin Resource Sharing with automatic preflight OPTIONS (204).
   - `logger`: Structured logging for request methods, paths, and status codes.
   - `recover`: Traps unhandled runtime faults, returning 500 JSON without crashing the runtime.
4. **Typed Context Engine (`Context`):** High-ergonomics access to query strings, route parameters, headers, JSON body decoding, and immediate response builders (`json`, `text`, `html`, `status`).
5. **Route Grouping (`group`):** Nested prefixes (`/api/v1`) with group-scoped middlewares.
6. **Pure Decoupled Testing:** Handlers operate via `app.handle_request(req_dict)`, allowing deterministic unit tests without binding OS sockets.

---

## 1. Installation

In your project's `aipo.toml`:

```toml
[dependencies]
"aipo.http" = { path = "packages/aipo-http" }
```

---

## 2. Basic Setup and Routing

```text
import aipo.http as http

let app = http.create()

# Plain text response
app.get("/", fn (c) {
    c.text("Welcome to Aipo HTTP!")
})

# JSON response
app.get("/api/health", fn (c) {
    c.json({
        "status": "healthy",
        "uptime": 3600
    })
})

# HTML response
app.get("/welcome", fn (c) {
    c.html("<h1>Aipo HTTP Portal</h1>")
})
```

---

## 3. Path Parameters and Wildcards

```text
# Single parameter
app.get("/users/:id", fn (c) {
    let user_id = c.param("id")
    c.json({ "id": user_id, "name": "Developer" })
})

# Nested parameters
app.get("/orgs/:org/repos/:repo", fn (c) {
    let org = c.param("org")
    let repo = c.param("repo")
    c.text(f"Repository: {org}/{repo}")
})

# Wildcard catch-all for static assets
app.get("/static/*filepath", fn (c) {
    let file = c.param("filepath")
    c.text(f"Serving file: {file}")
})
```

---

## 4. Query Strings & JSON Body Parsing

```text
# Query parameters (?q=search&page=2)
app.get("/search", fn (c) {
    let query = c.query_param("q")
    let page = c.query_param("page")
    c.json({ "query": query, "page": page })
})

# POST request with JSON payload
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

## 5. Onion Middleware Pipeline

Middlewares follow the `fn (ctx, next)` signature:

```text
let app = http.create()

# Audit & custom header middleware
app.use(fn (c, next) {
    c.set_header("x-server", "aipo-engine")
    let res = next()
    return res
})

# Auth guard with short-circuit
app.use(fn (c, next) {
    let token = c.header("Authorization")
    if token != "Bearer secret-token" {
        c.status(401)
        c.json({ "error": "Unauthorized" })
        return none # Halts pipeline execution
    }
    return next()
})
```

### Built-in Middlewares

```text
# CORS configuration
app.use(http.cors({
    "origin": "https://example.com",
    "methods": "GET, POST, PUT, DELETE"
}))

# Request logger
app.use(http.logger())

# Panic / runtime fault recovery
app.use(http.recover())
```

---

## 6. Route Grouping (`group`)

```text
let api_v1 = app.group("/api/v1")

# Registered as /api/v1/status
api_v1.get("/status", fn (c) {
    c.json({ "version": "1.0.0" })
})

# Registered as /api/v1/users
api_v1.get("/users", fn (c) {
    c.json([])
})
```

---

## 7. Zero-Network Testability

Execute tests in memory with pure dictionaries:

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
