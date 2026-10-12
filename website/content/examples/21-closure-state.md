---
title: "Closures com estado"
description: Exemplo real do repositório Aipo.
---

# Closures com estado

Fonte original: [`examples/21_closure_state.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/examples/21_closure_state.aipo).

## Executar

```bash
cargo run -q -p aipo-cli -- run examples/21_closure_state.aipo
```

## Código completo

```aipo
# A closure keeps shared state: each counter below owns its own `count`.
fn make_counter() {
    var count = 0

    fn bump() {
        count += 1
        return count
    }

    return bump
}

let first = make_counter()
let second = make_counter()
io.println(first())
io.println(first())
io.println(second())
```

[Saída esperada registrada no repositório](https://github.com/poppyTM/aipo-lang/blob/main/examples/21_closure_state.stdout). Esta página mostra a fonte real, não certifica uma nova execução no HEAD.

[Todos os exemplos](/examples/) · [Manual](/manual/)
