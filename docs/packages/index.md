# Pacotes & Frameworks Oficiais

O ecossistema oficial da linguagem **Aipo** é composto por bibliotecas e frameworks de alto nível projetados com os mesmos padrões de rigor, segurança por padrão e determinismo do compilador central.

Conforme estabelecido pela governança da linguagem, **recursos de domínio específico (como HTML, CSS, UI, simulações e áudio) não inflam a gramática do compilador**. Em vez disso, eles são distribuídos como pacotes canônicos gerenciados pelo `aipo package`.

---

## Catálogo de Pacotes Oficiais

| Pacote | Versão | Domínio | Descrição |
|---|:---:|---|---|
| **[`aipo.html`](/packages/aipo-html)** | `v0.1.0` | **Web, SSR & DOM** | DSL declarativa para HTML5, serializador SSR, CSS-in-Aipo com media queries e reatividade com **MVU/TEA com Comandos**. |
| **[`aipo.http`](/packages/aipo-http)** | `v0.1.0` | **Web Server & APIs** | Framework HTTP com roteador zero-regex em árvore de segmentos, pipeline estilo cebola, context engine e dispatch desacoplado. |
| **[`aipo.zoe`](/packages/aipo-zoe)** | `v0.1.0` | **GUI Declarativa & Leona** | Framework declarativo de interface de usuário em Aipo puro, com aceleração GPU a 60+ FPS, hooks de estado e engine de layout Leona. |
| **[`aipo.ui`](/packages/aipo-ui)** | `v0.1.0` | **UI Multiplataforma** | Framework declarativo de interface universal (Desktop GPU via Skia/Freya, WebGL e Terminal TUI) com layout Flexbox/Taffy. |
| **[`aipo.game`](/packages/aipo-game)** | `v0.1.0` | **Game Engine 2D** | Micro-engine 2D orientada a Atores e Cenas com comportamentos em 1 linha, nós visuais e determinismo de simulação. |

---

## Como Utilizar um Pacote Oficial

No arquivo `aipo.toml` do seu projeto, declare a dependência sob a tabela `[dependencies]`:

```toml
[package]
name = "meu-projeto"
version = "0.1.0"

[dependencies]
"aipo.html" = { path = "packages/aipo-html" }
```

Em seguida, no seu código `.aipo`:

```text
import aipo.html as h

h.div(class = "app") {
    h.h1("Construído com Aipo!")
}
```
