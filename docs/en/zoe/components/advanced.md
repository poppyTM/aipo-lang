# Advanced Widgets

Specialized, high-complexity widgets tailored for developer tooling and IDE applications.

---

## `code_editor`

High-performance virtualized code editor:
- **Gutter with Line Numbers**: Dynamically measured based on line count.
- **Aipo Syntax Highlighting**: Automatic tokenization of keywords, types, strings, comments, and numeric literals.
- **Blinking Cursor & Active Line Highlight**: Visual emphasis on the active row.
- **Vertical Culling**: Unrendered lines outside viewport bounds are culled to ensure 60 FPS performance.

```text
let script = "fn update(dt) {\n    let speed = 120.0\n    player.x += speed * dt\n}"

zoe.code_editor({
    "code": script,
    "cursor_line": 2,
    "cursor_col": 14,
    "width": "100%",
    "height": 260.0
})
```

---

## `node_graph`

Infinite visual canvas for shader graphs, dialogue trees, and blueprint logic:
- **Bézier Connection Cables**: GPU-rendered cables with glow and smoothstep anti-aliasing via `host_draw_bezier`.
- **Tactile Node Cards**: Rounded borders, top color accents, and contact drop shadows.
- **Color-Coded Sockets**: Semantic circular ports for floats, vectors, colors, and textures.

```text
let nodes = [
    {
        "id": "tex",
        "title": "Texture 2D",
        "x": 40.0,
        "y": 60.0,
        "color": zoe.color.blue,
        "outputs": [{ "name": "RGBA", "color": zoe.color.yellow }]
    },
    {
        "id": "out",
        "title": "Fragment Shader",
        "x": 320.0,
        "y": 80.0,
        "color": zoe.color.green,
        "inputs": [{ "name": "Final Color", "color": zoe.color.yellow }]
    }
]

let connections = [
    {
        "from_node": "tex",
        "from_socket": "RGBA",
        "to_node": "out",
        "to_socket": "Final Color",
        "color": zoe.color.yellow
    }
]

zoe.node_graph({
    "nodes": nodes,
    "connections": connections,
    "width": "100%",
    "height": 400.0
})
```

---

## `modal_dialog` and `open_modal`

Elevated dialog cards with backdrop scrims and action buttons:

```text
fn show_confirm() {
    let body = zoe.label("Export current scene?", { "font_size": 13.0 })
    
    zoe.open_modal("Export Project", [body], fn() {
        print("Export confirmed!")
    }, none, {
        "width": 380.0,
        "height": 180.0,
        "confirm_text": "Export",
        "cancel_text": "Cancel"
    })
}
```
