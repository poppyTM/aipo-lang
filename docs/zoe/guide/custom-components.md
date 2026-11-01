# Criando Componentes Customizados

Uma das maiores forças do Zoe UI é seu protocolo aberto e uniforme de autoria de componentes. Qualquer desenvolvedor pode criar novos elementos sem wrappers nativos ou forks da biblioteca.

---

## 1. O Contrato Canônico de Componente

Todo componente no Zoe é uma função em Aipo que recebe um dicionário de propriedades (`props`) e, opcionalmente, uma lista de filhos (`children`), retornando um `ElementNode`:

```text
fn make_meu_componente(props: Dict = {}, children: List = []) {
    let width_val = if props.has("width") then props["width"] else "auto"
    let height_val = if props.has("height") then props["height"] else 32.0

    return zoe.rect(
        {
            "tag": "meu_componente",
            "direction": "row",
            "align_items": "center",
            "width": width_val,
            "height": height_val,
            "padding_x": 12.0,
            "gap": 8.0,
            "background": zoe.color.surface_0,
            "border_width": 1.0,
            "border_color": zoe.color.surface_2,
            "border_radius": 6.0
        },
        children
    )
}
```

---

## 2. Desenho Customizado em GPU (`on_custom_draw`)

Para componentes que necessitam desenhar primitivas especializadas (como cabos de Bézier, formas vetoriais, osciloscópios ou editores gráficos), utilize o gancho `on_custom_draw`:

```text
fn make_radar_chart(props: Dict = {}) {
    let on_custom_draw = fn(node) {
        if node == none or node.layout == none {
            return
        }
        let r = node.layout
        
        # Desenha um círculo central
        host_draw_circle(r.x + r.width / 2.0, r.y + r.height / 2.0, 30.0, 0.2, 0.6, 0.9, 0.8)
    }

    return zoe.rect({
        "tag": "radar_chart",
        "width": 200.0,
        "height": 200.0,
        "on_custom_draw": on_custom_draw
    }, [])
}
```

Ao registrar `on_custom_draw`, o nó participa normalmente de todas as passadas de layout do Leona e recebe a geometria final `node.layout.x`, `node.layout.y`, `node.layout.width`, `node.layout.height`.
