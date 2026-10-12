---
title: "Mutação e rollback"
description: Exemplo real do repositório Aipo.
---

# Mutação e rollback

Código extraído de [`examples/13_mutation_and_rollback.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/examples/13_mutation_and_rollback.aipo), sem adaptar a sintaxe às propostas futuras.

## Executar

```bash
cargo run -q -p aipo-cli -- run examples/13_mutation_and_rollback.aipo
```

## Código completo

```aipo
# Mutating a published instance re-checks `invariant()` at the stable
# boundary; on failure the entry values return and the operation yields a
# recoverable `Failure` instead of publishing a forbidden state.
struct Span {
    var lo
    var hi
}

Span:init(var self, lo, hi) {
    self.lo = lo
    self.hi = hi
}

Span:invariant {
    self.lo < self.hi
}

Span:widen(var self, amount) {
    self.lo = self.lo - amount
}

let r = Span{ lo: 2, hi: 8 }
r.widen(1)
io.println(r.lo)

attempt {
    r.widen(-10)
} failed err {
    io.println(f"rolled back: {err.message}")
}
io.println(r.lo)
io.println(r.hi)
```

A [saída esperada](https://github.com/poppyTM/aipo-lang/blob/main/examples/13_mutation_and_rollback.stdout) é uma fixture versionada. O site não afirma ter executado este exemplo na revisão atual.

[Todos os exemplos](/examples/) · [Manual](/manual/)
