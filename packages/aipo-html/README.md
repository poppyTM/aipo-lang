# aipo.html

Pacote oficial da linguagem **Aipo** para construção de interfaces web declarativas com HTML5, CSS-in-Aipo tipado e reatividade **MVU de Granularidade Fina** (*Fine-Grained Model-View-Update*).

## Instalação

Adicione ao seu `aipo.toml`:

```toml
[dependencies]
"aipo.html" = { path = "packages/aipo-html" }
```

## Características

- **Sintaxe Limpa e Sem Ruído:** Esqueça `children: [...]` e tags de fechamento. O Aipo usa *trailing blocks* `{ ... }` e funções de tag naturais (`div`, `h1`, `p`, `button`).
- **MVU de Granularidade Fina:** Atualizações cirúrgicas no DOM com previsibilidade matemática e zero diffing de árvore inteira.
- **CSS-in-Aipo Tipado:** Crie estilos reutilizáveis, com suporte a pseudo-classes (`hover`, `active`) e classes automáticas com hash.
- **Interoperabilidade Total:** Compatível com Tailwind CSS, classes utilitárias e eventos padrão do navegador.

## Exemplo Rápido (Contador)

```text
import aipo.html as h

struct Model {
    count: Int
}

enum Msg {
    Increment,
    Decrement,
    Reset
}

fn update(m: Model, msg: Msg) -> Model {
    match msg {
        Msg::Increment => Model{ count: m.count + 1 },
        Msg::Decrement => Model{ count: m.count - 1 },
        Msg::Reset => Model{ count: 0 }
    }
}

fn view(m: Model, dispatch: Fn) {
    h.div(class: "p-6 max-w-sm mx-auto bg-white rounded-xl shadow-md") {
        h.h1(f"Contador: {m.count}", class: "text-2xl font-bold")
        h.div(class: "flex gap-2 mt-4") {
            h.button("+1", class: "btn-primary", on_click: _ => dispatch(Msg::Increment))
            h.button("-1", class: "btn-secondary", on_click: _ => dispatch(Msg::Decrement))
            h.button("Zerar", class: "btn-danger", on_click: _ => dispatch(Msg::Reset))
        }
    }
}

h.mount("#app", init = Model{ count: 0 }, update, view)
```
