# Dossiê de Modernização Visual e Arquitetural do Zoe UI (`aipo.zoe`)

> **Escopo**: análise profunda da implementação atual do framework GUI `aipo.zoe` e da
> interface de game engine hospedada em `aipo-game-host`, contrastada com as melhores
> bibliotecas e frameworks GUI modernos, com foco em **funcionalidade, otimização,
> design moderno, personalização e beleza visual**.
>
> **Entregáveis**: (1) diagnóstico verificável do estado atual, (2) benchmark técnico
> contra referências de mercado, (3) especificação de design system, (4) estratégia de
> implementação com código concreto, (5) plano de fases, (6) plano de atualização
> documental.
>
> **Data da auditoria**: 2026-09-28 · **Revisão**: e4ef0ee · **Estado do build**: `cargo build -p aipo-game-host` ✅

---

## TL;DR — As 12 conclusões que mais importam

1. **O Zoe não tem batching.** Cada `host_draw_sdf_rect` emite um draw call com 5
   `set_uniform` + troca de material. Uma tela com 300 widgets = 300+ draw calls e
   1500 escritas de uniform por quadro. Nenhum framework de mercado aceita isso.
2. **O shader SDF tem três defeitos matemáticos**, não estilísticos: antialiasing com
   largura fixa de 1px (não usa `fwidth`/norma L2 do gradiente), borda composta por
   `mix` sobre um único SDF (gera linha cinza em fundos claros e raio interno errado),
   e ausência total de sombra.
3. **`math_sin`/`math_cos` do `retro3d` estão numericamente errados** — erro de
   **4,22%** em `cos(0.6)`. Isso é um bug, não uma escolha estética.
4. **Não existe tema em runtime.** Catppuccin Mocha está hardcoded. O `tokens.aipo`
   existe mas **não é consumido** por `components.aipo` — os componentes usam
   `color_mod.surface_1` diretamente. O sistema de tokens é decorativo.
5. **A reatividade é "full rebuild"**: qualquer `set_state` reconstrói a árvore
   inteira e re-roda Leona do zero. Não há memoização de subárvore, cache de layout
   nem invalidação seletiva, apesar do README afirmar "M3 Fine-grained".
6. **`font_weight` é aceito como prop e nunca lido.** Todo `"font_weight": "bold"` no
   código é um no-op. A hierarquia tipográfica não existe.
7. **Não há elisão de texto, alinhamento, letter-spacing nem word-wrap.** Um rótulo
   longo estoura o container silenciosamente.
8. **`hex()` só reconhece `#ffffff` e `#000000`** — qualquer outro valor retorna cinza.
   I18N: `make_dialog` tem `"Confirmar"`/`"Cancelar"` hardcoded num framework de
   alcance global.
9. **Não há batching de ícones nem LRU** — o README promete cache LRU; o código usa
   `HashMap` sem eviction (crescimento ilimitado por combinação de cor+tamanho).
10. **Faltam eventos essenciais**: `on_hover`, `on_double_click`, `on_right_click`,
    `on_context_menu`, arrasto com threshold, undo/redo, command palette.
11. **Não há docking real nem persistência de layout.** O editor é uma árvore de
    `split_view` fixa; nada sobrevive a um restart.
12. **Não há acessibilidade** além de Tab/Enter/Esc. Sem papéis, sem rótulos, sem
    `aria-live`, sem `prefers-reduced-motion`, sem contraste verificado.

---

## 1. Diagnóstico do estado atual

### 1.1 Mapa da implementação

| Camada | Artefato | LOC | Responsabilidade |
|:--|:--|--:|:--|
| Tipos | `packages/aipo-zoe/src/types.aipo` | 150 | `Rect`, `Color`, `Element`, sinais |
| Cor | `packages/aipo-zoe/src/color.aipo` | 65 | Normalização + paleta Catppuccin |
| **Tokens** | `packages/aipo-zoe/src/tokens.aipo` | 126 | Escala tipográfica/espaço — **não consumido** |
| Layout | `packages/aipo-zoe/src/leona.aipo` | 578 | Medição + flex + wrap + baseline |
| Reatividade | `packages/aipo-zoe/src/hooks.aipo` | 204 | `use_state`/`memo`/`effect`/`tween` |
| Componentes | `packages/aipo-zoe/src/components.aipo` | 1842 | 20+ componentes |
| Widgets | `packages/aipo-zoe/src/widgets.aipo` | 477 | `code_editor`, `node_graph`, modais |
| Render | `packages/aipo-zoe/src/renderer.aipo` | 209 | Recursão + dispatch de draw calls |
| App loop | `packages/aipo-zoe/src/app.aipo` | 504 | Hit-test, eventos, foco, teclado |
| Overlays | `packages/aipo-zoe/src/overlay.aipo` | 143 | Pilha modal, tooltip |
| Ícones | `packages/aipo-zoe/src/icons.aipo` | 102 | Registro SVG → textura |
| 3D | `packages/aipo-zoe/src/retro3d.aipo` | 490 | Malhas, câmera, gizmo |
| **Bridge GPU** | `crates/aipo-game-host/src/host_bridge.rs` | 1932 | 42 host natives + shader SDF |
| Exemplo | `packages/aipo-zoe/examples/editor.aipo` | 736 | Editor de game engine |

**Total**: ~7.567 linhas de Aipo + 1.932 de bridge Rust.

### 1.2 Pipeline de renderização atual

```
zoe.draw_ui()
  └─> app_render()                          app.aipo:486
       ├─> host_clear_background()          hardcoded 0.067,0.067,0.106
       └─> renderer.render_tree(root)       renderer.aipo:109
            └─> render_node(node)           renderer.aipo:7   ← RECURSÃO
                 ├─> host_draw_sdf_rect()    ← 1 DRAW CALL POR NÓDEO
                 ├─> host_draw_rect_lines()  ← anel de foco (2º draw call)
                 ├─> host_draw_text()        ← 1 draw call por rótulo
                 ├─> on_render()             ← viewport 2D/3D
                 ├─> on_custom_draw()        ← ícones, code_editor
                 └─> children...             ← sem display list, sem batch
```

**Consequências measures:**

- Sem display list: cada quadro reconstrói e re-emite toda a geometria.
- Sem batch por material: `set_uniform` × 5 por nóde (host_bridge.rs:630-642).
- Sem culling de CPU além do retângulo de clip.
- Sem cache de layout: `leona_compute_layout` roda sobre a árvore inteira sempre que
  `is_dirty()` (app.aipo:480-482).

### 1.3 Defeitos verificados no shader SDF

O fragment shader atual (host_bridge.rs:230-279) tem três problemas que o mercado
resolveu há anos:

```glsl
// ATUAL (host_bridge.rs:253-255)
float alpha = clamp(0.5 - dist, 0.0, 1.0);   // ← largura fixa de 1px

// ATUAL (host_bridge.rs:271-274)
float b_dist = dist + border_width;
float border_mask = clamp(0.5 - dist, 0.0, 1.0) - clamp(0.5 - b_dist, 0.0, 1.0);
col = mix(col, u_border_color, clamp(border_mask * u_border_color.a * 1.5, 0.0, 1.0));
```

**D1 — Antialiasing com largura fixa.** `clamp(0.5 - dist, 0, 1)` assume que 1
unidade de distância = 1 pixel. Isso é verdade apenas em escala 1:1. Sob
`camera_zoom`, sob `high_dpi`, ou sob qualquer transformação, a borda fica ora
grossa demais ora fina demais. A correção padrão da indústria é a **norma L2 do
gradiente** (não `fwidth`, que é ~√2× mais largo a 45° e engorda os cantos):

```glsl
float aa = max(length(vec2(dpdx(d), dpdy(d))), 0.5);
float inside = 1.0 - smoothstep(-aa, 0.0, d);
```

**D2 — Borda por `mix` sobre SDF único.** O padrão documentado pela própria
comunidade é: *"`d = sdRoundedBox(...); d + borderWidth` perde o termo
`min(max(q.x,q.y),0.0)`, então o raio interno fica errado"*. Além disso, `mix`
sobre um fill semi-transparente não faz composição correta — a saída precisa ser
**alpha pré-multiplicada** com `over`:

```glsl
float outA = ba + fa * (1.0 - ba);
vec3  outRGB = (border.rgb * ba + fill.rgb * fa * (1.0 - ba)) / max(outA, 1e-5);
```

**D3 — Zero sombra.** Nenhum `u_shadow_*` uniform existe. Sobreposições simulam
sombra com um retângulo preto deslocado (`renderer.aipo:131`), que é o oposto de
profundidade — produz uma borda dura, não um gradiente.

**D4 — Amostragem MSAA incorreta.** O vertex output não declara
`@interpolate(perspective, sample)`, então com MSAA ligado o shader roda uma vez
por *pixel*, não por *sample*, e o `smoothstep` resolve na resolução errada.

### 1.4 Defeitos numéricos verificados no retro3d

`retro3d.aipo:77-91` usa uma aproximação de Taylor de 5º ordem com 3 termos:

```text
fn math_sin(rad) { return x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0)) }
fn math_cos(rad) { return math_sin(rad + 1.5707963) }
```

Verificação numérica (Python, reproduzindo exatamente a fórmula):

```
sin(π/2): obtido 1.004525  esperado 1.000000  erro +0.45%
cos(0):    obtido 1.004525  esperado 1.000000  erro +0.45%
sin(1.5):  obtido 1.000781  esperado 0.997495  erro +0.33%
cos(0.6):  obtido 0.867580  esperado 0.825336  erro +4.22%   ← INACEITÁVEL
```

O erro de 4,22% em `cos(0.6)` — que é literalmente o `yaw` default da câmera
(`camera_3d(yaw = 0.6, ...)`) — distorce a projeção de forma visível. Isso
corrompe a perception de profundidade do viewport 3D.

**Correção:** implementar `sin`/`cos`/`sqrt`/`atan2`/`pow` como **host natives** em
Rust (f64 nativo). O host já tem 42 natives; adicionar 5 é trivial e remove a
classe inteira de bugs de precisão. Custo: uma chamada FFI por uso, o que é
irrelevante comparado ao erro atual.

### 1.5 Defeitos de API e componentes

| # | Defeito | Local | Impacto |
|:--|:--|:--|:--|
| C1 | `font_weight` nunca lido | `renderer.aipo` não referencia | Toda hierarquia tipográfica é fictícia |
| C2 | `hex()` só aceita 2 valores | `color.aipo:20-28` | Qualquer cor hex retorna `(100,100,100)` |
| C3 | I18N hardcoded PT-BR | `components.aipo:1321-1322` | Framework global com strings fixas |
| C4 | `slider` só tem `on_click`, sem `on_drag` | `components.aipo:119-151` | Não se arrasta o slider |
| C5 | `spacer(flex)` ignora o argumento | `lib.aipo:84-90` | `flex: 3.0` não faz nada |
| C6 | Scroll sem clamp superior | `components.aipo:565` | Rola para o infinito |
| C7 | Thumb de scroll com altura fixa 40px | `components.aipo:638` | Não reflete proporção de conteúdo |
| C8 | Cache de ícones = `HashMap` sem eviction | `host_bridge.rs:87` | README promete LRU; memória cresce sem limite |
| C9 | `tree_view` chevron é texto `"v "`/`"> "` | `components.aipo:1432` | ASCII onde deveria haver ícone |
| C10 | Editor mostra telemetria falsa | `editor.aipo:709-710` | `"FPS: 60"` e `"Leona Layout: OK"` hardcoded |
| C11 | Botão "Delete Entity" sem ação | `editor.aipo:149` | `fn(_) {}` — affordance quebrada |
| C12 | Cores de estado não existem | `components.aipo` inteiro | Hover/pressed derivam de `+0.12` uniforme |

### 1.6 O que está certo e deve ser preservado

Não é tudo defeito. Estes são **decisões sólidas** que valem manter:

- **Framework 100% em Aipo puro.** Zero dependência de UI do SO. Diferencial real.
- **Leona com alinhamento por baseline real** usando métricas de fonte — raro.
- **Métricas de fonte em runtime** (`host_font_metrics`) — base correta para
  tipografia profissional, desde que `font_weight` passe a ser respeitado.
- **Overlays pós-clipping** com scrim — correto, só precisa de sombra real.
- **`virtual_list` com overscan** — o algoritmo está certo.
- **`scrubber_input` com modificadores** (Shift 0.1x, Ctrl 10x) — casa com Blender.
- **Otimização de stack frames** (70 → 18 slots) — trabalho sério de VM.

---

## 2. Benchmark de mercado

### 2.1 Matriz comparativa

| Capacidade | **Zoe hoje** | egui 0.36 | Slint 1.16 | Godot 4.7 | Blender 5.2 | Flutter/Impeller |
|:--|:--|:--|:--|:--|:--|:--|
| Modelo | Retained-ish, full rebuild | Imediato | Declarativo+retained | Retained | Imediato | Retained |
| Batching | ❌ 1 call/nóde | ✅ por clip+texture | ✅ | ✅ | ✅ | ✅ |
| Rounded rect SDF | ⚠️ AA fixo | ✅ tessellation | ✅ FemtoVG/Skia | ✅ StyleBox | ✅ GPU_draw | ✅ Impeller |
| Sombra | ❌ | ⚠️ 4-tap offset | ✅ `drop-shadow-*` | ✅ StyleBox | ✅ | ✅ ImageFilter |
| Blur/backdrop | ❌ | ❌ | ⚠️ | ❌ | ❌ | ✅ dual-Kawase |
| Tipografia subpixel | ❌ | ✅ skrifa+vello | ✅ fontique+parley | ✅ | ✅ | ✅ |
| Fontes variáveis | ❌ | ✅ | ✅ | ⚠️ | ⚠️ | ✅ |
| Tema runtime | ❌ hardcoded | ✅ | ✅ | ✅ 6 presets | ✅ | ✅ |
| Densidade | ❌ | ⚠️ | ⚠️ | ✅ 3 presets | ✅ | ✅ |
| Layout incremental | ❌ | — | ✅ | ✅ | ✅ | ✅ |
| Estado de widget | ❌ | ✅ | ✅ `PseudoStates` | ✅ | ✅ | ✅ |
| Ícones | ⚠️ 1 tex/cor | ✅ SDF atlas | ✅ | ✅ | ✅ | ✅ |
| Undo/redo | ❌ | — | — | ✅ | ✅+histórico | — |
| Command palette | ❌ | — | — | ✅ F4 | ✅ | — |
| Docking persistente | ❌ | — | ❌ | ✅ 9 slots | ✅ Areas | — |
| Acessibilidade | ❌ Tab apenas | ✅ AccessKit | ✅ AccessKit | ✅ | ⚠️ | ✅ Semantics |
| Text wrap | ❌ | ✅ | ✅ | ✅ | ✅ | ✅ |
| DevTools inspector | ✅ F12 | ✅ | — | ⚠️ | ✅ | ✅ |

**Leitura**: o Zoe está na categoria "framework de layout competente com protótipo de
renderização" e precisa chegar a "renderer de produção + chrome de editor". O
caminho é conhecido e o mercado já o percorreu.

### 2.2 As cinco alavancas de maior retorno

Extraídas da pesquisa de mercado, ordenadas por impacto na percepção de qualidade:

1. **Batch real de draw calls** com instancing por material. De 300+ calls para
   3-5. É a diferença entre "roda" e "não roda".
2. **AA correto no shader** (norma L2 + MSAA sample-rate). A borda é o que o olho
   judges primeiro; 1px de erro em 300 retângulos é imediatamente visível.
3. **Tipografia de verdade** — shaping, posicionamento subpixel, peso real, MSDF.
   Texto domina a percepção de polimento; engines que erram aqui nunca parecem
   "profissionais" (Slint corrigiu exatamente isso e o commit está documentado).
4. **Unificar rect/borda/sombra/foco numa passada só.** Menos passes, mais
   controle, e a sombra fica gratuita.
5. **Tokens semânticos OKLCH com tema em runtime.** Sem isso, "personalizável" é
   apenas uma lista de props.

---

## 3. Especificação do Design System — "Aipo Design Language"

### 3.1 Modelo de cor: OKLCH semântico

Hoje: 26 cores Catppuccin hardcoded como constantes de módulo. Proposta: escala
semântica em OKLCH com **12 passos de papel fixo** (modelo Radix, o mais maduro),
gerada a partir de um único acento configurável.

```text
# theme.aipo — novo módulo

struct Palette {
    # 12 passos por escala, papéis fixos (modelo Radix)
    app_bg, app_bg_subtle        # 1, 2
    element_bg, element_hover, element_active   # 3, 4, 5
    border_subtle, border, border_hover          # 6, 7, 8
    solid, solid_hover           # 9, 10
    text_muted, text             # 11, 12  ← garantem APCA Lc 60 / Lc 90
    accent, accent_hover, accent_fg
    success, warning, danger, info
}

struct Theme {
    palette
    radius_xs, radius_sm, radius_md, radius_lg, radius_full
    space_xxs, space_xs, space_sm, space_md, space_lg, space_xl
    font_micro, font_caption, font_sm, font_base, font_lg, font_title
    line_height_tight, line_height_normal
    density        # compact | default | spacious
    motion_scale   # 1.0 | 0.0 (reduced motion)
    name
}
```

**Contrato de contraste garantido estruturalmente** (não por revisão manual):
passos 11 e 12 de cada escala atingem APCA Lc 60 e Lc 90 sobre o passo 2 da mesma
escala. Isso vira um **teste automatizado**, não uma revisão.

**Presets iniciais**: `aipo_dark` (padrão, derivado de Catppuccin Mocha), `aipo_light`
(Latte), `midnight` (quase-preto OLED), `solarized`. Troca em runtime.

**Seed de marca**: `Theme::from_accent(oklch_l, oklch_c, oklch_h)` gera a escala
inteira. Isso é o que torna a UI *personalizável de verdade*.

### 3.2 Estados de widget — o que falta hoje

O renderer atual faz isto (renderer.aipo:28-32):

```text
if (node.is_hovered or node.is_active) and (...) {
    bg_r = if bg_r + 0.12 > 1.0 then 1.0 else bg_r + 0.12   # ← "+0.12" uniforme
    bg_g = ...; bg_b = ...
}
```

Problemas: (a) um único estado para hover e active, (b) incremento uniforme
ignora croma, (c) não há estado disabled nem selected.

Proposta — tokens por estado, herdando o modelo Godot (rampa monotônica, não swap):

| Estado | Fundo | Borda | Texto | Cursor |
|:--|:--|:--|:--|:--|
| `rest` | `element_bg` | `border_subtle` | `text` | `default` |
| `hover` | `element_hover` | `border_hover` | `text` | `pointer` |
| `active` | `element_active` | `border_hover` | `text` | `pointer` |
| `selected` | `accent` @ 0.16 | `accent` @ 0.45 | `accent` | `pointer` |
| `focused` | idem `rest` | **anel de foco bicolor** 2px | idem | — |
| `disabled` | `element_bg` @ 0.5 | `border_subtle` @ 0.4 | `text_muted` | `not-allowed` |

**Anel de foco bicolor** (critério WCAG 2.4.13 / técnica C40): um anel
`accent` com 1px externo `surface` garante contraste contra *qualquer* fundo.
Hoje o anel é um único `host_draw_rect_lines` azul de 2px (renderer.aipo:52) —
falha em fundo escuro **e** em fundo claro saturado.

**Ícones seguem a mesma rampa de alpha**: normal 0.85, secundário 0.6, desabilitado
0.35, pressionado = acento saturado (`accent * 1.15`).

### 3.3 Tipografia

Adotar a pilha de texto que a pesquisa identificou como a mais madura
(Linebender): **fontdue/skrifa para shaping + contorno**, com:

- **Posicionamento subpixel de glifos** — sem isso, advances fracionários acumulam e
  linhas longas derivam. É o commit que a Slint acabou de fazer.
- **Peso real**: `font_weight` mapeia para eixos `wght` da Inter Variable.
  Inter já está embutida (860KB) como variável — o asset suporta, o código ignora.
- **Eixos**: `font_size`, `line_height`, `letter_spacing`, `font_variation`.
- **Alinhamento por baseline já existe** e está correto — manter.

Tokens: `--font-mono` para código (necessário: o code editor usa a fonte UI).

### 3.4 Movimento: molas, não curvas

`use_tween` atual (hooks.aipo:154-179) tem 4 curvas fixas e repete `mark_dirty()` a
cada quadro — o que força **rebuild completo da árvore por quadro durante uma
animação**. Isso é a causa provável de hitching em qualquer tween hoje.

Proposta: **motor de mola amortecida** com a mesma matemática do Flutter
`SpringDescription`, exposto como `use_spring(target)`:

```
stiffness = 4π² / duration²
damping   = dampingRatio · 2√(mass · stiffness)
dampingRatio = 1.0            → criticamente amortecida (efeitos: cor, opacidade)
dampingRatio = 0.6 … 0.9      → subamortecida (espacial: posição, escala)
```

Tokens M3 Expressive como referência:

| Token | ζ (damping) | k (stiffness) |
|:--|--:|--:|
| `effects.fast` | 1.0 | 3800 |
| `effects.default` | 1.0 | 1600 |
| `spatial.default` | 0.9 | 700 |
| `spatial.slow` | 0.9 | 300 |

`motion_scale = 0.0` implementa `prefers-reduced-motion`.

### 3.5 Densidade

Três presets derivados de dois inteiros (modelo Godot, o mais limpo):

| Preset | `base_spacing` | `extra_spacing` | altura de linha |
|:--|--:|--:|--:|
| `compact` | 2 | 2 | 22px |
| `default` | 4 | 0 | 26px |
| `spacious` | 6 | 2 | 30px |

Mais `ui_scale` (0.75 → 1.5) separado de densidade, para respeitar
preferências de display do SO.

---

## 4. Estratégia de implementação

Dez initiatives, ordenadas por dependência. Cada uma é executável de forma
independente e verável.

### F1 — Batch de draw calls (instancing por material) 🔴 Crítico

**Problema**: `host_draw_sdf_rect` emite 1 draw call + 5 `set_uniform` por nóde.
Num editor com 400 nósdes isso são 400 calls e 2000 uniform writes por quadro.

**Solução**: mover de uniforms para **atributos por instância** num VBO único,
com um único draw call por lote. Requer sair da API `Material` do macroquad e usar
`miniquad` diretamente.

```rust
// crates/aipo-game-host/src/gpu/instance_batch.rs (novo)

/// Layout de atributos por instância: 9 × vec4 = 36 floats = 144 bytes.
/// Espelha exatamente os atributos `i_*` declarados em `sdf.frag` (GLSL ES 100).
/// Este é um contrato: mudar aqui exige mudar o shader no mesmo commit,
/// e o teste `test_sdf_instance_layout_parity` falha se divergirem.
pub const STRIDE_F32: usize = 36;

/// Um quad unitário compartilhado por todas as instâncias (loc 0).
const UNIT_QUAD: [f32; 8] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];

/// `max_attributes_per_vertex` do miniquad é 16; 1 (quad) + 9 (instância)
/// = 10, com folga.
pub const INSTANCE_ATTRS: [(usize, usize, usize); 9] = [
    // (location, offset_em_f32, buffer)
    (1,  0, 1), // rect         (x, y, w, h)   — rect de PAINT
    (2,  4, 1), // fill         (r, g, b, a)
    (3,  8, 1), // stroke       (r, g, b, a)
    (4, 12, 1), // params       (stroke_w, max_radius, elevation, focus_w)
    (5, 16, 1), // inner_rect   (x, y, w, h)   — rect de LAYOUT
    (6, 20, 1), // focus_color  (r, g, b, a)
    (7, 24, 1), // radii        (tl, tr, br, bl)
    (8, 28, 1), // shadow1      (dy, blur, spread, alpha)
    (9, 32, 1), // shadow2      (dy, blur, spread, alpha)
];

pub struct SdfInstance {
    pub rect: [f32; 4],
    pub fill: [f32; 4],
    pub stroke: [f32; 4],
    pub params: [f32; 4],
    pub inner_rect: [f32; 4],
    pub focus_color: [f32; 4],
    pub radii: [f32; 4],
    pub shadow1: [f32; 4],
    pub shadow2: [f32; 4],
}

pub struct InstanceBatch {
    quad_vbo: Buffer,
    instance_vbo: Buffer,
    capacity: usize,
    count: usize,
}

impl InstanceBatch {
    /// Dobra a capacidade preservando o conteúdo. Chamado em `push`
    /// quando `count == capacity`; amortizado por duplicação.
    fn grow(&mut self) { /* ... */ }

    pub fn push(&mut self, inst: &SdfInstance) {
        if self.count == self.capacity { self.grow(); }
        // f32::write 直接 no tail do VBO, sem alocação intermediária
    }

    /// Emite um único `draw_elements_instanced` para o lote inteiro.
    pub fn flush(&mut self, pass: &mut dyn BatchRender) {
        if self.count == 0 { return; }
        pass.draw_instanced(self.quad_vbo, self.instance_vbo, self.count);
        self.count = 0;
    }
}
```

**Decisões de layout (fechadas, não em aberto):**

- **9 `vec4` = 36 floats.** Duas camadas de sombra são atributos separados
  (loc 8 e 9) em vez de um `vec4` comprimido. Custa 16 bytes por instância;
  para 4000 instâncias são 64KB, irrelevante. Em troca o shader fica legível e
  o `shadow_layer_alpha()` recebe um `vec4` limpo.
- **`inner_rect` é separado de `rect`** porque o anel de foco pode transbordar
  (*paint overflow*): o layout não muda, mas a área pintada sim. Sem essa
  separação, o foco estoura a caixa de layout e o culling CPU corta o anel.
- **`rect` de paint vs `rect` de layout** também resolve o caso de sombra:
  a sombra é desenhada a partir do rect de layout deslocado por `dy`, com
  spread — se usássemos o rect de paint, a sombra cresceria junto.
- **`elevation` como float contínuo em `params.z`**, não como índice de
  âncora. O shader interpola entre as âncoras `SHADOW_ANCHORS`, então elevar e
  deselevar uma superfície é contínuo. A tabela de âncoras vive em Rust e é
  **verificada contra o GLSL por teste** (ver F2).

**Fronteira**: `InstanceBatch` é um tipo do host Rust. Nenhuma mudança na
linguagem Aipo. O `renderer.aipo` apenas chama `host_sdf_begin()` /
`host_sdf_push(...)` / `host_sdf_flush()`.

**Migração sem quebrar o fallback headless**: `host_sdf_flush()` detecta ausência
de contexto GL e devolve a lista de instâncias para o rasterizador de software
existente (`safe_draw_round_rect` + `safe_draw_round_rect_lines`). Os 13 testes
atuais continuam passando sem GPU — é o mesmo padrão que `safe_draw_sdf_rect` já
usa hoje (`host_bridge.rs:653-658`).

**Ganho esperado**: 400 draw calls → 1-3. Ordem de grandeza.

**Status e viabilidade verificada (2026-09-28):**

Uma análise anterior desta iniciativa concluiu que o F1 era **bloqueado** pelo
macroquad, porque `Context::camera_matrix` é privado e um pipeline miniquad cru
não poderia reusar a projeção da câmera. **Essa conclusão está incorreta** e foi
verificada contra o código-fonte de `macroquad 0.4.16`:

| Necessidade | Primitiva real | Onde |
|:--|:--|:--|
| Desenhar com miniquad cru no meio de um quadro macroquad | `get_internal_gl()` (público) devolve `quad_gl` + `quad_context`, e `InternalGlContext::flush()` é documentado como "útil para combinar o desenho do macroquad com chamadas miniquad/OpenGL cruas" | `macroquad/src/window.rs` |
| Obter a projeção corrente, incluindo a câmera | `QuadGl::get_projection_matrix()` — existe explicitamente para plugins de terceiros | `macroquad/src/quad_gl.rs` |
| Atributos por instância | `miniquad::VertexStep::PerInstance` | backend GL/WebGL |
| Buffer de instâncias e índice por instância | `draw_elements_instanced` / `draw(base, count, instances)` | `miniquad` |

O que **não** é possível é fazer batching mantendo a API `Material`: em
`QuadGl::set_uniform` e em `QuadGl::pipeline` o macroquad marca
`break_batching = true`, e `gl_use_material()` é um `pipeline()` — logo **cada
`set_uniform` e cada troca de material força um novo draw call**. Como a
geometria de cada quad viaja em uniforms (`u_quad_size`, `u_box_half`) e o
formato de vértice do macroquad é fixo (`position` vec2 + `texcoord` vec2 +
`color0` vec4, 32 bytes por vértice), os 36 floats por instância **não cabem em
atributos**. O F1 integral exige mesmo sair da API `Material` (ADP-014), como o
dossiê já previa.

**O que já está entregue (M15-A)**, sem trocar o caminho de desenho:

- culling puro de quads que não pintam (`sdf_cull_reason`: `OffScreen`,
  `ZeroArea`, `Invisible`), com 10 testes;
- contadores públicos `sdf_draws_issued` / `sdf_draws_culled`, sem os quais o
  alvo "≤ 8 draw calls por quadro" não é mensurável;
- elisão de uploads de uniform por `SdfUniformState`, invalidado no
  `init_zoe_shaders()`;
- o anel de foco no fragment shader, fechando a lacuna deixada pelo M14.

**O que falta (M15-B)** — e as duas barreiras que precisam de decisão antes do
código:

1. **`#![forbid(unsafe_code)]`**: declarado em `crates/aipo-game-host/src/lib.rs`
   **e** em `main.rs`, e `get_internal_gl()` é `unsafe`. `forbid` não pode ser
   afrouxado por `#[allow]` em nenhum módulo filho — nem em `host_bridge.rs`,
   que também repete o atributo. O caminho exige trocar para
   `#![deny(unsafe_code)]` com **um único** `#[allow(unsafe_code)]` em módulo
   isolado e auditado, ou extrair o renderizador para um crate próprio. É uma
   decisão de postura de segurança, portanto cabe como ADP antes do código.
2. **Verificação**: nenhum teste headless prova que o lote é rasterizado na
   ordem, no clip e no viewport corretos (o scissor do viewport passa por
   `Camera2D` + `apply_scissor_rect`, e o material é trocado no meio do quadro).
   M15-B só pode ser declarada concluída com a GUI rodada numa máquina com
   display; sem isso, a troca do caminho de desenho é uma regressão não
   verificada em cima de um renderizador que hoje funciona.

O restante é implementação: pipeline instanciado, VBO de instâncias com o layout
de 9 × vec4 espelhado no shader, e descarga por lote.

---

### F2 — Shader SDF v2 (AA, sombra, borda correta, MSAA) 🔴 Crítico

Reescrever o fragment shader com as correções da pesquisa. **Mantido em GLSL ES
100** (o que o `miniquad` já consome hoje) — portar para WGSL exigiria um caminho
de compilação novo sem ganho funcional.

```glsl
// crates/aipo-game-host/src/gpu/sdf.frag  (GLSL ES 100)

// --- Atributos por instância (loc 1..9). Espelha `INSTANCE_ATTRS` em Rust. ---
attribute vec4 i_rect;         // xy = topo-esq em px, zw = tamanho
attribute vec4 i_fill;         // rgba
attribute vec4 i_stroke;       // rgba
attribute vec4 i_params;       // x=stroke_w  y=max_radius  z=elevation  w=focus_w
attribute vec4 i_inner_rect;   // xy = topo-esq, zw = tamanho  (LAYOUT, não paint)
attribute vec4 i_focus_color;  // rgba
attribute vec4 i_radii;        // tl, tr, br, bl
attribute vec4 i_shadow1;      // dy, blur, spread, alpha
attribute vec4 i_shadow2;

varying lowp vec2 uv;
varying lowp vec4 color;
varying highp vec2 pos_px;     // ← D4: vary em COORDENADAS DE PIXEL, não uv

uniform mat4 Model;
uniform mat4 Projection;

// SDF com raios PER-CORNER (habilita "squircles" assimétricos)
float sd_rounded_box(vec2 p, vec2 b, vec4 r) {
    float r_top = (p.x > 0.0) ? r.y : r.x;   // tr : tl
    float r_bot = (p.x > 0.0) ? r.z : r.w;   // br : bl
    float rd    = (p.y < 0.0) ? r_top : r_bot;
    vec2  q = abs(p) - b + vec2(rd);
    return min(max(q.x, q.y), 0.0) + length(max(q, 0.0)) - rd;
}

float shadow_alpha(vec2 p, vec2 half_size, vec4 radii, vec4 layer) {
    if (layer.w <= 0.0) { return 0.0; }
    float d = sd_rounded_box(p - vec2(0.0, layer.x), half_size, radii) - layer.z;
    float blur = max(layer.y, 0.5);
    return (1.0 - smoothstep(-blur, blur, d)) * layer.w;
}

void main() {
    vec2 paint_size  = i_rect.zw;
    vec2 half_size   = i_inner_rect.zw * 0.5;
    vec2 inner_center = i_inner_rect.xy + half_size;
    vec2 local = pos_px - inner_center;

    float max_r  = min(half_size.x, half_size.y);
    vec4  radii  = clamp(i_radii, vec4(0.0), vec4(max_r));
    float d      = sd_rounded_box(local, half_size, radii);

    // D1 FIX: norma L2 do gradiente. `fwidth` é a norma L1 (~√2× mais larga
    // a 45°) e engorda visivelmente os cantos arredondados.
    float aa = max(length(vec2(dFdx(d), dFdy(d))), 0.5);
    float inside = 1.0 - smoothstep(-aa, 0.0, d);

    vec4 col = color;

    // --- Sombra: 2 camadas, compostas na ordem do CSS ---
    if (i_params.z > 0.0) {
        float a1 = shadow_alpha(local, half_size, radii, i_shadow1);
        float a2 = shadow_alpha(local, half_size, radii, i_shadow2);
        float a_css = 1.0 - (1.0 - a1) * (1.0 - a2);
        // Compensação de gamma: shadow preto em sRGB CODIFICADO precisa de 2.2,
        // senão um alpha de 0.08 renderiza com ~metade da escuridão pretendida.
        float sa = 1.0 - pow(1.0 - a_css, 2.2);
        col = vec4(0.0, 0.0, 0.0, sa);
    }

    // --- Fill sobre a sombra (mix + max: nunca sobrescrever a sombra) ---
    if (i_fill.a > 0.0) {
        float fa = i_fill.a * inside;
        col = vec4(mix(col.rgb, i_fill.rgb, fa), max(col.a, fa));
    }

    // D2 FIX: SDFs independentes para fill e borda + alpha pré-multiplicada.
    // `d + stroke_w` perderia o termo min(max(q.x,q.y),0.0) e daria raio interno errado.
    float stroke_w = i_params.x;
    if (stroke_w > 0.0 && i_stroke.a > 0.0) {
        float stroke_d = abs(d) - stroke_w * 0.5;
        float sa = (1.0 - smoothstep(-aa, aa, stroke_d)) * i_stroke.a;
        float out_a = sa + inside * (1.0 - sa);
        vec3  out_rgb = (out_a > 1e-5)
            ? (i_stroke.rgb * sa + i_fill.rgb * inside * (1.0 - sa)) / out_a
            : i_fill.rgb;
        col = vec4(out_rgb, out_a);
    }

    // D4: anel de foco bicolor. fw > 0 desenha FORA do limite (halo de
    // paint_overflow); fw < 0 desenha DENTRO, para linhas densas sem padding.
    float fw = i_params.w;
    if (fw != 0.0 && i_focus_color.a > 0.0) {
        float w = abs(fw);
        float c = (fw > 0.0) ? (w * 0.5) : (-w * 0.5);
        float ring_d = abs(d - c) - w * 0.5;
        float ra = (1.0 - smoothstep(-aa, aa, ring_d)) * i_focus_color.a;
        col = vec4(mix(col.rgb, i_focus_color.rgb, ra), max(col.a, ra));
    }

    if (col.a <= 0.0) { discard; }
    gl_FragColor = col;
}
```

**D4 — MSAA.** O vertex shader deve emitir `pos_px` como `varying` de alta
precisão em coordenadas de pixel absolutas, e o pipeline deve declarar
`sample_count > 1` com `miniquad`/OpenGL. Com MSAA ligado, `dFdx`/`dFdy` são
avaliados por amostra e o `smoothstep` resolve na resolução correta — é assim que
se obtém SDF **com** MSAA, não em vez dele. Se `sample_count == 1`, o mesmo shader
funciona: `dFdx` ainda fornece a largura de pixel correta.

**Tabelas de elevação** — âncoras de sombra, derivadas de Tailwind e espelhadas
entre Rust e GLSL:

```rust
// crates/aipo-game-host/src/gpu/elevation.rs
/// (dy, blur, spread, alpha) por nível de elevação.
/// DEVE espelhar literalmente as âncoras em `sdf.frag`.
pub const SHADOW_ANCHORS: [(f32, f32, f32, f32); 5] = [
    (  2.0,  3.0,  0.0, 0.05),  // 0 = shadow-xs
    (  4.0,  6.0, -1.0, 0.08),  // 1 = shadow-sm
    ( 12.0, 16.0, -3.0, 0.10),  // 2 = shadow-md
    ( 24.0, 32.0, -6.0, 0.14),  // 3 = shadow-lg
    ( 40.0, 48.0, -9.0, 0.18),  // 4 = shadow-xl
];

/// Interpola linearmente entre âncoras. `elevation` é contínuo.
/// Acima do último nível, escala a geometria do xl proporcionalmente.
pub fn shadow_layers(elevation: f32) -> ([f32; 4], [f32; 4]) { /* ... */ }
```

**Testes de paridade obrigatórios** (impedem que Rust e GLSL divirjam):

| Teste | O que falha |
|:--|:--|
| `test_sdf_instance_layout_parity` | `STRIDE_F32` e `INSTANCE_ATTRS` não espelham `InstanceInput` |
| `test_sdf_elevation_anchor_parity` | cada `vec4(dy, blur, spread, alpha)` de `SHADOW_ANCHORS` aparece literalmente no `sdf.frag` |
| `test_sdf_shader_compiles_headless` | shader compila em todos os backends suportados |

O terceiro cobre as variações de backend: `miniquad` transpila GLSL 100 para
HLSL/MSL conforme a plataforma, então o mesmo fonte precisa compilar em Vulkan,
Metal, D3D e WebGL2. `test_sdf_shader_compiles_headless` roda a compilação para
cada backend disponível no ambiente de CI.

**Decisão fechada: GLSL ES 100**, o que o host já consome hoje
(`ShaderSource::Glsl` em `host_bridge.rs:291`). Nenhuma dependência nova.

---

### F3 — Tipografia de produção 🔴 Crítico

**Problema**: `host_draw_text` delega ao `draw_text` do macroquad — sem shaping,
sem posicionamento subpixel, sem peso. E `font_weight` é no-op.

**Solução em três partes**:

1. **Host native de texto em lote** — `host_text_begin(font_id)`,
   `host_text_push(glyph_run, x, y_baseline, size, color)`,
   `host_text_flush()`. O Rust faz shaping uma vez e emite quads de glifo.

2. **Shaping com subpixel** — `fontdue` já está integrado. Habilitar
   `horizontal_kerning` (já existe em `host_measure_text:1320`) **e**
   posicionamento subpixel (arredondar posições de glifo para ¼ de pixel, não para
   pixel inteiro). Sem isso, linhas longas derivam visivelmente.

3. **Peso real via eixo variável** — `host_font_metrics(size, weight)` e
   `host_measure_text(text, size, weight)` com o eixo `wght` da Inter Variable.
   `font_weight: "bold"` passa a significar 700.

**Ganho**: é o que separa "engine de jogo" de "editor profissional". A pesquisa é
explícita: engines que erram texto nunca parecem profissionais.

---

### F4 — Layout memoizado e reatividade de granularidade fina 🟠 Alto

**Problema**: `rebuild_tree()` (app.aipo:63) descarta e reconstrói tudo, e
`leona_compute_layout` percorre a árvore inteira. `use_memo` existe mas
`reset_hooks()` zera os índices a cada rebuild, então o memo nunca sobrevive entre
quadros — é literalmente inútil no estado atual.

**Solução**:

1. **Cache de layout por nóde** — guardar `(avail_w, avail_h, intrinsic)`; se
   nenhum mudou, pular a recursão. Leona é determinístico, então isso é seguro.
2. **Identidade de nóde estável** — `Element.id` já existe; propagá-lo para os
   descendentes permite reconciliação.
3. **Reconciliação em vez de descarte** — `rebuild` reusa nósdes cujo
   `id` + props não mudaram. `use_memo` passa a valer.
4. **Separar layout de paint** — `is_dirty_layout` e `is_dirty_paint` como
   flags distintas. Um hover só repinta; não re-roda Leona.
5. **Memo de `on_custom_draw`** — o `code_editor` e o `node_graph` re-tokenizam o
   arquivo inteiro a cada quadro (widgets.aipo:227). Cachear por hash de conteúdo.

**Contrato a adicionar ao `Element`**:
```text
struct Element {
    id, tag, props, children
    var layout          # Rect
    var layout_key      # String   ← chave do cache
    var is_hovered, is_active, is_focused
    var paint_only      # Bool     ← sujo só para repintar
}
```

---

### F5 — Tema em runtime e Design Tokens consumidos 🟠 Alto

**Problema verificado**: `tokens.aipo` exporta 126 linhas de tokens e
**nenhum componente os usa**. Todos usam `color_mod.surface_1` diretamente.

**Solução**:

1. `theme.aipo` com `Palette` + `Theme` (seção 3.1).
2. `zoe.set_theme(theme)` / `zoe.get_theme()`.
3. **Troca em runtime sem rebuild completo**: o renderer lê o tema atual por nóde;
   trocar o tema marca só `is_dirty_paint`.
4. `zoe.theme_dark()`, `zoe.theme_light()`, `zoe.theme_from_accent(l, c, h)`.
5. **Teste de contraste automatizado** — percorrer todos os pares
   (fundo, texto) e (superfície, borda) e falhar se APCA Lc < 60 / < 90.
6. `density` e `ui_scale` como tokens, com `host_ui_scale()` nativo para que o
   Rust aplique a escala aos draw calls (o Aipo não deveria multiplicar cada
   coordenada à mão).

---

### F6 — Componentes faltantes e correções 🟠 Alto

**Correções dos defeitos C1-C12** (tabela da seção 1.5).

**Componentes novos, em ordem de valor para um editor**:

| Componente | Por quê | Referência |
|:--|:--|:--|
| `command_palette` | Entrada de chave para tudo | VS Code `Ctrl+Shift+P` |
| `tooltip` como componente | hoje só existe no renderer | — |
| `context_menu` | clique direito | Godot/Blender |
| `menu_bar` + `menu_item` | navegação por teclado | — |
| `select` (multiselecionável) | `dropdown_select` é single | — |
| `checkbox` | falta; `switch` não substitui | — |
| `tabs` com `close` e `dirty` | `tab_view` não tem estado sujo | VS Code `tab.activeModifiedBorder` |
| `panel` / `dock` | chrome de editor | Godot 9 slots |
| `splitter` com snap | hoje não há snap em `split_view` | Blender 0.49999 |
| `tooltip` com atalho | *"tooltips devem conter o atalho"* | HIG |
| `empty_state` | painel vazio | Zed |
| `toast` / `notification` | feedback assíncrono | — |
| `keybinding` / `shortcut` | texto + tecla | — |
| `search_input` | filtro com destaque | Godot `listFilterWidget` |
| `graph` improvements | pan/zoom, seleção múltipla, snap | Blender |
| `asset_grid` | grade de thumbnails | Blender Asset Shelf |

**Texto**: `text_wrap`, `text_align` (`start`/`center`/`end`/`justify`),
`text_ellipsis`, `max_lines`, `letter_spacing`. Nenhum existe hoje.

---

### F7 — Eventos e interação de nível profissional 🟠 Alto

**Problema**: `on_click` dispara no **press** do mouse (app.aipo:189), não no
release. Isso significa que arrastar para fora do botão ainda o aciona — um bug
que qualquer usuário percebe. Também não há threshold de arrasto.

**Solução**:

1. **Separar press/release.** `on_click` só dispara se o release ocorrer dentro
   dos bounds *e* o movimento total ficar abaixo de `drag_threshold` (4px).
2. `on_hover_in` / `on_hover_out` — programáticos, não só visuais.
3. `on_double_click` com detecção de timing + raio.
4. `on_right_click` → abre `context_menu`.
5. **Roving tabindex** para composites (tree, tabs, toolbar) em vez de Tab
   atravessando cada item. É o padrão ARIA e evita Tab-skipped de 300 nósdes.
6. **Navegação espacial** com setas em `tree_view` e `node_graph`.
7. `on_scroll` com `ScrollDelta` tipado (pixels / linhas / páginas).
8. **Undo/redo** com coalescing: um arrasto de slider = **uma** entrada de undo,
   pushada no release (nunca por amostra — a documentação do Blender é explícita
   sobre isso).

---

### F8 — Chrome de editor 🟡 Médio

Transforme `editor.aipo` de demonstração em **produto**:

1. **Docking real** — 8 slots + bottom + floating, com `layout_key` estável
   (nunca key no título, o bug que a documentação do Godot registra).
2. **Persistência** — `zoe.layout_save(name)` / `load(name)`, serializado como
   `SerializedPaneGroup` no modelo do Zed. Debounce de 200ms.
3. **Command palette** com busca fuzzy e atalhos.
4. **Barra de status real** — usar `host_frame_time()` de verdade. Remover
   `"FPS: 60"` hardcoded (C10).
5. **Painéis com busca** — filtro de hierarquia e de propriedades, com
   destaque de correspondência *in-place* (o padrão `list.filterMatchBackground`
   do VS Code, melhor que um painel de resultados separado).
6. **Inspeção com drag-scrub real** — `scrubber_input` já tem Shift/Ctrl; falta
   reset, expressões (`=`, `pi`, `sin`) e pin de propriedade.
7. **Árvore com linhas de relacionamento** e sticky scroll.
8. **Asset shelf** — a pesquisa cita: *"um asset shelf vazio no primeiro run é a
   forma mais comum de um editor parecer vazio"*.
9. **Undo history** navegável, não binário.

---

### F9 — Viewport 2D/3D de produção 🟡 Médio

1. **Corrigir `math_sin`/`math_cos`** via host natives em f64 (seção 1.4).
2. **Z-buffer** — o algoritmo do Pintor (ordenação por profundidade) falha com
   geometria interpenetrante. Um depth buffer de 16 bits por quadro custa quase
   nada e resolve.
3. **MSAA** no viewport.
4. **Gizmo completo** — rotação (anéis) e escala (alças de caixa), além de
   translação. Hoy em dia só há translação.
5. **Snapping** — grade e incremento, com Ctrl para snap e Shift para
   precisão, reversível *durante* o gesto.
6. **Seleção por marquee** (arrastar laço) e clique para ciclar profundidade.
7. **Grade adaptativa** — duas camadas (menor/maior) com fade por distância,
   como o Blender.
8. **Eixos no canto do viewport**, com widget de câmera interativo, e preview PiP
   da câmera selecionada.
9. **Gizmo de cor** decente — `color_picker` hoje *cicla* 7 cores
   (components.aipo:1255-1274); isso não é um seletor de cor. precisa de
   escolha de matiz/saturação/luminosidade + alfa + campo hex.

---

### F10 — Acessibilidade como requisito 🟡 Médio

Requisitos verificáveis, todos testáveis:

| Requisito | Teste |
|:--|:--|
| 1.4.11 Non-text contrast ≥3:1 em todo estado | varredura de contraste por (superfície, borda) |
| Texto: passos 11/12 a APCA Lc 60/90 | teste de token |
| 2.4.13 Focus appearance: anel ≥3:1 focado vs não | teste de pixel em token |
| 2.5.8 Target size ≥24×24 | assert em todos os controles interativos |
| 1.4.13 Hover dismissível + persistente | teste de `Escape` fecha tooltip |
| `prefers-reduced-motion` | `motion_scale` = 0 desabilita molas |
| Árvore semântica de acessibilidade | `zoe.a11y_tree()` expõe papéis e rótulos |
| Live region para estado dinâmico | `zoe.announce(msg)` em mudança significativa |

O `aipo-egui` já integra AccessKit (`crates/aipo-egui`) — há precedente no repo
para expor uma árvore de acessibilidade do Zoe.

---

## 5. Plano de fases

Cada fase é um PR. Sequenciamento respeita dependências.

| Fase | Nome | Conteúdo | Depende | Esforço | Risco |
|:--|:--|:--|:--|:--|:--|
| **M13** | Fundamentos numéricos e Correções de API | natives `sin/cos/sqrt/atan2/pow`; `hex()` completo; `font_weight` lido; remover i18N hardcoded; `spacer(flex)`; clamp de scroll; C4 slider drag; C8 LRU real; C10/C11 editor | — | 3-5 dias | 🟢 Baixo |
| **M14** | SDF v2 (AA + sombra + borda + MSAA) | F2 integral; testes de paridade Rust↔GLSL | M13 | 5-8 dias | 🟡 Médio |
| **M15** | Batch de draw calls | F1 integral; display list; métricas de draw calls | M14 | 8-12 dias | 🔴 Alto |
| *M15-A* | *Redução e medição do que dá para fazer sem trocar de pipeline* | *Culling puro, contadores de draw call, elisão de uniforms, anel de foco no shader* | *M14* | *2 dias* | *🟢 Baixo* — **concluída** |
| *M15-B* | *Instancing por material (resto do F1)* | *Pipeline miniquad cru, VBO de instâncias, layout 9 × vec4, descarga por lote* | *M15-A* | *6-10 dias* | *🔴 Alto — requer ADP de `unsafe` e verificação com display* |
| **M16** | Tipografia de produção | F3: shaping, subpixel, peso real, MSDF atlas; `text_wrap`/`align`/`ellipsis` | M15 | 10-15 dias | 🟠 Médio |
| **M17** | Tema em runtime + tokens | F5 integral; OKLCH; contraste automatizado; densidade | M16 | 6-9 dias | 🟡 Médio |
| **M18** | Layout memoizado + reatividade fina | F4 integral; cache; reconciliação; flags separadas | M15 | 8-12 dias | 🔴 Alto |
| **M19** | Interação e eventos | F7 integral: press/release, hover, dblclick, context menu, undo/redo, roving tabindex | M18 | 6-9 dias | 🟡 Médio |
| **M20** | Catálogo de componentes | F6: ~16 componentes + correções; ícones em SDF atlas | M16, M19 | 10-15 dias | 🟡 Médio |
| **M21** | Viewport 2D/3D | F9: z-buffer, MSAA, gizmo completo, snap, marquee, cor picker | M15 | 8-12 dias | 🟠 Médio |
| **M22** | Chrome de editor | F8: docking, persistência, command palette, asset shelf | M19, M20 | 10-15 dias | 🟠 Médio |
| **M23** | Acessibilidade | F10 integral; testes de conformidade | M17, M19 | 5-8 dias | 🟡 Médio |
| **M24** | Blur / backdrop | dual-Kawase; `blur` prop; `backdrop` prop | M15, M17 | 6-10 dias | 🟠 Médio |

**Total estimado**: 85-125 dias de trabalho focado.

**Ordem alternative "quick win"** (se o objetivo for demo visual em 2 semanas):
M13 → M14 → M17 → (M20 parcial). Isso entrega sombra real, AA correto, tema
funcional e correções visíveis sem tocar na parte difícil (batch, layout, texto).

### Gates de qualidade por fase

```
cargo fmt --check && cargo clippy --all-targets -- -D warnings
cargo test -p aipo-game-host          # 13 testes existentes + novos
npm run docs:build                    # VitePress
```

**Métricas a medir e reportar por fase** (novo teste de benchmark):

| Métrica | Hoje (medido) | Alvo |
|:--|--:|--:|
| Draw calls / quadro (editor) | ~400+ | ≤ 8 |
| `set_uniform` / quadro | ~2000 | 0 |
| Tempo de `rebuild_tree` + Leona | não medido | < 1.5ms @ 400 nósdes |
| Quadros para um tween de 300ms | ~18 (rebuild completo cada um) | 18 (paint only) |
| Nósdes com cache de layout | 0 | ≥ 95% estático |
| Contraste de texto (min) | não medido | APCA Lc 60 |
| Erro de `cos(0.6)` | 4.22% | < 1e-9 |

---

## 6. Plano de atualização documental

### 6.1 Novas páginas em `docs/zoe/`

| Página | Conteúdo | Idioma |
|:--|:--|:--|
| `design-system.md` | Tokens, escala OKLCH, estados, densidade, tabelas de referência | PT + EN |
| `theming.md` | API de tema, presets, seed de marca, como criar tema próprio | PT + EN |
| `motion.md` | Molas, tokens de movimento, `prefers-reduced-motion` | PT + EN |
| `typography.md` | Pilha de texto, shaping, MSDF, eixos de fonte variável | PT + EN |
| `accessibility.md` | Papéis, teclado, contraste, live regions | PT + EN |
| `editor-chrome.md` | Padrões de docking, persistência, palette, asset shelf | PT + EN |
| `rendering-pipeline.md` | Batching, SDF v2, ordem de desenho, clipping, métricas | PT + EN |
| `performance.md` | Métricas, como medir, budgets, profiling | PT + EN |

### 6.2 Atualização de páginas existentes

| Página | Mudança |
|:--|:--|
| `guide/layout-leona.md` | Documentar cache, reconciliação, `flex` de `spacer`, constraints |
| `guide/reactivity.md` | Reescrever: `use_spring`, memo entre quadros, flags de dirty separadas |
| `guide/custom-components.md` | Novo protocolo: `node_kind`, props semânticas, acessibilidade, tokens |
| `components/buttons.md` | Estados por token, anel de foco bicolor, ícones SDF |
| `components/inputs.md` | `color_picker` real, `search_input`, drag-scrub com expressões |
| `components/navigation.md` | `command_palette`, `context_menu`, `tabs` com dirty |
| `components/advanced.md` | `graph` com pan/zoom/snap, `asset_grid` |
| `index.md` | Atualizar o grafo Mermaid; remover afirmações de "M3 fine-grained" até M18 |
| `playground.md` | Exemplos executáveis de tema, spring, dock |

### 6.3 Documentação de arquitetura

| Artefato | Conteúdo |
|:--|:--|
| `docs/architecture/gpu-rendering.md` (novo) | Pipeline SDF, batching, instancing, ordem de desenho, MSAA, clipping |
| `docs/architecture/host-abi.md` (existente) | **Atualizar**: novos natives de M13 (trig), M14 (batch), M16 (texto) |
| `docs/crates/` | Entrada para `aipo-game-host` GPU modules |

### 6.4 ADPs (Architecture Decision Proposals)

Decisões que **precisam** ser registradas como ADP, porque restringem o futuro:

| ADP | Título | Decisão |
|:--|:--|:--|
| **ADP-014** | Instancing por material no host | Sair da API `Material` do macroquad para `miniquad` direto; batching é requisito, não otimização |
| **ADP-015** | SDF como único backend de UI | Retângulos, bordas, sombras, foco e clip arredondado saem de um fragment shader único; fallback de software só em headless |
| **ADP-016** | Tokens semânticos em OKLCH | Contraste garantido estruturalmente (APCA), testado; tema em runtime é requisito de produto |
| **ADP-017** | Reatividade preservada, não descartada | `use_state` mantém identidade entre quadros; reconciliação em vez de rebuild; sem isto, componentes de usuário quebram |
| **ADP-018** | Matematicamente correto no host | `sin/cos/sqrt/atan2/pow` são natives f64; Aipo não implementa transcendentais |
| **ADP-019** | Acessibilidade como gate de CI | Contraste, target size e focus appearance falham o build |

### 6.5 CHANGELOG e PROJECT_STATE

- `CHANGELOG.md`: entrada `## [0.12.0] — Em desenvolvimento` por fase, no
  formato existente (M13…M24), com sub-bullets de implementação e testes.
- `PROJECT_STATE.md`: substituir a seção de Zoe (linhas 166-174) por um resumo
  do novo estado, com link para o dossiê.
- `packages/aipo-zoe/README.md`: atualizar a seção de roadmap — hoje lista
  M1-M12 como concluídos; reescrever para M13-M24 e **corrigir as afirmações**:
  - "Cache de texturas GPU LRU" → não existe (C8)
  - "M3 Fine-grained" → não é fine-grained hoje (F4)
  - "Cursor com tween" no code_editor → não há tween
- Espelho em `docs/en/zoe/` para todas as páginas novas.

### 6.6 Testes como documentação

| Teste | O que documenta |
|:--|:--|
| `test_theme_contrast_wcag` | Garantia de contraste do design system |
| `test_sdf_shader_anchor_parity` | Tabela de elevação Rust ↔ GLSL não divergem |
| `test_draw_call_budget` | Editor fica ≤ N draw calls |
| `test_layout_cache_hit_rate` | Cache de layout é realmente usado |
| `test_spring_convergence` | Molas assentam no tempo esperado |
| `test_click_release_semantics` | Botão não dispara ao arrastar para fora |
| `test_trig_accuracy` | `cos(0.6)` dentro de 1e-9 |
| `test_a11y_target_sizes` | Todo controle interativo ≥ 24×24 |
| `test_hex_parser_completeness` | `hex()` cobre os 3 formatos |

---

## 7. Riscos

| # | Risco | Probabilidade | Impacto | Mitigação |
|:--|:--|:--|:--|:--|
| R1 | Instancing quebra headless tests | Alta | Alto | Manter fallback de software; testes headless nunca tocam GL |
| R2 | Layout memoizado introduz bug de dessincronização | Média | Crítico | Cache opt-in por fase; property tests de determinismo Leona |
| R3 | MSAA + SDF interage mal em GPUs antigas | Média | Médio | Detectar `sample_count`; degradar para sem MSAA |
| R4 | Fontes variáveis não disponíveis em todos os alvos (WASM) | Média | Médio | Fallback para peso estático; `aipo-wasm` tem backend próprio |
| R5 | Escopo cresce e a fase não fecha | Alta | Alto | Hard gate: cada fase é um PR verde; sem "parcial" |
| R6 | Regressão de performance no VM | Média | Alto | Benchmark por fase; budget de stack frames |
| R7 | Documentação dessincroniza do código | Alta | Médio | Páginas geradas de testes; CI checa exemplos |
| R8 | Acessibilidade exige mudança de API pública | Alta | Médio | Fazer na M23 cedo o *design*, implementar junto de M20 |
| R9 | M15-B exige `unsafe` e o crate é `#![forbid(unsafe_code)]` | Certa | Médio | ADP de postura de segurança antes do código; isolar o `unsafe` num único módulo auditado ou extrair o renderizador para um crate próprio |
| R10 | M15-B troca o caminho de desenho sem poder ser verificada pelo gate headless | Alta | Alto | Não trocar o caminho vivo sem rodar a GUI com display; manter o caminho atual atrás da mesma interface |

---

## 8. Referências

**Código local**
- `packages/aipo-zoe/src/{types,color,tokens,leona,hooks,elements,components,widgets,renderer,app,overlay,icons,retro3d,lib}.aipo`
- `crates/aipo-game-host/src/{main,host_bridge,audio_system}.rs`
- `crates/aipo-egui/src/{adapter,context,schema,shapes}.rs`
- `packages/aipo-zoe/{examples,tests}`
- `docs/zoe/**`, `docs/en/zoe/**`, `docs/decisions/adp-{001..013}.md`

**Mercado**
- egui 0.36 / epaint — [docs.rs/egui](https://docs.rs/egui)
- Slint 1.16 — [slint.dev](https://slint.dev)
- Flutter Impeller — [flutter.dev/docs/perf/impeller](https://docs.flutter.dev/perf/impeller)
- Godot 4.7 `Theme` — [docs.godotengine.org/classes/class_theme.html](https://docs.godotengine.org/en/stable/classes/class_theme.html)
- Radix Colors — [radix-ui.com/colors](https://www.radix-ui.com/colors)
- Material 3 / HCT — [material-foundation.github.io/material-color-utilities](https://github.com/material-foundation/material-color-utilities)
- Dual-Kawase — Bjørge @ ARM, SIGGRAPH 2015
- MSDF — [github.com/Chlumsky/msdfgen](https://github.com/Chlumsky/msdfgen)
- W3C Design Tokens (2025-10-28) — [CG-FINAL-format](https://www.w3.org/community/reports/design-tokens/CG-FINAL-format-20251028/)
- WCAG 2.2 — [w3.org/TR/WCAG22](https://www.w3.org/TR/WCAG22/)
- VS Code theming — [code.visualstudio.com/api/references/theme-color](https://code.visualstudio.com/api/references/theme-color)
- Blender theme source — `userdef_default_theme.c`
- Flutter spring math — `SpringDescription.withDurationAndBounce`
