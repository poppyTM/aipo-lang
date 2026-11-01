# Playground Interativo do Zoe UI

Experimente o **Zoe UI** e o motor de layout **Leona** diretamente na documentação.

---

## Demonstração Viva de Componentes

O Zoe UI oferece uma vasta gama de primitivas com tema Catppuccin calibrado. Abaixo você encontra um exemplo completo combinando o layout split, scrubber inputs, visualizador e gráficos de nós:

```text
import aipo.zoe as zoe

fn view() {
    let valor_x = zoe.use_state(12.5)
    let editor_script = zoe.use_state("fn on_start() {\n    print(\"Zoe UI Ativo!\")\n}")

    let barra_topo = zoe.row({
        "height": 40.0,
        "background": zoe.color.mantle,
        "align_items": "center",
        "padding_x": 16.0,
        "gap": 12.0
    }, [
        zoe.icon("lucide:sparkles", { "color": zoe.color.blue, "size": 18.0 }),
        zoe.label("Zoe Playground Studio", { "font_weight": "bold", "color": zoe.color.text }),
        zoe.spacer(),
        zoe.badge("v0.2.0 SDF", { "background": zoe.color.surface_1, "color": zoe.color.green })
    ])

    let editor = zoe.code_editor({
        "code": editor_script,
        "height": 240.0
    })

    let inspector = zoe.column({ "gap": 10.0, "padding": 12.0, "background": zoe.color.base }, [
        zoe.label("Coordenada Transform:", { "font_size": 12.0, "color": zoe.color.lavender }),
        zoe.scrubber_input({
            "label": "X",
            "value": valor_x,
            "min": -100.0,
            "max": 100.0,
            "width": "100%"
        }),
        zoe.button("Executar Código", _ => print("Executando..."), { "variant": "primary" })
    ])

    return zoe.column({ "width": "100%", "height": "100%" }, [
        barra_topo,
        zoe.split_view({ "split": 0.65 }, editor, inspector)
    ])
}

fn setup() {
    zoe.mount(view)
}

fn update(dt) {
    zoe.step(dt)
}

fn draw() {
    zoe.draw_ui()
}
```

---

## 💻 Como Rodar Localmente

Para rodar este ou qualquer exemplo do Zoe UI localmente em sua máquina com aceleração por hardware nativa:

```bash
# Clone ou acesse o repositório
cd aipo-lang

# Execute o exemplo do Editor Integrado (Game Engine + 3D Viewport + Shaders SDF):
cargo run -p aipo-game-host -- packages/aipo-zoe/examples/editor.aipo
```
