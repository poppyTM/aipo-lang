# Zoe UI (`aipo.zoe`)

Framework declarativo de interface de usuário (GUI) para a linguagem **Aipo**, baseado no algoritmo de layout **Leona**, sistema reativo de hooks (`use_state`) e componentes visuais estilizados na paleta **Catppuccin Mocha**.

O **Zoe UI** foi concebido para o ecossistema Aipo com arquitetura pura, declarativa e leve: o código roda diretamente na máquina virtual Aipo e renderiza em GPU nativa (60+ FPS) via [`aipo-game-host`](file:///home/raillen/Documentos/Projetos/aipo-lang/crates/aipo-game-host), com compatibilidade pronta para futuros alvos WebAssembly e JavaScript.

---

## Características Principais

1. **Árvore Declarativa e Containers Ergonômicos:**
   Construção de interfaces limpas e expressivas com containers sem cerimônia (`column`, `row`, `stack`, `center`, `spacer`, `divider`, `split_view`, `tab_bar`, `tab_view`, `scroll_view`, `viewport`) e catálogo completo de componentes (`button`, `switch`, `slider`, `text_input`, `number_input`, `dropdown_select`, `color_picker`, `card`, `badge`, `label`, `rect`).

2. **Engine de Layout Leona:**
   Algoritmo hierárquico de medição e posicionamento que resolve:
   - Dimensões absolutas em pixels (`Float` / `Int`).
   - Dimensões relativas em porcentagem (`"100%"`, `"50%"`).
   - Dimensões intrínsecas e dimensionamento flexível (`"flex"`, `"auto"`).
   - Alinhamento transversal (`align_items`: `"start"`, `"center"`, `"end"`).
   - Distribuição no eixo principal (`justify_content`: `"start"`, `"center"`, `"end"`).
   - Espaçamento interno (`padding`, `padding_x`, `padding_y`) e espaçamento entre filhos (`gap`).

3. **Reatividade Fina com Hooks:**
   Runtime de sinais reativos (`use_state`, `set_state`) com rastreamento automático de quadros sujos (*dirty flags*), disparando reconstrução e re-layout apenas quando o estado sofre mutação.

4. **Catppuccin Mocha Embutido:**
   Paleta completa de cores modernas acessíveis via `zoe.color` (`zoe.color.base`, `zoe.color.blue`, `zoe.color.mantle`, etc.) e utilitários `rgb`, `rgba`, `hex`.

5. **Aceleração por GPU Nativa:**
   Renderização com 60+ FPS no desktop nativo conectada diretamente ao host Miniquad através do `aipo-game-host`.

6. **Ferramentas de Viewport & Edição Visual (Picking e Gizmos):**
   Suporte a sub-retângulo GPU Scissor e câmera 2D (`set_viewport_camera`), transformações bidirecionais de tela/mundo (`world_to_screen`, `screen_to_world`), detecção espacial AABB (`point_in_rect`), hit-testing de manipuladores (`test_gizmo_hit`) e renderização de Gizmos de translação 2D (`draw_gizmo_2d`) com restrição de eixos (X, Y e Centro livre).

7. **Motor 3D Retro Completo (`retro3d`):**
   Renderização 3D completa com estética retro sem sobrecarga de pipelines pesados:
   - **Matemática Vetorial 3D:** Funções nativas `vec3`, `vec3_add`, `vec3_sub`, `vec3_scale`, produto escalar/vetorial e normalização.
   - **Câmera Orbital Perspectiva:** `camera_3d` e `project_3d` com rotação `yaw` (horizontal) e `pitch` (vertical), distância orbital e *near-plane clipping*.
   - **Primitivas de Malha:** Criação de malhas tridimensionais com vetores normais (`cube_mesh`, `pyramid_mesh`).
   - **Grade de Chão Infinita:** `draw_grid_3d` com linhas convergentes ao horizonte.
   - **Rasterizador Sólido com Flat-Shading:** `render_mesh_3d` com iluminação difusa direcional (Lambertian), descarte de faces traseiras (*backface culling*) em screen space e ordenação de triângulos pelo Algoritmo do Pintor (*Painter's depth sorting*).
   - **Gizmo 3D e Picking:** Manipulador de 3 eixos (`draw_gizmo_3d`, `test_gizmo_3d_hit`) com X (Pêssego), Y (Verde), Z (Azul) e cubo central para manipulação direta de entidades 3D no espaço.

---

## Instalação

Adicione a dependência ao seu `aipo.toml`:

```toml
[dependencies]
"aipo.zoe" = { path = "packages/aipo-zoe" }
```

Gere ou audite o lockfile do seu projeto:

```bash
aipo package lock .
aipo package audit .
```

---

## Exemplo Rápido: Contador Reativo Ergonômico

```text
import aipo.zoe as zoe

fn view() {
    let count = zoe.use_state(0)

    return zoe.center({ "background": zoe.color.base, "gap": 16.0 }, [
        zoe.label(f"Contador: {count.get()}", {
            "font_size": 24.0,
            "color": zoe.color.text
        }),
        zoe.button("Incrementar +1", _ => zoe.set_state(count, count.get() + 1), {
            "variant": "primary",
            "background": zoe.color.blue
        })
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

## Executando Exemplos de Demonstração

- Um dashboard completo e interativo com contador, switches, sliders e barra de progresso está disponível em `examples/dashboard.aipo`:
```bash
cargo run -p aipo-game-host -- packages/aipo-zoe/examples/dashboard.aipo
```

- Um editor visual de game engine completo com alternância de modos **2D e 3D**, splitters redimensionáveis, abas de hierarquia e assets, viewport com picking de objetos, manipulador de transformação visual (Gizmo 2D e Gizmo 3D de 3 eixos), câmeras interativas (Pan/Zoom 2D e Órbita/Dolly Zoom 3D), renderização retro 3D com iluminação flat e inspetor com edição bidirecional em tempo real (coordenadas X, Y e Z) está disponível em `examples/editor.aipo`:
```bash
cargo run -p aipo-game-host -- packages/aipo-zoe/examples/editor.aipo
```

---

## Testes Automatizados

O pacote conta com uma suíte de testes de unidade e integração:

```bash
# Executa a suíte de testes do framework Zoe UI
cargo test -p aipo-game-host --test bridge_tests test_zoe_ui_unit_test_suite

# Executa o teste de compilação e execução estável multi-frame do Dashboard
cargo test -p aipo-game-host --test bridge_tests test_zoe_ui_dashboard_compilation_and_execution

# Executa o teste de compilação e execução estável multi-frame do Editor Visual
cargo test -p aipo-game-host --test bridge_tests test_zoe_ui_editor_compilation_and_execution
```

---

## Roadmap de Amadurecimento Arquitetural (M1 a M6 — Concluído)

O desenvolvimento do Zoe UI concluiu o plano de evolução inspirado nas melhores práticas da indústria (Freya/Torin, Flutter, Egui, SolidJS e Slint), preservando a filosofia de ser 100% puro em Aipo e livre de dependências pesadas de UI do SO:

1. **M1 — Superfícies Flutuantes (Portals & Overlays) [Implementado]:** Introdução da `OverlayStack` pós-clipping com suporte a diálogos modais (`dialog`, `open_dialog`), menus flutuantes reais com sombreamento (`dropdown_menu`, `dropdown_item`) e balões de tooltip com atraso de hover suave (350ms).
2. **M2 — Leona Constraints & Intrinsic Wrap [Implementado]:** Suporte a restrições `min_width`, `max_width`, `min_height`, `max_height`, quebra de fluxo com `wrap: true` / `flex_wrap: true`, contêiner de sobreposição espacial `stack` e medições intrínsecas enxutas e eficientes.
3. **M3 — Reatividade Fina & Memoização de Sub-árvores [Implementado]:** Hooks `use_memo` e `use_effect`, com cache de dependências e invalidação seletiva de nós para eliminar reconstruções globais desnecessárias da árvore de elementos.
4. **M4 — Foco, Acessibilidade & Navegação por Teclado [Implementado]:** Navegação universal via `Tab`/`Shift+Tab`, anéis visuais de foco (*focus ring*) com halo neon Catppuccin, ativação com `Enter`/`Espaço`, descarte com `Escape` e gerenciamento de foco (`get_focused_node`, `set_focused_node`).
5. **M5 — Componentes de Produtividade & Ferramentas [Implementado]:** Implementação do container de árvore hierárquica navegável (`tree_view`) com chevrons expansíveis e seleção ativa, e lista virtualizada para grandes coleções (`virtual_list` em $O(\text{viewport})$ de nós).
6. **M6 — Micro-animações & Inspetor Visual DevTools [Implementado]:** Interpolações contínuas (`use_tween`) sincronizadas com `step(dt)` (curvas `linear`, `ease_in`, `ease_out`, `ease_in_out`) e inspetor de layout geométrico em tempo real (*DevTools Inspector Overlay* alternável via tecla `F12` ou `toggle_inspector`).
7. **M7 — Design Tokens, Motor de Ícones Vetoriais e Componentes Modernos [Implementado]:**
   - **Design Tokens (`zoe.tokens`):** Tipografia escalada (10px a 18px), grade de 4px/8px, raios de borda, alturas de controle e superfícies semânticas Catppuccin Mocha (`bg_canvas`, `bg_panel`, `bg_surface`, `accent_x`, `accent_y`, `accent_z`).
   - **Motor de Ícones Vetoriais por Hardware:** Rasterização via `tiny-skia` + `svgtypes` com cache de texturas GPU LRU (`Texture2D`), suportando `zoe.icon` e `zoe.register_icon` compatíveis com Lucide, Heroicons, Phosphor, Tabler, Material Symbols e Devicons.
   - **Componentes de Precisão:** `scrubber_input` (arrasto horizontal com `Shift` 0.1x micro e `Ctrl` 10x snap), `segmented_group` (controle em pílula com superfície elevada) e `hierarchy_tree` (árvore de cena com seleção total de largura, rails de guia e badges de tipo semântico).
8. **M8 — Motor Tipográfico Subpixel & Fonte Inter [Implementado]:**
   - **Fonte Inter Embutida:** Substituição da fonte padrão ProggyClean pela Inter Variable (860KB) com hinting subpixel profissional.
   - **Medição Pixel-Perfect:** Novas primitivas nativas `host_measure_text` (largura exata com kerning via `fontdue`) e `host_font_metrics` (ascent, descent, line_gap, line_height) para layout Leona e rendering com alinhamento por baseline real.
9. **M9 — Shaders Analíticos de UI (GPU SDF) [Implementado]:**
   - **Fragment Shader SDF por Hardware:** Renderização matemática analítica (`sd_rounded_box`) de quads arredondados com *anti-aliasing* contínuo direto no pixel shader da GPU.
   - **Primitiva Unificada `host_draw_sdf_rect`:** Integração de curvatura precisa (`border_radius`), borda interna contínua de 1px e *top inner highlight* simulando iluminação física ambiente com reflexão no chanfro superior.
10. **M10 — Leona 2.0 & Alinhamento por Baseline Tipográfica [Implementado]:**
   - **Layout Baseado em Métricas Reais da Fonte:** Eliminação de tamanhos arbitrários; contêineres medem conteúdos intrínsecos através de `host_font_metrics` e `host_measure_text`.
   - **Suporte a `align_items: "baseline"`:** Alinhamento milimétrico de ícones e textos pela linha de base tipográfica em linhas horizontais.
   - **Otimização Estrutural de Stack Frames:** Redução do consumo de slots da pilha do interpretador de 70 para ~18 slots por nível, permitindo árvores profundas (30+ níveis) com zero risco de overflow de operand stack.
11. **M11 — Catálogo de Componentes Avançados [Implementado]:**
   - **`code_editor`:** Editor de código virtualizado com gutter e numeração de linhas, realce sintático léxico completo para a linguagem Aipo (`fn`, `let`, `var`, `if`, strings, comentários, números), cursor com tween e culling vertical.
   - **`node_graph`:** Canvas visual para grafos lógicos e shaders com grade sutil, cartões de nós com portas de soquete coloridas por tipo (float, vec, color, tex) e cabos suaves renderizados via `host_draw_bezier` com brilho e suavização.
   - **`modal_dialog` & `open_modal`:** Sistema de diálogos modais flutuantes com backdrop scrim escurecido e botões de ação estilizados.
12. **M12 — Site Documental Dedicado do Zoe UI & Playground [Implementado]:**
   - **Hub Documental no VitePress:** Estrutura completa em `docs/zoe/` (e espelho bilíngue em `docs/en/zoe/`) com guias de arquitetura, Leona 2.0, reatividade e catálogo de componentes.
   - **Playground Interativo:** Demonstrações e exemplos de código prontos para execução direta.

Consulte a documentação completa no hub do Zoe UI em [`docs/zoe/index.md`](file:///home/raillen/Documentos/Projetos/aipo-lang/docs/zoe/index.md) e [`docs/en/zoe/index.md`](file:///home/raillen/Documentos/Projetos/aipo-lang/docs/en/zoe/index.md).


