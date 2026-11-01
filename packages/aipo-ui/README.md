# aipo.ui

Pacote oficial da linguagem **Aipo** para construção de interfaces declarativas universais e multiplataforma com layout **Taffy** (CSS Flexbox e CSS Grid) e renderizadores desacoplados (**Desktop GPU Skia**, **WebGL/Canvas** e **Terminal TUI**).

---

## Filosofia & Separação de Domínios

Diferente do [`aipo.html`](../aipo-html/README.md) (que opera estritamente sobre a árvore DOM de navegadores e CSS web tradicional), o `aipo.ui` é **100% agnóstico de plataforma**:

1. **Mesma Árvore Declarativa em Qualquer Lugar:** Um único código de interface roda nativamente no Desktop com aceleração por hardware (Vulkan/Metal/DirectX via Skia), no navegador via WebAssembly/Canvas sem custos de reflow do DOM, ou diretamente no Terminal com cores ANSI 24-bit TrueColor.
2. **Motor de Layout Taffy Integrado:** Implementação de ponta de CSS Flexbox e Grid em Rust, compilada com zero overhead de interpretação, calculando coordenadas com precisão de submícron.
3. **Reatividade MVU Previsível:** Estado imutável gerenciado pelo padrão *Model-View-Update* com despachos tipados e zero mutação oculta.

---

## Instalação

Adicione ao seu `aipo.toml`:

```toml
[dependencies]
"aipo.ui" = { path = "packages/aipo-ui" }
```

---

## Exemplo Rápido: Contador Universal

Este exemplo é executável: [`examples/counter.aipo`](examples/counter.aipo). Ele existe
na forma real da linguagem — Aipo não tem `enum` nem convenção de `main`, e um arquivo de
entrada precisa chamar algo por conta própria.

```text
import aipo.ui as ui

struct Model {
    var count = 0
}

fn update(m, msg) -> Model {
    var next = Model{ count: m.count }
    if msg == "increment" {
        next.count += 1
    } elif msg == "decrement" {
        next.count -= 1
    } elif msg == "reset" {
        next.count = 0
    }
    return next
}

fn view(m, dispatch) {
    let label = f"Valor Atual: {m.count}"

    return ui.Column(
            gap = 16,
            padding = 32,
            align = ui.align_center().value,
            justify = ui.justify_center().value,
            background = ui.rgb(24, 24, 27),
            body = fn() {
                ui.Text(label, font_size = 28.0, font_weight = "bold", color = ui.white())
                ui.Spacer()
            }
        )
}

fn start() {
    ui.mount_desktop("Contador Aipo UI", 400, 300, Model{ count = 0 }, update, view)
}
```

Saída de `aipo run examples/counter.aipo`:

```text
root=Column children=2
texto=Valor Atual: 0 cor=24,24,27
montagem concluida; o desenho fica a cargo do host
```

### Notas de sintaxe

Duas limitações da revisão atual moldam o exemplo acima. Ambas têm reprodução mínima em
[`docs/journal/2026-10-05-syntax-drift.md`](../../docs/journal/2026-10-05-syntax-drift.md).

| Limite | Como contornar |
|---|---|
| Argumento nomeado combinado com bloco trailer (`ui.Column(gap = 16) do { }`) não preenche defaults e falha com `AIPO_RT_TYPE_MISMATCH` | Passe os filhos como `body = fn() { ... }` |
| Um closure invocado que, dentro dele, cria outro closure capturando um upvalue causa `operand stack underflow` | Closes `body` são invocados pelo runtime: não crie closures aninhadas dentro deles. Por isso `ui.Button(on_click = ...)` aninhado em `body` ainda não é seguro |

### Tags de layout sem `enum`

Aipo não tem `enum` na V1 (está no backlog pós-V1). Cada tag é uma struct que carrega um
token canônico, com um construtor por variante:

```text
ui.align_center().value          # "center"
ui.justify_space_between().value # "space-between"
ui.direction_row_reverse().value # "row-reverse"
ui.overflow_scroll().value       # "scroll"
```

---

## API Pública

O módulo `lib.aipo` é a fachada e possui todos os nomes públicos. Como `export` não aceita
nomes qualificados e `import` não cria namespace, os submódulos exportam auxiliares com
prefixo `make_*` e a fachada define o nome curto — o mesmo padrão de
[`aipo.zoe`](../aipo-zoe/README.md) e [`aipo.html`](../aipo-html/README.md).

| Grupo | Símbolos |
|---|---|
| Layout | `Box` `Row` `Column` `Stack` `ScrollArea` `Spacer` |
| Widgets | `Text` `Button` `TextInput` `Slider` `Checkbox` `ProgressBar` |
| Tags | `align_*` `justify_*` `direction_*` `overflow_*` |
| Cor | `rgb` `rgba` `hex` `white` `black` `transparent` |
| Ciclo de vida | `mount_desktop` `mount_canvas` `mount_tui` `dispatch` |

Os tipos `Rect`, `Color`, `UINode`, `UIEvent` e as quatro structs de tag **não** podem ser
re-exportados pela fachada (limitation de `export`). Quem precisar construí-los diretamente
importa `aipo.ui.src.types`.

---

## Renderizadores Pluggáveis

| Renderizador | Alvo | Motor de Desenho | Características |
|---|---|---|---|
| **`aipo-ui-renderer-skia`** | Desktop (Linux, macOS, Windows) | Skia 2D / GPU (Vulkan, Metal, DX12) | 120+ FPS, anti-aliasing de texto subpixel, sombras com blur gaussiano, aceleração total de GPU. |
| **`aipo-ui-renderer-canvas`** | WebAssembly (Navegadores) | HTML5 Canvas 2D / WebGL | Aplicação executando em bytecode Wasm de alto desempenho, contornando a árvore DOM e repaints do browser. |
| **`aipo-ui-renderer-tui`** | Terminal ANSI | Terminal Virtual / Crossterm | Execução instantânea em servidores e terminais sem servidor X11/Wayland, com suporte a mouse e 24-bit TrueColor. |

---

## Testes

```bash
aipo test packages/aipo-ui
```

9 testes cobrem tags, cores, construção da árvore, semântica de props, primitivas de
layout e o mount MVU. A suíte é executada no CI por
`crates/aipo-cli/tests/package_suites.rs`, que é a razão de este pacote não voltar a
derivar para fora da gramática sem ninguém perceber.
