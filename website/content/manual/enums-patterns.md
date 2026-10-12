---
title: "Enums, match e padrões"
description: Manual atualizado com fontes da linguagem Aipo.
---

# Enums, match e padrões

Use `enum` para representar **um conjunto fechado de alternativas**. A seleção `match` escolhe o comportamento adequado para cada variante.

## Variantes simples e com dados

```aipo
enum Forma {
    Ponto,
    Circulo { raio: Int },
    Retangulo(largura: Int, altura: Int),
}

let f1 = Forma.Ponto
let f2 = Forma.Circulo{ raio: 10 }
let f3 = Forma.Retangulo(4, 5)
```

Essas construções vêm do programa de conformidade [`32_enum_variants_and_matching.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/docs/conformance/programs/32_enum_variants_and_matching.aipo), incluído no repositório como fixture.

## Selecionar pela variante

```aipo
fn descrever(f) {
    match f {
        when Forma.Ponto {
            io.println("ponto")
        }
        when Forma.Circulo { raio } if raio > 5 {
            io.println("circulo grande " + String(raio))
        }
        when Forma.Circulo { raio } {
            io.println("circulo pequeno " + String(raio))
        }
        when Forma.Retangulo(largura, altura) {
            io.println("retangulo " + String(largura * altura))
        }
    }
}
```

O `when` pode reconhecer valores, variantes e estruturas, e `if` pode acrescentar uma guarda. Uma guarda falsa continua para o próximo braço, não pula diretamente para o `else`. Para desconstrução de campos sem enum, confira [`31_match_destructure_and_guards.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/docs/conformance/programs/31_match_destructure_and_guards.aipo).

**Pratique:** acrescente outra variante com dados e um braço de `match` apropriado.

[Voltar ao manual](/manual/) · [Fluxo de controle](/manual/control-flow).
