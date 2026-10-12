---
title: "Inicialização e invariantes"
description: Exemplo real do repositório Aipo.
---

# Inicialização e invariantes

Código extraído de [`examples/12_struct_init_fixed_invariant.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/examples/12_struct_init_fixed_invariant.aipo), sem adaptar a sintaxe às propostas futuras.

## Executar

```bash
cargo run -q -p aipo-cli -- run examples/12_struct_init_fixed_invariant.aipo
```

## Código completo

```aipo
# Construction runs `init` when declared; fields are immutable by default
# after publication; `invariant()` must hold at the end of construction.
struct Player {
    id
    name
    health
}

Player:init(id, name, health = 100) {
    self.id = id
    self.name = name
    self.health = health
}

Player:invariant {
    self.name != ""
    self.health >= 0
    self.health <= 100
}

let ana = Player{ id: 1, name: "ana" }
io.println(ana.id)
io.println(ana.name)
io.println(ana.health)
```

A [saída esperada](https://github.com/poppyTM/aipo-lang/blob/main/examples/12_struct_init_fixed_invariant.stdout) é uma fixture versionada. O site não afirma ter executado este exemplo na revisão atual.

[Todos os exemplos](/examples/) · [Manual](/manual/)
