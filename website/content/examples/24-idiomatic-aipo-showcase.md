---
title: "Aipo idiomático, do início ao fim"
description: Exemplo real do repositório Aipo.
---

# Aipo idiomático, do início ao fim

Fonte original: [`examples/24_idiomatic_aipo_showcase.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/examples/24_idiomatic_aipo_showcase.aipo).

## Executar

```bash
cargo run -q -p aipo-cli -- run examples/24_idiomatic_aipo_showcase.aipo
```

## Código completo

```aipo
# A small task board combining structs, higher-order methods, `match` and
# recoverable failures: idiomatic Aipo in one file.
struct Task {
    title
    var done = false
}

Task:finish(var self) {
    self.done = true
}

var board = []
board.add(Task{ title: "parser" })
board.add(Task{ title: "tests" })
board[0].finish()

let open = board.filter(fn (t) { return not t.done })
io.println(len(open))
each task in board {
    match task.done {
    when true
        io.println(f"done: {task.title}")
    else
        io.println(f"todo: {task.title}")
    }
}

fn find(title) {
    each task in board {
        if task.title == title {
            return task
        }
    }
    return fail (f"unknown task: {title}")
}

attempt {
    io.println(find("nope").title)
} failed err {
    io.println("no such task")
}
```

[Saída esperada registrada no repositório](https://github.com/poppyTM/aipo-lang/blob/main/examples/24_idiomatic_aipo_showcase.stdout). Esta página mostra a fonte real, não certifica uma nova execução no HEAD.

[Todos os exemplos](/examples/) · [Manual](/manual/)
