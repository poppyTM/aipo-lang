---
title: "Structs, campos e métodos"
description: Manual prático baseado nos exemplos da implementação atual.
---

# Structs, campos e métodos

`struct` reúne dados relacionados. Métodos são operações de um tipo; o código atual usa `Tipo:metodo`. Campos são imutáveis por padrão e podem ser declarados como `var` quando a mudança faz parte do modelo.

## Dados e comportamento

```aipo
struct Task {
    title
    var done = false
}

Task:finish(var self) {
    self.done = true
}

let task = Task{ title: "parser" }
task.finish()
io.println(task.done)
```

Recortado do [exemplo idiomático](/examples/24-idiomatic-aipo-showcase).

## Inicialização e integridade

```aipo
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
```

Fonte: [exemplo de init/invariant](/examples/12-struct-init-fixed-invariant).

**Não confundir com a proposta histórica da sintaxe:** exemplos antigos podem apresentar `impl` ou `self!`, enquanto os programas do repositório acima usam `Tipo:metodo` e `var self`. As duas superfícies não devem ser anunciadas como equivalentes sem testes.

**Pratique:** acrescente um campo `var score = 0` e um método que o incremente.

[Próximo: interfaces](/manual/interfaces) · [Falhas e rollback](/manual/failures).
