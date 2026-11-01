# Funções, Closures & Lambdas

As funções em Aipo são cidadãs de primeira classe (*first-class citizens*). Elas podem ser passadas como argumentos, atribuídas a variáveis, retornadas por outras funções e capturar variáveis do ambiente léxico.

---

## Declaração Básica de Funções

As funções são introduzidas com a palavra-chave `fn` e delimitadas por chaves `{ ... }`:

```aipo
fn somar(a, b) {
    return a + b
}

let total = somar(10, 20)
io.println(total) # 30
```

Se o `return` for omitido ou não especificar um valor, a função retorna `none`.

---

## Parâmetros Opcionais & Argumentos Nomeados

O Aipo permite especificar valores padrão para parâmetros opcionais:

```aipo
fn conectar(host, porta = 8080, timeout = 5000) {
    io.println(f"Conectando a {host}:{porta} com timeout {timeout}ms")
}

conectar("localhost")           # usa porta 8080 e timeout 5000
conectar("db.interno", 5432)    # usa porta 5432 e timeout 5000
```

Também é possível chamar funções passando argumentos nomeados para maior legibilidade e flexibilidade:

```aipo
fn conectar(host, porta = 8080, timeout = 5000) {
    io.println(f"Conectando a {host}:{porta} com timeout {timeout}ms")
}

conectar("api.servico", timeout = 1000)
```

---

## Funções Anônimas & Closures

Funções anônimas (`fn(params) { ... }`) podem ser atribuídas a variáveis e preservam o escopo de variáveis capturadas do ambiente léxico externo:

```aipo
fn criar_contador(inicial = 0) {
    var count = inicial
    return fn() {
        count += 1
        return count
    }
}

let c = criar_contador(10)
io.println(c()) # 11
io.println(c()) # 12
```

A VM do Aipo implementa upvalues seguros e compartilhados, garantindo que mutações na variável capturada reflitam corretamente entre múltiplos closures.

---

## Lambdas Concisas (`=>`)

Para funções curtas de expressão única (especialmente úteis em operações de coleção como `map` e `filter`), o Aipo oferece a sintaxe de flecha:

```aipo
let numeros = [1, 2, 3, 4, 5]

# Dobrando valores com lambda de parâmetro único
let dobrados = numeros.map(x => x * 2)
io.println(dobrados) # [2, 4, 6, 8, 10]

# Filtrando pares
let pares = numeros.filter(x => x % 2 == 0)
io.println(pares) # [2, 4]
```

Lambdas de múltiplos parâmetros utilizam parênteses: `(a, b) => a + b`.

---

## Funções Locais com Auto-Recursão

O Aipo suporta a declaração de funções locais dentro do corpo de outras funções ou métodos, com resolução completa de auto-recursão via `FillSelfCapture`:

```aipo
fn fatorial(n) {
    fn loop_rec(atual, acumulador) {
        if atual <= 1 {
            return acumulador
        }
        return loop_rec(atual - 1, acumulador * atual)
    }

    return loop_rec(n, 1)
}

io.println(fatorial(5)) # 120
```
