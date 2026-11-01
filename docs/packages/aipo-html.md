# aipo.html — Framework Web Declarativo e SSR

`aipo.html` é o pacote canônico da linguagem Aipo para construção de aplicações web modernas, Single Page Applications (SPAs) e renderização no lado do servidor (*Server-Side Rendering — SSR*).

O pacote combina:
1. **Sintaxe Declarativa Baseada em Blocos:** sem JSX, sem macros invasivas, usando funções puras de tag com blocos `do ... end` ou `{ ... }`.
2. **Serialização SSR Pura (`render_to_string`):** geração instantânea de HTML5 estático com mitigação contra XSS via escape automático de entidades em corpos de texto e atributos.
3. **Fragmentos Virtuais (`fragment`):** agrupamento de múltiplos nós irmãos sem emitir elementos contêineres adicionais no HTML final.
4. **CSS-in-Aipo com Escopo e Media Queries (`css`):** geração determinística de classes com hash, suporte a pseudo-classes (`hover`, `active`, `focus`) e blocos `@media` responsivos, com injeção automática no `<head>` (`get_injected_css`).
5. **Arquitetura MVU/TEA Reativa com Comandos (`mount`, `Cmd`):** arquitetura inspirada em The Elm Architecture, permitindo estado previsível com `[model, cmd]`.

---

## 1. Instalação e Configuração

No arquivo `aipo.toml` do seu projeto:

```toml
[dependencies]
"aipo.html" = { path = "packages/aipo-html" }
```

---

## 2. Sintaxe Declarativa de Tags

O `aipo.html` provê funções de tag nativas que aceitam atributos como dicionário e o corpo de elementos filhos via bloco de fechamento (*trailing block*):

### Contêineres com Filhos

```text
import aipo.html as h

let page = h.div({ "class": "container", "id": "main" }) do
    h.header do
        h.h1("Portal Aipo")
    end
    h.main do
        h.section({ "class": "hero" }) do
            h.p("Desenvolvimento web declarativo e determinístico.")
            h.button("Explorar", { "class": "btn-primary" })
        end
    end
    h.footer do
        h.span("© 2026 Aipo Lang")
    end
end
```

### Tags Folha e Elementos Void

Elementos sem fechamento (como `img`, `input`, `hr`, `br`) são tratados como tags folha automáticas:

```text
let avatar = h.img("avatar.png", "Avatar do Usuário", { "class": "rounded-full" })
let email_field = h.input({ "type": "email", "placeholder": "contato@aipo.dev" })
let line = h.hr()
```

### Fragmentos Virtuais

Quando você deseja renderizar múltiplos nós sem criar um nó pai `<div>` desnecessário no DOM ou no SSR:

```text
let list_items = h.fragment do
    h.li("Primeiro item")
    h.li("Segundo item")
    h.li("Terceiro item")
end
```

---

## 3. Renderização no Servidor (SSR)

A função `render_to_string` converte uma árvore de nós em uma string HTML5 segura:

```text
import aipo.html as h

let doc = h.div({ "class": "article-card" }) do
    h.h2("Escaping <script> & Seguro")
    h.p("Conteúdo auditado sem injeção de código.")
    h.input({ "disabled": true, "type": "text" })
end

let html = h.render_to_string(doc)
# Produz:
# <div class="article-card"><h2>Escaping &lt;script&gt; &amp; Seguro</h2><p>Conteúdo auditado sem injeção de código.</p><input disabled type="text" /></div>
```

---

## 4. CSS-in-Aipo Tipado e Media Queries

O `h.css` gera classes de estilo com hashes únicos e isolamento de escopo:

```text
import aipo.html as h

let btn_style = h.css({
    "padding": 12,
    "background": "#6366f1",
    "border_radius": 8,
    "color": "#ffffff",
    "hover": {
        "background": "#4f46e5"
    },
    "media": {
        "(min-width: 768px)": {
            "padding": 20
        }
    }
})

# No elemento:
let botao = h.button("Ação", { "class": btn_style })

# Para embutir no <head> no SSR:
let css_head = h.get_injected_css()
```

---

## 5. Reatividade MVU com Comandos (TEA)

Para SPAs e interfaces dinâmicas, `aipo.html` implementa o padrão Model-View-Update completo com tuplas `[model, cmd]`:

```text
import aipo.html as h

struct Model {
    count
}

fn update(m, msg) {
    if msg == "inc" {
        return Model{ count: m.count + 1 }
    } elif msg == "dec" {
        return Model{ count: m.count - 1 }
    } elif msg == "reset_and_double" {
        # Retorna novo modelo e dispara comando subsequente
        return [Model{ count: 10 }, h.cmd_msg("inc")]
    }
    return m
}

fn view(m, dispatch) {
    return h.div({ "class": "counter-box" }) do
        h.h1(f"Valor atual: {m.count}")
        h.button("+1", { "on_click": fn () { dispatch("inc") } })
        h.button("-1", { "on_click": fn () { dispatch("dec") } })
    end
}

# Inicialização
let app = h.mount("#app", Model{ count: 0 }, update, view)
```
