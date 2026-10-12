---
title: "Interfaces, contratos e tipos opcionais"
description: "Referência de uso organizada por tarefas e comportamento do Aipo."
---

# Interfaces, contratos e tipos opcionais

As interfaces do Aipo descrevem **quais operações** um valor precisa oferecer, em vez de impor herança entre classes. Na implementação atual, `interface` e a diretiva `#!satisfies` podem participar de verificações estáticas e de runtime.

## Contrato estrutural

```aipo
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

O exemplo base é [interfaces e satisfy](/examples/14-interfaces-and-satisfy). O contrato deve ser satisfeito pelas operações oferecidas pelo valor, não pelo nome da classe.

## Contratos opcionais

`name: Type` declara o tipo esperado de um parâmetro; `-> Type` declara o tipo de retorno. `Type?` aceita também `none`.

**Importante:** um contrato de assinatura e uma `Failure` recuperável não são a mesma coisa. Uma violação estática pode ser rejeitada antes da execução; uma incompatibilidade não demonstrável estaticamente pode virar *contract fault*.

**Pratique:** adicione outro tipo com método `greet` e chame `welcome` sem alterar a função.

[Exemplo completo](/examples/14-interfaces-and-satisfy) · [Contrato e limites](/reference/status).
