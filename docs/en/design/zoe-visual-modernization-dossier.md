# Zoe UI (`aipo.zoe`) Visual and Architectural Modernization Dossier

> **Scope**: in-depth analysis of the current implementation of the `aipo.zoe` GUI framework and of the
> game engine interface hosted in `aipo-game-host`, contrasted with the best modern GUI
> libraries and frameworks, focusing on **functionality, optimization,
> modern design, customization and visual beauty**.
>
> **Deliverables**: (1) a verifiable diagnosis of the current state, (2) a technical benchmark
> against market references, (3) a design system specification, (4) an implementation strategy
> with concrete code, (5) a phase plan, (6) a documentation update plan.
>
> **Audit date**: 2026-09-28 · **Revision**: e4ef0ee · **Build state**: `cargo build -p aipo-game-host` ✅

---

## TL;DR — The 12 conclusions that matter most

1. **Zoe has no batching.** Each `host_draw_sdf_rect` emits one draw call with 5
   `set_uniform` + a material switch. A screen with 300 widgets = 300+ draw calls and
   1500 uniform writes per frame. No market framework accepts this.
2. **The SDF shader has three mathematical defects**, not stylistic ones: antialiasing with
   a fixed 1px width (it does not use `fwidth`/the L2 norm of the gradient), a border composed by
   `mix` over a single SDF (produces a gray line on light backgrounds and a wrong inner radius),
   and a total absence of shadow.
3. **`math_sin`/`math_cos` in `retro3d` are numerically wrong** — an error of
   **4.22%** in `cos(0.6)`. This is a bug, not an aesthetic choice.
4. **There is no runtime theming.** Catppuccin Mocha is hardcoded. `tokens.aipo`
   exists but **is not consumed** by `components.aipo` — the components use
   `color_mod.surface_1` directly. The token system is decorative.
5. **Reactivity is a "full rebuild"**: any `set_state` rebuilds the entire tree
   and re-runs Leona from scratch. There is no subtree memoization, layout cache
   or selective invalidation, despite the README claiming "M3 Fine-grained".
6. **`font_weight` is accepted as a prop and never read.** Every `"font_weight": "bold"` in the
   code is a no-op. The typographic hierarchy does not exist.
7. **There is no text ellipsis, alignment, letter-spacing or word-wrap.** A long label
   silently overflows its container.
8. **`hex()` only recognizes `#ffffff` and `#000000`** — any other value returns gray.
   I18N: `make_dialog` has `"Confirmar"`/`"Cancelar"` hardcoded in a framework with
   global reach.
9. **There is no icon batching nor LRU** — the README promises an LRU cache; the code uses a
   `HashMap` with no eviction (unbounded growth per color+size combination).
10. **Essential events are missing**: `on_hover`, `on_double_click`, `on_right_click`,
    `on_context_menu`, drag with threshold, undo/redo, command palette.
11. **There is no real docking nor layout persistence.** The editor is a fixed
    `split_view` tree; nothing survives a restart.
12. **There is no accessibility** beyond Tab/Enter/Esc. No roles, no labels, no
    `aria-live`, no `prefers-reduced-motion`, no verified contrast.

---

## 1. Diagnosis of the current state

### 1.1 Implementation map

| Layer | Artifact | LOC | Responsibility |
|:--|:--|--:|:--|
| Types | `packages/aipo-zoe/src/types.aipo` | 150 | `Rect`, `Color`, `Element`, signals |
| Color | `packages/aipo-zoe/src/color.aipo` | 65 | Normalization + Catppuccin palette |
| **Tokens** | `packages/aipo-zoe/src/tokens.aipo` | 126 | Typographic/spacing scale — **not consumed** |
| Layout | `packages/aipo-zoe/src/leona.aipo` | 578 | Measurement + flex + wrap + baseline |
| Reactivity | `packages/aipo-zoe/src/hooks.aipo` | 204 | `use_state`/`memo`/`effect`/`tween` |
| Components | `packages/aipo-zoe/src/components.aipo` | 1842 | 20+ components |
| Widgets | `packages/aipo-zoe/src/widgets.aipo` | 477 | `code_editor`, `node_graph`, modals |
| Render | `packages/aipo-zoe/src/renderer.aipo` | 209 | Recursion + draw-call dispatch |
| App loop | `packages/aipo-zoe/src/app.aipo` | 504 | Hit-test, events, focus, keyboard |
| Overlays | `packages/aipo-zoe/src/overlay.aipo` | 143 | Modal stack, tooltip |
| Icons | `packages/aipo-zoe/src/icons.aipo` | 102 | SVG → texture registry |
| 3D | `packages/aipo-zoe/src/retro3d.aipo` | 490 | Meshes, camera, gizmo |
| **GPU bridge** | `crates/aipo-game-host/src/host_bridge.rs` | 1932 | 42 host natives + SDF shader |
| Example | `packages/aipo-zoe/examples/editor.aipo` | 736 | Game engine editor |

**Total**: ~7,567 lines of Aipo + 1,932 of Rust bridge.

### 1.2 Current rendering pipeline

```
zoe.draw_ui()
  └─> app_render()                          app.aipo:486
       ├─> host_clear_background()          hardcoded 0.067,0.067,0.106
       └─> renderer.render_tree(root)       renderer.aipo:109
            └─> render_node(node)           renderer.aipo:7   ← RECURSION
                 ├─> host_draw_sdf_rect()    ← 1 DRAW CALL PER NODE
                 ├─> host_draw_rect_lines()  ← focus ring (2nd draw call)
                 ├─> host_draw_text()        ← 1 draw call per label
                 ├─> on_render()             ← 2D/3D viewport
                 ├─> on_custom_draw()        ← icons, code_editor
                 └─> children...             ← no display list, no batch
```

**Measured consequences:**

- No display list: every frame rebuilds and re-emits all geometry.
- No per-material batch: `set_uniform` × 5 per node (host_bridge.rs:630-642).
- No CPU culling beyond the clip rectangle.
- No layout cache: `leona_compute_layout` runs over the entire tree whenever
  `is_dirty()` (app.aipo:480-482).

### 1.3 Verified defects in the SDF shader

The current fragment shader (host_bridge.rs:230-279) has three problems that the market
solved years ago:

```glsl
// CURRENT (host_bridge.rs:253-255)
float alpha = clamp(0.5 - dist, 0.0, 1.0);   // ← fixed 1px width

// CURRENT (host_bridge.rs:271-274)
float b_dist = dist + border_width;
float border_mask = clamp(0.5 - dist, 0.0, 1.0) - clamp(0.5 - b_dist, 0.0, 1.0);
col = mix(col, u_border_color, clamp(border_mask * u_border_color.a * 1.5, 0.0, 1.0));
```

**D1 — Antialiasing with a fixed width.** `clamp(0.5 - dist, 0, 1)` assumes that 1
distance unit = 1 pixel. That is only true at 1:1 scale. Under
`camera_zoom`, under `high_dpi`, or under any transformation, the edge ends up either
too thick or too thin. The industry-standard fix is the **L2 norm of the
gradient** (not `fwidth`, which is ~√2× wider at 45° and fattens the corners):

```glsl
float aa = max(length(vec2(dpdx(d), dpdy(d))), 0.5);
float inside = 1.0 - smoothstep(-aa, 0.0, d);
```

**D2 — Border via `mix` over a single SDF.** The pattern documented by the community
itself is: *"`d = sdRoundedBox(...); d + borderWidth` loses the term
`min(max(q.x,q.y),0.0)`, so the inner radius comes out wrong"*. Furthermore, `mix`
over a semi-transparent fill does not composite correctly — the output needs to be
**premultiplied alpha** with `over`:

```glsl
float outA = ba + fa * (1.0 - ba);
vec3  outRGB = (border.rgb * ba + fill.rgb * fa * (1.0 - ba)) / max(outA, 1e-5);
```

**D3 — Zero shadow.** No `u_shadow_*` uniform exists. Overlays simulate
shadow with an offset black rectangle (`renderer.aipo:131`), which is the opposite of
depth — it produces a hard edge, not a gradient.

**D4 — Incorrect MSAA sampling.** The vertex output does not declare
`@interpolate(perspective, sample)`, so with MSAA on the shader runs once
per *pixel*, not per *sample*, and the `smoothstep` resolves at the wrong resolution.

### 1.4 Verified numerical defects in retro3d

`retro3d.aipo:77-91` uses a 5th-order Taylor approximation with 3 terms:

```text
fn math_sin(rad) { return x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0)) }
fn math_cos(rad) { return math_sin(rad + 1.5707963) }
```

Numerical verification (Python, reproducing the formula exactly):

```
sin(π/2): obtido 1.004525  esperado 1.000000  erro +0.45%
cos(0):    obtido 1.004525  esperado 1.000000  erro +0.45%
sin(1.5):  obtido 1.000781  esperado 0.997495  erro +0.33%
cos(0.6):  obtido 0.867580  esperado 0.825336  erro +4.22%   ← INACEITÁVEL
```

The 4.22% error in `cos(0.6)` — which is literally the camera's default `yaw`
(`camera_3d(yaw = 0.6, ...)`) — visibly distorts the projection. This
corrupts the depth perception of the 3D viewport.

**Fix:** implement `sin`/`cos`/`sqrt`/`atan2`/`pow` as **host natives** in
Rust (native f64). The host already has 42 natives; adding 5 is trivial and removes the
entire class of precision bugs. Cost: one FFI call per use, which is
irrelevant compared to the current error.

### 1.5 API and component defects

| # | Defect | Location | Impact |
|:--|:--|:--|:--|
| C1 | `font_weight` never read | `renderer.aipo` does not reference it | The whole typographic hierarchy is fictitious |
| C2 | `hex()` only accepts 2 values | `color.aipo:20-28` | Any hex color returns `(100,100,100)` |
| C3 | I18N hardcoded in PT-BR | `components.aipo:1321-1322` | Global framework with fixed strings |
| C4 | `slider` only has `on_click`, no `on_drag` | `components.aipo:119-151` | The slider cannot be dragged |
| C5 | `spacer(flex)` ignores the argument | `lib.aipo:84-90` | `flex: 3.0` does nothing |
| C6 | Scroll with no upper clamp | `components.aipo:565` | Scrolls to infinity |
| C7 | Scroll thumb with a fixed 40px height | `components.aipo:638` | Does not reflect the content proportion |
| C8 | Icon cache = `HashMap` with no eviction | `host_bridge.rs:87` | README promises LRU; memory grows without limit |
| C9 | `tree_view` chevron is the text `"v "`/`"> "` | `components.aipo:1432` | ASCII where there should be an icon |
| C10 | Editor shows fake telemetry | `editor.aipo:709-710` | `"FPS: 60"` and `"Leona Layout: OK"` hardcoded |
| C11 | "Delete Entity" button with no action | `editor.aipo:149` | `fn(_) {}` — broken affordance |
| C12 | State colors do not exist | all of `components.aipo` | Hover/pressed derive from a uniform `+0.12` |

### 1.6 What is right and should be preserved

Not everything is a defect. These are **solid decisions** worth keeping:

- **Framework 100% in pure Aipo.** Zero OS UI dependency. A real differentiator.
- **Leona with real baseline alignment** using font metrics — rare.
- **Runtime font metrics** (`host_font_metrics`) — the right foundation for
  professional typography, as long as `font_weight` starts being honored.
- **Post-clipping overlays** with scrim — correct, they only need a real shadow.
- **`virtual_list` with overscan** — the algorithm is right.
- **`scrubber_input` with modifiers** (Shift 0.1x, Ctrl 10x) — matches Blender.
- **Stack-frame optimization** (70 → 18 slots) — serious VM work.

---

## 2. Market benchmark

### 2.1 Comparison matrix

| Capability | **Zoe today** | egui 0.36 | Slint 1.16 | Godot 4.7 | Blender 5.2 | Flutter/Impeller |
|:--|:--|:--|:--|:--|:--|:--|
| Model | Retained-ish, full rebuild | Immediate | Declarative+retained | Retained | Immediate | Retained |
| Batching | ❌ 1 call/node | ✅ per clip+texture | ✅ | ✅ | ✅ | ✅ |
| Rounded rect SDF | ⚠️ fixed AA | ✅ tessellation | ✅ FemtoVG/Skia | ✅ StyleBox | ✅ GPU_draw | ✅ Impeller |
| Shadow | ❌ | ⚠️ 4-tap offset | ✅ `drop-shadow-*` | ✅ StyleBox | ✅ | ✅ ImageFilter |
| Blur/backdrop | ❌ | ❌ | ⚠️ | ❌ | ❌ | ✅ dual-Kawase |
| Subpixel typography | ❌ | ✅ skrifa+vello | ✅ fontique+parley | ✅ | ✅ | ✅ |
| Variable fonts | ❌ | ✅ | ✅ | ⚠️ | ⚠️ | ✅ |
| Runtime theme | ❌ hardcoded | ✅ | ✅ | ✅ 6 presets | ✅ | ✅ |
| Density | ❌ | ⚠️ | ⚠️ | ✅ 3 presets | ✅ | ✅ |
| Incremental layout | ❌ | — | ✅ | ✅ | ✅ | ✅ |
| Widget state | ❌ | ✅ | ✅ `PseudoStates` | ✅ | ✅ | ✅ |
| Icons | ⚠️ 1 tex/color | ✅ SDF atlas | ✅ | ✅ | ✅ | ✅ |
| Undo/redo | ❌ | — | — | ✅ | ✅+history | — |
| Command palette | ❌ | — | — | ✅ F4 | ✅ | — |
| Persistent docking | ❌ | — | ❌ | ✅ 9 slots | ✅ Areas | — |
| Accessibility | ❌ Tab only | ✅ AccessKit | ✅ AccessKit | ✅ | ⚠️ | ✅ Semantics |
| Text wrap | ❌ | ✅ | ✅ | ✅ | ✅ | ✅ |
| DevTools inspector | ✅ F12 | ✅ | — | ⚠️ | ✅ | ✅ |

**Reading**: Zoe sits in the "competent layout framework with a rendering prototype"
category and needs to get to "production renderer + editor chrome". The
path is known and the market has already walked it.

### 2.2 The five highest-return levers

Drawn from the market research, ordered by impact on perceived quality:

1. **Real draw-call batching** with per-material instancing. From 300+ calls to
   3-5. It is the difference between "runs" and "doesn't run".
2. **Correct AA in the shader** (L2 norm + sample-rate MSAA). The edge is what the eye
   judges first; a 1px error across 300 rectangles is immediately visible.
3. **Real typography** — shaping, subpixel positioning, real weight, MSDF.
   Text dominates the perception of polish; engines that get this wrong never look
   "professional" (Slint fixed exactly this and the commit is documented).
4. **Unify rect/border/shadow/focus in a single pass.** Fewer passes, more
   control, and the shadow comes for free.
5. **OKLCH semantic tokens with runtime theming.** Without that, "customizable" is
   just a list of props.

---

## 3. Design System specification — "Aipo Design Language"

### 3.1 Color model: semantic OKLCH

Today: 26 hardcoded Catppuccin colors as module constants. Proposal: a
semantic OKLCH scale with **12 steps of fixed role** (the Radix model, the most mature),
generated from a single configurable accent.

```text
# theme.aipo — new module
struct Palette {
    # 12 steps per scale, fixed roles (Radix model)
    app_bg, app_bg_subtle        # 1, 2
    element_bg, element_hover, element_active   # 3, 4, 5
    border_subtle, border, border_hover          # 6, 7, 8
    solid, solid_hover           # 9, 10
    text_muted, text             # 11, 12  ← guarantee APCA Lc 60 / Lc 90
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

**Contrast contract guaranteed structurally** (not by manual review):
steps 11 and 12 of each scale reach APCA Lc 60 and Lc 90 over step 2 of the same
scale. This becomes an **automated test**, not a review.

**Initial presets**: `aipo_dark` (default, derived from Catppuccin Mocha), `aipo_light`
(Latte), `midnight` (near-black OLED), `solarized`. Switchable at runtime.

**Brand seed**: `Theme::from_accent(oklch_l, oklch_c, oklch_h)` generates the entire
scale. That is what makes the UI *truly customizable*.

### 3.2 Widget states — what is missing today

The current renderer does this (renderer.aipo:28-32):

```text
if (node.is_hovered or node.is_active) and (...) {
    bg_r = if bg_r + 0.12 > 1.0 then 1.0 else bg_r + 0.12   # ← uniform "+0.12"
    bg_g = ...; bg_b = ...
}
```

Problems: (a) a single state for hover and active, (b) a uniform increment
ignores chroma, (c) there is no disabled or selected state.

Proposal — per-state tokens, inheriting the Godot model (monotonic ramp, not swap):

| State | Background | Border | Text | Cursor |
|:--|:--|:--|:--|:--|
| `rest` | `element_bg` | `border_subtle` | `text` | `default` |
| `hover` | `element_hover` | `border_hover` | `text` | `pointer` |
| `active` | `element_active` | `border_hover` | `text` | `pointer` |
| `selected` | `accent` @ 0.16 | `accent` @ 0.45 | `accent` | `pointer` |
| `focused` | same as `rest` | **two-color focus ring** 2px | same | — |
| `disabled` | `element_bg` @ 0.5 | `border_subtle` @ 0.4 | `text_muted` | `not-allowed` |

**Two-color focus ring** (WCAG 2.4.13 criterion / technique C40): an
`accent` ring with a 1px outer `surface` guarantees contrast against *any* background.
Today the ring is a single 2px blue `host_draw_rect_lines` (renderer.aipo:52) —
it fails on a dark background **and** on a saturated light background.

**Icons follow the same alpha ramp**: normal 0.85, secondary 0.6, disabled
0.35, pressed = saturated accent (`accent * 1.15`).

### 3.3 Typography

Adopt the text stack that the research identified as the most mature
(Linebender): **fontdue/skrifa for shaping + outlines**, with:

- **Subpixel glyph positioning** — without it, fractional advances accumulate and
  long lines drift. It is the commit Slint just made.
- **Real weight**: `font_weight` maps to the `wght` axes of Inter Variable.
  Inter is already embedded (860KB) as a variable font — the asset supports it, the code ignores it.
- **Axes**: `font_size`, `line_height`, `letter_spacing`, `font_variation`.
- **Baseline alignment already exists** and is correct — keep it.

Tokens: `--font-mono` for code (necessary: the code editor uses the UI font).

### 3.4 Motion: springs, not curves

The current `use_tween` (hooks.aipo:154-179) has 4 fixed curves and calls `mark_dirty()` on
every frame — which forces a **full tree rebuild per frame during an
animation**. This is the probable cause of hitching in any tween today.

Proposal: a **damped spring engine** with the same math as Flutter's
`SpringDescription`, exposed as `use_spring(target)`:

```
stiffness = 4π² / duration²
damping   = dampingRatio · 2√(mass · stiffness)
dampingRatio = 1.0            → critically damped (effects: color, opacity)
dampingRatio = 0.6 … 0.9      → underdamped (spatial: position, scale)
```

M3 Expressive tokens as a reference:

| Token | ζ (damping) | k (stiffness) |
|:--|--:|--:|
| `effects.fast` | 1.0 | 3800 |
| `effects.default` | 1.0 | 1600 |
| `spatial.default` | 0.9 | 700 |
| `spatial.slow` | 0.9 | 300 |

`motion_scale = 0.0` implements `prefers-reduced-motion`.

### 3.5 Density

Three presets derived from two integers (Godot model, the cleanest):

| Preset | `base_spacing` | `extra_spacing` | row height |
|:--|--:|--:|--:|
| `compact` | 2 | 2 | 22px |
| `default` | 4 | 0 | 26px |
| `spacious` | 6 | 2 | 30px |

Plus `ui_scale` (0.75 → 1.5) separate from density, to respect the
OS display preferences.

---

## 4. Implementation strategy

Ten initiatives, ordered by dependency. Each one is independently executable
and verifiable.

### F1 — Draw-call batching (per-material instancing) 🔴 Critical

**Problem**: `host_draw_sdf_rect` emits 1 draw call + 5 `set_uniform` per node.
In an editor with 400 nodes that is 400 calls and 2000 uniform writes per frame.

**Solution**: move from uniforms to **per-instance attributes** in a single VBO,
with a single draw call per batch. This requires leaving macroquad's `Material` API and using
`miniquad` directly.

```rust
// crates/aipo-game-host/src/gpu/instance_batch.rs (new)

/// Per-instance attribute layout: 9 × vec4 = 36 floats = 144 bytes.
/// Mirrors exactly the `i_*` attributes declared in `sdf.frag` (GLSL ES 100).
/// This is a contract: changing it here requires changing the shader in the same commit,
/// and the test `test_sdf_instance_layout_parity` fails if they diverge.
pub const STRIDE_F32: usize = 36;

/// A unit quad shared by all instances (loc 0).
const UNIT_QUAD: [f32; 8] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];

/// miniquad's `max_attributes_per_vertex` is 16; 1 (quad) + 9 (instance)
/// = 10, with room to spare.
pub const INSTANCE_ATTRS: [(usize, usize, usize); 9] = [
    // (location, offset_in_f32, buffer)
    (1,  0, 1), // rect         (x, y, w, h)   — PAINT rect
    (2,  4, 1), // fill         (r, g, b, a)
    (3,  8, 1), // stroke       (r, g, b, a)
    (4, 12, 1), // params       (stroke_w, max_radius, elevation, focus_w)
    (5, 16, 1), // inner_rect   (x, y, w, h)   — LAYOUT rect
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
    /// Doubles the capacity while preserving the contents. Called in `push`
    /// when `count == capacity`; amortized by doubling.
    fn grow(&mut self) { /* ... */ }

    pub fn push(&mut self, inst: &SdfInstance) {
        if self.count == self.capacity { self.grow(); }
        // f32::write directly at the tail of the VBO, with no intermediate allocation
    }

    /// Emits a single `draw_elements_instanced` for the entire batch.
    pub fn flush(&mut self, pass: &mut dyn BatchRender) {
        if self.count == 0 { return; }
        pass.draw_instanced(self.quad_vbo, self.instance_vbo, self.count);
        self.count = 0;
    }
}
```

**Layout decisions (closed, not open):**

- **9 `vec4` = 36 floats.** Two shadow layers are separate attributes
  (loc 8 and 9) rather than one compressed `vec4`. It costs 16 bytes per instance;
  for 4000 instances that is 64KB, irrelevant. In exchange the shader stays readable and
  `shadow_layer_alpha()` receives a clean `vec4`.
- **`inner_rect` is separate from `rect`** because the focus ring can overflow
  (*paint overflow*): the layout does not change, but the painted area does. Without this
  separation, the focus overflows the layout box and CPU culling clips the ring.
- **Paint `rect` vs layout `rect`** also solves the shadow case:
  the shadow is drawn from the layout rect offset by `dy`, with
  spread — if we used the paint rect, the shadow would grow along with it.
- **`elevation` as a continuous float in `params.z`**, not as an anchor
  index. The shader interpolates between the `SHADOW_ANCHORS`, so raising and
  lowering a surface is continuous. The anchor table lives in Rust and is
  **verified against the GLSL by a test** (see F2).

**Boundary**: `InstanceBatch` is a Rust host type. No change to the
Aipo language. `renderer.aipo` just calls `host_sdf_begin()` /
`host_sdf_push(...)` / `host_sdf_flush()`.

**Migration without breaking the headless fallback**: `host_sdf_flush()` detects the absence
of a GL context and hands the instance list to the existing software
rasterizer (`safe_draw_round_rect` + `safe_draw_round_rect_lines`). The 13 current
tests keep passing without a GPU — it is the same pattern that `safe_draw_sdf_rect` already
uses today (`host_bridge.rs:653-658`).

**Expected gain**: 400 draw calls → 1-3. An order of magnitude.

**Status and verified feasibility (2026-09-28):**

An earlier analysis of this initiative concluded that F1 was **blocked** by
macroquad, because `Context::camera_matrix` is private and a raw miniquad pipeline
could not reuse the camera projection. **That conclusion is incorrect** and was
verified against the source code of `macroquad 0.4.16`:

| Need | Real primitive | Where |
|:--|:--|:--|
| Draw with raw miniquad in the middle of a macroquad frame | `get_internal_gl()` (public) returns `quad_gl` + `quad_context`, and `InternalGlContext::flush()` is documented as "useful for combining macroquad drawing with raw miniquad/OpenGL calls" | `macroquad/src/window.rs` |
| Get the current projection, including the camera | `QuadGl::get_projection_matrix()` — exists explicitly for third-party plugins | `macroquad/src/quad_gl.rs` |
| Per-instance attributes | `miniquad::VertexStep::PerInstance` | GL/WebGL backend |
| Instance buffer and per-instance index | `draw_elements_instanced` / `draw(base, count, instances)` | `miniquad` |

What is **not** possible is batching while keeping the `Material` API: in
`QuadGl::set_uniform` and in `QuadGl::pipeline` macroquad sets
`break_batching = true`, and `gl_use_material()` is a `pipeline()` — so **every
`set_uniform` and every material switch forces a new draw call**. Since the
geometry of each quad travels in uniforms (`u_quad_size`, `u_box_half`) and macroquad's
vertex format is fixed (`position` vec2 + `texcoord` vec2 +
`color0` vec4, 32 bytes per vertex), the 36 floats per instance **do not fit in
attributes**. Full F1 really does require leaving the `Material` API (ADP-014), as the
dossier had already anticipated.

**What is already delivered (M15-A)**, without changing the drawing path:

- pure culling of quads that do not paint (`sdf_cull_reason`: `OffScreen`,
  `ZeroArea`, `Invisible`), with 10 tests;
- public counters `sdf_draws_issued` / `sdf_draws_culled`, without which the
  "≤ 8 draw calls per frame" target is not measurable;
- elision of uniform uploads through `SdfUniformState`, invalidated in
  `init_zoe_shaders()`;
- the focus ring in the fragment shader, closing the gap left by M14.

**What remains (M15-B)** — and the two barriers that need a decision before
the code:

1. **`#![forbid(unsafe_code)]`**: declared in `crates/aipo-game-host/src/lib.rs`
   **and** in `main.rs`, and `get_internal_gl()` is `unsafe`. `forbid` cannot be
   loosened by `#[allow]` in any child module — not even in `host_bridge.rs`,
   which also repeats the attribute. The path requires switching to
   `#![deny(unsafe_code)]` with **a single** `#[allow(unsafe_code)]` in an
   isolated, audited module, or extracting the renderer into a crate of its own. It is a
   decision about security posture, so it belongs in an ADP before the code.
2. **Verification**: no headless test proves that the batch is rasterized in the
   right order, clip and viewport (the viewport scissor goes through
   `Camera2D` + `apply_scissor_rect`, and the material is switched in the middle of the frame).
   M15-B can only be declared complete with the GUI run on a machine with a
   display; without that, swapping the drawing path is an unverified regression
   on top of a renderer that works today.

The rest is implementation: an instanced pipeline, an instance VBO with the
9 × vec4 layout mirrored in the shader, and per-batch flushing.

---

### F2 — SDF Shader v2 (AA, shadow, correct border, MSAA) 🔴 Critical

Rewrite the fragment shader with the fixes from the research. **Kept in GLSL ES
100** (what `miniquad` already consumes today) — porting to WGSL would require a new
compilation path with no functional gain.

```glsl
// crates/aipo-game-host/src/gpu/sdf.frag  (GLSL ES 100)

// --- Per-instance attributes (loc 1..9). Mirrors `INSTANCE_ATTRS` in Rust. ---
attribute vec4 i_rect;         // xy = top-left in px, zw = size
attribute vec4 i_fill;         // rgba
attribute vec4 i_stroke;       // rgba
attribute vec4 i_params;       // x=stroke_w  y=max_radius  z=elevation  w=focus_w
attribute vec4 i_inner_rect;   // xy = top-left, zw = size  (LAYOUT, not paint)
attribute vec4 i_focus_color;  // rgba
attribute vec4 i_radii;        // tl, tr, br, bl
attribute vec4 i_shadow1;      // dy, blur, spread, alpha
attribute vec4 i_shadow2;

varying lowp vec2 uv;
varying lowp vec4 color;
varying highp vec2 pos_px;     // ← D4: vary in PIXEL COORDINATES, not uv

uniform mat4 Model;
uniform mat4 Projection;

// SDF with PER-CORNER radii (enables asymmetric "squircles")
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

    // D1 FIX: L2 norm of the gradient. `fwidth` is the L1 norm (~√2× wider
    // at 45°) and visibly fattens the rounded corners.
    float aa = max(length(vec2(dFdx(d), dFdy(d))), 0.5);
    float inside = 1.0 - smoothstep(-aa, 0.0, d);

    vec4 col = color;

    // --- Shadow: 2 layers, composited in CSS order ---
    if (i_params.z > 0.0) {
        float a1 = shadow_alpha(local, half_size, radii, i_shadow1);
        float a2 = shadow_alpha(local, half_size, radii, i_shadow2);
        float a_css = 1.0 - (1.0 - a1) * (1.0 - a2);
        // Gamma compensation: black shadow in ENCODED sRGB needs 2.2,
        // otherwise an alpha of 0.08 renders with ~half the intended darkness.
        float sa = 1.0 - pow(1.0 - a_css, 2.2);
        col = vec4(0.0, 0.0, 0.0, sa);
    }

    // --- Fill over the shadow (mix + max: never overwrite the shadow) ---
    if (i_fill.a > 0.0) {
        float fa = i_fill.a * inside;
        col = vec4(mix(col.rgb, i_fill.rgb, fa), max(col.a, fa));
    }

    // D2 FIX: independent SDFs for fill and border + premultiplied alpha.
    // `d + stroke_w` would lose the min(max(q.x,q.y),0.0) term and give a wrong inner radius.
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

    // D4: two-color focus ring. fw > 0 draws OUTSIDE the boundary (paint_overflow
    // halo); fw < 0 draws INSIDE, for dense rows with no padding.
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

**D4 — MSAA.** The vertex shader must emit `pos_px` as a high-precision
`varying` in absolute pixel coordinates, and the pipeline must declare
`sample_count > 1` with `miniquad`/OpenGL. With MSAA on, `dFdx`/`dFdy` are
evaluated per sample and the `smoothstep` resolves at the correct resolution — that is how
you get SDF **with** MSAA, not instead of it. If `sample_count == 1`, the same shader
works: `dFdx` still provides the correct pixel width.

**Elevation tables** — shadow anchors, derived from Tailwind and mirrored
between Rust and GLSL:

```rust
// crates/aipo-game-host/src/gpu/elevation.rs
/// (dy, blur, spread, alpha) per elevation level.
/// MUST literally mirror the anchors in `sdf.frag`.
pub const SHADOW_ANCHORS: [(f32, f32, f32, f32); 5] = [
    (  2.0,  3.0,  0.0, 0.05),  // 0 = shadow-xs
    (  4.0,  6.0, -1.0, 0.08),  // 1 = shadow-sm
    ( 12.0, 16.0, -3.0, 0.10),  // 2 = shadow-md
    ( 24.0, 32.0, -6.0, 0.14),  // 3 = shadow-lg
    ( 40.0, 48.0, -9.0, 0.18),  // 4 = shadow-xl
];

/// Linearly interpolates between anchors. `elevation` is continuous.
/// Above the last level, scales the xl geometry proportionally.
pub fn shadow_layers(elevation: f32) -> ([f32; 4], [f32; 4]) { /* ... */ }
```

**Mandatory parity tests** (prevent Rust and GLSL from diverging):

| Test | What fails |
|:--|:--|
| `test_sdf_instance_layout_parity` | `STRIDE_F32` and `INSTANCE_ATTRS` do not mirror `InstanceInput` |
| `test_sdf_elevation_anchor_parity` | each `vec4(dy, blur, spread, alpha)` of `SHADOW_ANCHORS` appears literally in `sdf.frag` |
| `test_sdf_shader_compiles_headless` | the shader compiles on all supported backends |

The third covers backend variations: `miniquad` transpiles GLSL 100 to
HLSL/MSL depending on the platform, so the same source needs to compile on Vulkan,
Metal, D3D and WebGL2. `test_sdf_shader_compiles_headless` runs the compilation for
each backend available in the CI environment.

**Closed decision: GLSL ES 100**, what the host already consumes today
(`ShaderSource::Glsl` in `host_bridge.rs:291`). No new dependency.

---

### F3 — Production typography 🔴 Critical

**Problem**: `host_draw_text` delegates to macroquad's `draw_text` — no shaping,
no subpixel positioning, no weight. And `font_weight` is a no-op.

**Solution in three parts**:

1. **Batched text host native** — `host_text_begin(font_id)`,
   `host_text_push(glyph_run, x, y_baseline, size, color)`,
   `host_text_flush()`. Rust does the shaping once and emits glyph quads.

2. **Shaping with subpixel** — `fontdue` is already integrated. Enable
   `horizontal_kerning` (already present in `host_measure_text:1320`) **and**
   subpixel positioning (round glyph positions to ¼ pixel, not to a
   whole pixel). Without this, long lines visibly drift.

3. **Real weight via variable axis** — `host_font_metrics(size, weight)` and
   `host_measure_text(text, size, weight)` with the `wght` axis of Inter Variable.
   `font_weight: "bold"` comes to mean 700.

**Gain**: it is what separates a "game engine" from a "professional editor". The research is
explicit: engines that get text wrong never look professional.

---

### F4 — Memoized layout and fine-grained reactivity 🟠 High

**Problem**: `rebuild_tree()` (app.aipo:63) discards and rebuilds everything, and
`leona_compute_layout` walks the entire tree. `use_memo` exists but
`reset_hooks()` zeroes the indices on every rebuild, so the memo never survives between
frames — it is literally useless in the current state.

**Solution**:

1. **Per-node layout cache** — store `(avail_w, avail_h, intrinsic)`; if
   none changed, skip the recursion. Leona is deterministic, so this is safe.
2. **Stable node identity** — `Element.id` already exists; propagating it to
   descendants enables reconciliation.
3. **Reconciliation instead of discarding** — `rebuild` reuses nodes whose
   `id` + props have not changed. `use_memo` starts to count.
4. **Separate layout from paint** — `is_dirty_layout` and `is_dirty_paint` as
   distinct flags. A hover only repaints; it does not re-run Leona.
5. **Memo of `on_custom_draw`** — `code_editor` and `node_graph` re-tokenize the
   entire file on every frame (widgets.aipo:227). Cache by content hash.

**Contract to add to `Element`**:
```text
struct Element {
    id, tag, props, children
    var layout          # Rect
    var layout_key      # String   ← cache key
    var is_hovered, is_active, is_focused
    var paint_only      # Bool     ← dirty only for repaint
}
```

---

### F5 — Runtime theming and consumed Design Tokens 🟠 High

**Verified problem**: `tokens.aipo` exports 126 lines of tokens and
**no component uses them**. All use `color_mod.surface_1` directly.

**Solution**:

1. `theme.aipo` with `Palette` + `Theme` (section 3.1).
2. `zoe.set_theme(theme)` / `zoe.get_theme()`.
3. **Runtime switching without a full rebuild**: the renderer reads the current theme per node;
   switching the theme marks only `is_dirty_paint`.
4. `zoe.theme_dark()`, `zoe.theme_light()`, `zoe.theme_from_accent(l, c, h)`.
5. **Automated contrast test** — walk all (background, text) and
   (surface, border) pairs and fail if APCA Lc < 60 / < 90.
6. `density` and `ui_scale` as tokens, with a native `host_ui_scale()` so that
   Rust applies the scale to the draw calls (Aipo should not multiply every
   coordinate by hand).

---

### F6 — Missing components and fixes 🟠 High

**Fixes for defects C1-C12** (table in section 1.5).

**New components, in order of value for an editor**:

| Component | Why | Reference |
|:--|:--|:--|
| `command_palette` | Key entry point for everything | VS Code `Ctrl+Shift+P` |
| `tooltip` as a component | today it only exists in the renderer | — |
| `context_menu` | right click | Godot/Blender |
| `menu_bar` + `menu_item` | keyboard navigation | — |
| `select` (multi-selectable) | `dropdown_select` is single | — |
| `checkbox` | missing; `switch` is not a substitute | — |
| `tabs` with `close` and `dirty` | `tab_view` has no dirty state | VS Code `tab.activeModifiedBorder` |
| `panel` / `dock` | editor chrome | Godot 9 slots |
| `splitter` with snap | today there is no snap in `split_view` | Blender 0.49999 |
| `tooltip` with shortcut | *"tooltips should contain the shortcut"* | HIG |
| `empty_state` | empty panel | Zed |
| `toast` / `notification` | asynchronous feedback | — |
| `keybinding` / `shortcut` | text + key | — |
| `search_input` | filter with highlighting | Godot `listFilterWidget` |
| `graph` improvements | pan/zoom, multi-selection, snap | Blender |
| `asset_grid` | thumbnail grid | Blender Asset Shelf |
**Text**: `text_wrap`, `text_align` (`start`/`center`/`end`/`justify`),
`text_ellipsis`, `max_lines`, `letter_spacing`. None exist today.

---

### F7 — Professional-grade events and interaction 🟠 High

**Problem**: `on_click` fires on mouse **press** (app.aipo:189), not on
release. This means dragging outside the button still triggers it — a bug
that any user notices. There is also no drag threshold.

**Solution**:

1. **Separate press/release.** `on_click` only fires if the release happens inside
   the bounds *and* the total movement stays below `drag_threshold` (4px).
2. `on_hover_in` / `on_hover_out` — programmatic, not just visual.
3. `on_double_click` with timing + radius detection.
4. `on_right_click` → opens `context_menu`.
5. **Roving tabindex** for composites (tree, tabs, toolbar) instead of Tab
   traversing every item. It is the ARIA pattern and avoids Tab-skipping across 300 nodes.
6. **Spatial navigation** with arrow keys in `tree_view` and `node_graph`.
7. `on_scroll` with a typed `ScrollDelta` (pixels / lines / pages).
8. **Undo/redo** with coalescing: a slider drag = **one** undo entry,
   pushed on release (never per sample — Blender's documentation is explicit
   about this).

---

### F8 — Editor chrome 🟡 Medium

Turn `editor.aipo` from a demo into a **product**:

1. **Real docking** — 8 slots + bottom + floating, with a stable `layout_key`
   (never a key on the title, the bug that Godot's documentation records).
2. **Persistence** — `zoe.layout_save(name)` / `load(name)`, serialized as
   `SerializedPaneGroup` in Zed's model. 200ms debounce.
3. **Command palette** with fuzzy search and shortcuts.
4. **Real status bar** — actually use `host_frame_time()`. Remove the
   hardcoded `"FPS: 60"` (C10).
5. **Panels with search** — hierarchy and property filters, with *in-place*
   match highlighting (VS Code's `list.filterMatchBackground` pattern,
   better than a separate results panel).
6. **Inspector with real drag-scrub** — `scrubber_input` already has Shift/Ctrl; it lacks
   reset, expressions (`=`, `pi`, `sin`) and property pinning.
7. **Tree with relationship lines** and sticky scroll.
8. **Asset shelf** — the research cites: *"an empty asset shelf on first run is the
   most common way for an editor to look empty"*.
9. **Navigable undo history**, not binary.

---

### F9 — Production 2D/3D viewport 🟡 Medium

1. **Fix `math_sin`/`math_cos`** via f64 host natives (section 1.4).
2. **Z-buffer** — the Painter's algorithm (depth sorting) fails with
   interpenetrating geometry. A 16-bit depth buffer per frame costs almost
   nothing and solves it.
3. **MSAA** in the viewport.
4. **Complete gizmo** — rotation (rings) and scale (box handles), in addition to
   translation. Today there is only translation.
5. **Snapping** — grid and increment, with Ctrl for snap and Shift for
   precision, reversible *during* the gesture.
6. **Marquee selection** (lasso drag) and click to cycle depth.
7. **Adaptive grid** — two layers (minor/major) with distance fade,
   like Blender.
8. **Axes in the viewport corner**, with an interactive camera widget, and a PiP preview
   of the selected camera.
9. **A decent color gizmo** — `color_picker` today *cycles* 7 colors
   (components.aipo:1255-1274); that is not a color picker. It needs
   hue/saturation/lightness selection + alpha + a hex field.

---

### F10 — Accessibility as a requirement 🟡 Medium

Verifiable requirements, all testable:

| Requirement | Test |
|:--|:--|
| 1.4.11 Non-text contrast ≥3:1 in every state | contrast sweep per (surface, border) |
| Text: steps 11/12 at APCA Lc 60/90 | token test |
| 2.4.13 Focus appearance: ring ≥3:1 focused vs not | pixel test on token |
| 2.5.8 Target size ≥24×24 | assert on all interactive controls |
| 1.4.13 Dismissible + persistent hover | test that `Escape` closes the tooltip |
| `prefers-reduced-motion` | `motion_scale` = 0 disables springs |
| Semantic accessibility tree | `zoe.a11y_tree()` exposes roles and labels |
| Live region for dynamic state | `zoe.announce(msg)` on significant change |

`aipo-egui` already integrates AccessKit (`crates/aipo-egui`) — there is precedent in the repo
for exposing an accessibility tree from Zoe.

---

## 5. Phase plan

Each phase is a PR. Sequencing respects dependencies.

| Phase | Name | Content | Depends on | Effort | Risk |
|:--|:--|:--|:--|:--|:--|
| **M13** | Numerical foundations and API fixes | natives `sin/cos/sqrt/atan2/pow`; complete `hex()`; `font_weight` read; remove hardcoded i18N; `spacer(flex)`; scroll clamp; C4 slider drag; C8 real LRU; C10/C11 editor | — | 3-5 days | 🟢 Low |
| **M14** | SDF v2 (AA + shadow + border + MSAA) | F2 in full; Rust↔GLSL parity tests | M13 | 5-8 days | 🟡 Medium |
| **M15** | Draw-call batching | F1 in full; display list; draw-call metrics | M14 | 8-12 days | 🔴 High |
| *M15-A* | *Reduction and measurement of what can be done without switching pipelines* | *Pure culling, draw-call counters, uniform elision, focus ring in the shader* | *M14* | *2 days* | *🟢 Low* — **completed** |
| *M15-B* | *Per-material instancing (the rest of F1)* | *Raw miniquad pipeline, instance VBO, 9 × vec4 layout, per-batch flushing* | *M15-A* | *6-10 days* | *🔴 High — requires an `unsafe` ADP and verification with a display* |
| **M16** | Production typography | F3: shaping, subpixel, real weight, MSDF atlas; `text_wrap`/`align`/`ellipsis` | M15 | 10-15 days | 🟠 Medium |
| **M17** | Runtime theming + tokens | F5 in full; OKLCH; automated contrast; density | M16 | 6-9 days | 🟡 Medium |
| **M18** | Memoized layout + fine-grained reactivity | F4 in full; cache; reconciliation; separate flags | M15 | 8-12 days | 🔴 High |
| **M19** | Interaction and events | F7 in full: press/release, hover, dblclick, context menu, undo/redo, roving tabindex | M18 | 6-9 days | 🟡 Medium |
| **M20** | Component catalog | F6: ~16 components + fixes; icons in SDF atlas | M16, M19 | 10-15 days | 🟡 Medium |
| **M21** | 2D/3D viewport | F9: z-buffer, MSAA, complete gizmo, snap, marquee, color picker | M15 | 8-12 days | 🟠 Medium |
| **M22** | Editor chrome | F8: docking, persistence, command palette, asset shelf | M19, M20 | 10-15 days | 🟠 Medium |
| **M23** | Accessibility | F10 in full; conformance tests | M17, M19 | 5-8 days | 🟡 Medium |
| **M24** | Blur / backdrop | dual-Kawase; `blur` prop; `backdrop` prop | M15, M17 | 6-10 days | 🟠 Medium |

**Estimated total**: 85-125 days of focused work.

**Alternative "quick win" order** (if the goal is a visual demo in 2 weeks):
M13 → M14 → M17 → (partial M20). This delivers real shadow, correct AA, a working
theme and visible fixes without touching the hard part (batching, layout, text).

### Quality gates per phase

```
cargo fmt --check && cargo clippy --all-targets -- -D warnings
cargo test -p aipo-game-host          # 13 existing tests + new ones
npm run docs:build                    # VitePress
```

**Metrics to measure and report per phase** (new benchmark test):

| Metric | Today (measured) | Target |
|:--|--:|--:|
| Draw calls / frame (editor) | ~400+ | ≤ 8 |
| `set_uniform` / frame | ~2000 | 0 |
| `rebuild_tree` + Leona time | not measured | < 1.5ms @ 400 nodes |
| Frames for a 300ms tween | ~18 (full rebuild each) | 18 (paint only) |
| Nodes with layout cache | 0 | ≥ 95% static |
| Text contrast (min) | not measured | APCA Lc 60 |
| Error of `cos(0.6)` | 4.22% | < 1e-9 |

---

## 6. Documentation update plan

### 6.1 New pages in `docs/zoe/`

| Page | Content | Language |
|:--|:--|:--|
| `design-system.md` | Tokens, OKLCH scale, states, density, reference tables | PT + EN |
| `theming.md` | Theme API, presets, brand seed, how to create your own theme | PT + EN |
| `motion.md` | Springs, motion tokens, `prefers-reduced-motion` | PT + EN |
| `typography.md` | Text stack, shaping, MSDF, variable font axes | PT + EN |
| `accessibility.md` | Roles, keyboard, contrast, live regions | PT + EN |
| `editor-chrome.md` | Docking patterns, persistence, palette, asset shelf | PT + EN |
| `rendering-pipeline.md` | Batching, SDF v2, draw order, clipping, metrics | PT + EN |
| `performance.md` | Metrics, how to measure, budgets, profiling | PT + EN |

### 6.2 Updates to existing pages

| Page | Change |
|:--|:--|
| `guide/layout-leona.md` | Document cache, reconciliation, `spacer` `flex`, constraints |
| `guide/reactivity.md` | Rewrite: `use_spring`, memo across frames, separate dirty flags |
| `guide/custom-components.md` | New protocol: `node_kind`, semantic props, accessibility, tokens |
| `components/buttons.md` | Per-token states, two-color focus ring, SDF icons |
| `components/inputs.md` | Real `color_picker`, `search_input`, drag-scrub with expressions |
| `components/navigation.md` | `command_palette`, `context_menu`, `tabs` with dirty |
| `components/advanced.md` | `graph` with pan/zoom/snap, `asset_grid` |
| `index.md` | Update the Mermaid graph; remove "M3 fine-grained" claims until M18 |
| `playground.md` | Executable examples of theme, spring, dock |

### 6.3 Architecture documentation

| Artifact | Content |
|:--|:--|
| `docs/architecture/gpu-rendering.md` (new) | SDF pipeline, batching, instancing, draw order, MSAA, clipping |
| `docs/architecture/host-abi.md` (existing) | **Update**: new natives from M13 (trig), M14 (batch), M16 (text) |
| `docs/crates/` | Entry for `aipo-game-host` GPU modules |

### 6.4 ADPs (Architecture Decision Proposals)

Decisions that **must** be recorded as ADPs, because they constrain the future:

| ADP | Title | Decision |
|:--|:--|:--|
| **ADP-014** | Per-material instancing in the host | Leave macroquad's `Material` API for `miniquad` directly; batching is a requirement, not an optimization |
| **ADP-015** | SDF as the sole UI backend | Rectangles, borders, shadows, focus and rounded clip come out of a single fragment shader; software fallback only in headless |
| **ADP-016** | Semantic tokens in OKLCH | Contrast structurally guaranteed (APCA), tested; runtime theming is a product requirement |
| **ADP-017** | Reactivity preserved, not discarded | `use_state` keeps identity across frames; reconciliation instead of rebuild; without this, user components break |
| **ADP-018** | Mathematically correct in the host | `sin/cos/sqrt/atan2/pow` are f64 natives; Aipo does not implement transcendentals |
| **ADP-019** | Accessibility as a CI gate | Contrast, target size and focus appearance fail the build |

### 6.5 CHANGELOG and PROJECT_STATE

- `CHANGELOG.md`: an entry `## [0.12.0] — In development` per phase, in the
  existing format (M13…M24), with implementation and test sub-bullets.
- `PROJECT_STATE.md`: replace the Zoe section (lines 166-174) with a summary
  of the new state, with a link to the dossier.
- `packages/aipo-zoe/README.md`: update the roadmap section — today it lists
  M1-M12 as completed; rewrite it for M13-M24 and **correct the claims**:
  - "LRU GPU texture cache" → does not exist (C8)
  - "M3 Fine-grained" → is not fine-grained today (F4)
  - "Cursor with tween" in the code_editor → there is no tween
- Mirror in `docs/en/zoe/` for all new pages.

### 6.6 Tests as documentation

| Test | What it documents |
|:--|:--|
| `test_theme_contrast_wcag` | The design system's contrast guarantee |
| `test_sdf_shader_anchor_parity` | The Rust ↔ GLSL elevation table does not diverge |
| `test_draw_call_budget` | The editor stays at ≤ N draw calls |
| `test_layout_cache_hit_rate` | The layout cache is actually used |
| `test_spring_convergence` | Springs settle in the expected time |
| `test_click_release_semantics` | The button does not fire when dragging outside |
| `test_trig_accuracy` | `cos(0.6)` within 1e-9 |
| `test_a11y_target_sizes` | Every interactive control ≥ 24×24 |
| `test_hex_parser_completeness` | `hex()` covers the 3 formats |

---

## 7. Risks

| # | Risk | Probability | Impact | Mitigation |
|:--|:--|:--|:--|:--|
| R1 | Instancing breaks headless tests | High | High | Keep the software fallback; headless tests never touch GL |
| R2 | Memoized layout introduces a desynchronization bug | Medium | Critical | Opt-in cache per phase; Leona determinism property tests |
| R3 | MSAA + SDF interact poorly on older GPUs | Medium | Medium | Detect `sample_count`; degrade to no MSAA |
| R4 | Variable fonts not available on all targets (WASM) | Medium | Medium | Fall back to static weight; `aipo-wasm` has its own backend |
| R5 | Scope grows and the phase does not close | High | High | Hard gate: each phase is a green PR; no "partial" |
| R6 | Performance regression in the VM | Medium | High | Benchmark per phase; stack-frame budget |
| R7 | Documentation desynchronizes from the code | High | Medium | Pages generated from tests; CI checks examples |
| R8 | Accessibility requires a public API change | High | Medium | Do the *design* early in M23, implement together with M20 |
| R9 | M15-B requires `unsafe` and the crate is `#![forbid(unsafe_code)]` | Certain | Medium | Security-posture ADP before the code; isolate the `unsafe` in a single audited module or extract the renderer into a crate of its own |
| R10 | M15-B swaps the drawing path without being verifiable by the headless gate | High | High | Do not swap the live path without running the GUI with a display; keep the current path behind the same interface |

---

## 8. References

**Local code**
- `packages/aipo-zoe/src/{types,color,tokens,leona,hooks,elements,components,widgets,renderer,app,overlay,icons,retro3d,lib}.aipo`
- `crates/aipo-game-host/src/{main,host_bridge,audio_system}.rs`
- `crates/aipo-egui/src/{adapter,context,schema,shapes}.rs`
- `packages/aipo-zoe/{examples,tests}`
- `docs/zoe/**`, `docs/en/zoe/**`, `docs/decisions/adp-{001..013}.md`

**Market**
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
