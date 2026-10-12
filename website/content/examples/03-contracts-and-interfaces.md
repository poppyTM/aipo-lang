---
title: "Contratos e interfaces"
description: Exemplo real do repositório, não um snippet hipotético.
---

# Contratos e interfaces

Este programa foi extraído de [`examples/03_contracts_and_interfaces.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/examples/03_contracts_and_interfaces.aipo). Você pode copiar o código e executá-lo com a CLI.

## Executar

```bash
cargo run -q -p aipo-cli -- run examples/03_contracts_and_interfaces.aipo
```

## Código completo

```aipo
# Contratos de assinatura escritos nas fronteiras: parâmetros (`name: Type`),
# retorno (`-> T`) e opcionalidade (`T?`). O sema reprova o que é provável antes
# de executar; o resto vira contract fault em runtime, não `Failure`.
interface Greeter {
    fn greet(self) - > String
    }

    #!satisfies Greeter
    struct Robot {
        name
    }

    Robot:greet() -> String {
        return "beep " + self.name
    }

        # O parâmetro declara a interface; a conferência é estrutural em runtime.
        fn announce(who: Greeter) - > String{
            return who.greet()
        }

        # `T?` aceita `none` além de um valor conforme.
        fn maybe_announce(who: Greeter?) - > String{
            if who == none {
                return "nobody to greet"
            }
            return announce(who)
        }

        # `invariant()` é verificado na fronteira mutável estável; violar produz `Failure`
        # recuperável com rollback dos campos.
        struct Account {
            owner
            var balance
        }

        Account:invariant {
            self.balance >= 0
        }

        Account:withdraw(var self, amount: Int) {
            self.balance = self.balance - amount
            return self.balance
        }

        let robot = Robot{ name: "r2" }
        io.println(announce(robot))
        io.println(maybe_announce(robot))
        io.println(maybe_announce(none))

        let account = Account{ owner: "ana", balance: 10 }
        account.withdraw(4)
        io.println(f"balance: {account.balance}")

        attempt {
            account.withdraw(100)
        } failed err {
            io.println(f"refused: {err.message}")
        }
        io.println(f"balance after rollback: {account.balance}")
```

A [saída esperada](https://github.com/poppyTM/aipo-lang/blob/main/examples/03_contracts_and_interfaces.stdout) está versionada ao lado do exemplo. Não representa uma nova execução nesta revisão da documentação.

[Todos os exemplos](/examples/) · [Consultar a sintaxe](/manual/)
