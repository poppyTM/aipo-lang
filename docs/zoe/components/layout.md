# Contêineres de Layout

Contêineres estruturais para organização espacial com o motor Leona.

---

## `column`, `row`, `stack`, `center`

```text
# Organização Vertical
zoe.column({ "gap": 10.0, "padding": 16.0 }, [
    zoe.label("Item 1"),
    zoe.label("Item 2")
])

# Organização Horizontal com Alinhamento por Baseline
zoe.row({ "align_items": "baseline", "gap": 8.0 }, [
    zoe.icon("lucide:sparkles", { "size": 16.0 }),
    zoe.label("Destaque", { "font_size": 14.0 })
])

# Sobreposição (Stack)
zoe.stack({ "width": 400.0, "height": 300.0 }, [
    fundo_imagem,
    distintivo_flutuante
])

# Centralização Absoluta
zoe.center([
    zoe.label("Centralizado na Tela")
])
```

---

## `split_view`

Divisor redimensionável interativo para painéis estilo IDE e Blender:

```text
let split_pos = zoe.use_state(0.30) # 30% à esquerda

zoe.split_view(
    {
        "split": split_pos,
        "direction": "horizontal",
        "min_first": 150.0,
        "min_second": 200.0
    },
    painel_esquerdo,
    painel_direito
)
```

---

## `viewport`

Contêiner de exibição de cenas 2D ou 3D com tesoura de recorte em GPU (*hardware scissor clip*):

```text
zoe.viewport({
    "width": "100%",
    "height": "flex",
    "on_render": fn(rect) {
        # Código de renderização de cena 2D ou 3D
        zoe.draw_grid_3d(camera, rect)
    }
})
```
