# aipo.html — Declarative Web Framework and SSR

`aipo.html` is the canonical package for the Aipo programming language designed to build modern web applications, Single Page Applications (SPAs), and Server-Side Rendered (SSR) interfaces.

It features:
1. **Declarative Block-Based Syntax:** No JSX, no macros; pure tag functions using trailing `do ... end` or `{ ... }` blocks.
2. **Pure SSR Serialization (`render_to_string`):** Instant static HTML5 generation with XSS mitigation via automatic entity escaping in text bodies and attributes.
3. **Virtual Fragments (`fragment`):** Render multiple sibling elements without generating extra wrapper nodes.
4. **Scoped CSS-in-Aipo with Media Queries (`css`):** Deterministic class hashing, pseudo-classes (`hover`, `active`, `focus`), and responsive `@media` query blocks with `<head>` inlining (`get_injected_css`).
5. **Reactive MVU/TEA Architecture with Commands (`mount`, `Cmd`):** The Elm Architecture pattern supporting state updates and command dispatch chains `[model, cmd]`.

---

## 1. Installation

In your project's `aipo.toml`:

```toml
[dependencies]
"aipo.html" = { path = "packages/aipo-html" }
```

---

## 2. Declarative Tag Syntax

`aipo.html` provides tag functions that take attributes as dictionaries and child nodes via trailing closure blocks:

### Container Tags with Children

```text
import aipo.html as h

let page = h.div({ "class": "container", "id": "main" }) do
    h.header do
        h.h1("Aipo Portal")
    end
    h.main do
        h.section({ "class": "hero" }) do
            h.p("Declarative and deterministic web development.")
            h.button("Explore", { "class": "btn-primary" })
        end
    end
    h.footer do
        h.span("© 2026 Aipo Lang")
    end
end
```

### Leaf Tags and Void Elements

Self-closing elements (such as `img`, `input`, `hr`, `br`) are rendered without closing tags:

```text
let avatar = h.img("avatar.png", "User Avatar", { "class": "rounded-full" })
let email_field = h.input({ "type": "email", "placeholder": "contact@aipo.dev" })
let line = h.hr()
```

### Virtual Fragments

When you need to return multiple sibling nodes without an extra wrapping element:

```text
let list_items = h.fragment do
    h.li("First item")
    h.li("Second item")
    h.li("Third item")
end
```

---

## 3. Server-Side Rendering (SSR)

The `render_to_string` function serializes a virtual node tree into safe HTML5 markup:

```text
import aipo.html as h

let doc = h.div({ "class": "article-card" }) do
    h.h2("Escaping <script> & Safe")
    h.p("Audited content with entity escaping.")
    h.input({ "disabled": true, "type": "text" })
end

let html = h.render_to_string(doc)
```

---

## 4. Scoped CSS-in-Aipo & Media Queries

`h.css` generates unique class hashes and isolates styling scope:

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

let button_node = h.button("Action", { "class": btn_style })

# SSR Inlining into HTML <head>:
let css_head = h.get_injected_css()
```

---

## 5. Reactive MVU Architecture with Commands (TEA)

For interactive SPAs, `aipo.html` implements the Model-View-Update pattern supporting command chains:

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
        return [Model{ count: 10 }, h.cmd_msg("inc")]
    }
    return m
}

fn view(m, dispatch) {
    return h.div({ "class": "counter-box" }) do
        h.h1(f"Current count: {m.count}")
        h.button("+1", { "on_click": fn () { dispatch("inc") } })
        h.button("-1", { "on_click": fn () { dispatch("dec") } })
    end
}

let app = h.mount("#app", Model{ count: 0 }, update, view)
```
