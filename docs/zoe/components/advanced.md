# Componentes Avançados

Widgets especializados de alta complexidade para ferramentas profissionais, editores e IDEs.

---

## `code_editor`

Editor de código de alta performance projetado para scripts in-game, consoles de debug e ferramentas de desenvolvimento:
- **Gutter com Numeração Lateral**: Largura dinâmica calculada com base na contagem de linhas.
- **Realce Sintático Nativo de Aipo**: Tokenização automática de palavras-chave (`fn`, `let`, `var`, `if`, `while`), tipos (`Int`, `String`), strings, comentários e números.
- **Cursor e Linha Ativa**: Destaque translúcido na linha selecionada e barra de cursor vertical.
- **Culling Vertical**: Linhas fora da área visível são descartadas da renderização para manter 60 FPS estável.

```text
let script = "fn update(dt) {\n    let speed = 120.0\n    player.x += speed * dt\n}"

zoe.code_editor({
    "code": script,
    "cursor_line": 2,
    "cursor_col": 14,
    "width": "100%",
    "height": 260.0,
    "show_gutter": true
})
```

---

## `node_graph`

Canvas infinito para grafos visuais (shaders de nós, árvores de diálogo e blueprints lógicas):
- **Cabos em Curvas de Bézier**: Conexões desenhadas com fragment shaders em GPU (`host_draw_bezier`) com brilho (*glow*) e suavização contínua.
- **Cartões de Nós Táteis**: Cabeçalho de destaque com cores semânticas, bordas arredondadas e sombras de contato.
- **Portas de Soquete Coloridas**: Indicadores circulares de tipo (float, vetores, texturas, fluxo).

```text
let nos = [
    {
        "id": "textura",
        "title": "Textura 2D",
        "x": 40.0,
        "y": 60.0,
        "color": zoe.color.blue,
        "outputs": [{ "name": "RGBA", "color": zoe.color.yellow }]
    },
    {
        "id": "saida",
        "title": "Fragment Shader",
        "x": 320.0,
        "y": 80.0,
        "color": zoe.color.green,
        "inputs": [{ "name": "Cor Final", "color": zoe.color.yellow }]
    }
]

let conexoes = [
    {
        "from_node": "textura",
        "from_socket": "RGBA",
        "to_node": "saida",
        "to_socket": "Cor Final",
        "color": zoe.color.yellow
    }
]

zoe.node_graph({
    "nodes": nos,
    "connections": conexoes,
    "width": "100%",
    "height": 400.0
})
```

---

## `modal_dialog` e `open_modal`

Diálogos modais elevados com escurecimento de fundo (*scrim backdrop*), botão de fechamento e ações customizáveis:

```text
fn abrir_confirmacao() {
    let corpo = zoe.label("Deseja exportar a cena atual?", { "font_size": 13.0 })
    
    zoe.open_modal("Exportar Projeto", [corpo], fn() {
        print("Exportação confirmada!")
    }, none, {
        "width": 380.0,
        "height": 180.0,
        "confirm_text": "Exportar",
        "cancel_text": "Cancelar"
    })
}
```
