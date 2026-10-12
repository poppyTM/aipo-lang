---
title: "Interfaces estruturais"
description: Exemplo real do repositório Aipo.
---

# Interfaces estruturais

Código extraído de [`examples/14_interfaces_and_satisfy.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/examples/14_interfaces_and_satisfy.aipo), sem adaptar a sintaxe às propostas futuras.

## Executar

```bash
cargo run -q -p aipo-cli -- run examples/14_interfaces_and_satisfy.aipo
```

## Código completo

```aipo
# Interfaces are structural: a value satisfies one by exposing the declared
# operations, and `#!satisfies` states the promise explicitly.
interface Greeter {
    fn greet(self)
    }

    #!satisfies Greeter
    struct Robot {
        name
    }

    Robot:greet(self) {
        return "beep " + self.name
    }

        fn welcome(item: Greeter) {
            return item.greet()
        }

        io.println(welcome(Robot{ name: "r2" }))
```

A [saída esperada](https://github.com/poppyTM/aipo-lang/blob/main/examples/14_interfaces_and_satisfy.stdout) é uma fixture versionada. O site não afirma ter executado este exemplo na revisão atual.

[Todos os exemplos](/examples/) · [Manual](/manual/)
