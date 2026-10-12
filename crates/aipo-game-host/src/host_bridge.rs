//! Host FFI bridge connecting Aipo VM bytecode execution with Macroquad/Miniquad GPU rendering.
//!
//! Provides hardware-accelerated 2D draw calls, real-time keyboard/mouse input,
//! texture and spritesheet pipeline, and camera transformations with headless fallback.

#![forbid(unsafe_code)]

use crate::audio_system::{AUDIO, SynthConfig};
use aipo_sema::PreludeSurface;
use aipo_vm::{Value, Vm, VmFault};
/// Re-exported so integration tests can construct `SdfQuad` values without
/// depending on macroquad directly.
pub use macroquad::color::Color;
use macroquad::miniquad::{BlendFactor, BlendState, BlendValue, Equation};
use macroquad::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use std::sync::Mutex;
use std::sync::atomic::Ordering;

/// Texture cache for storing GPU textures loaded by scripts.
struct TextureCache {
    textures: Vec<Texture2D>,
    fallback_texture: Option<Texture2D>,
}

impl TextureCache {
    const fn new() -> Self {
        Self {
            textures: Vec::new(),
            fallback_texture: None,
        }
    }

    fn get_fallback(&mut self) -> Option<Texture2D> {
        if let Some(ref tex) = self.fallback_texture {
            return Some(tex.clone());
        }
        let tex = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Texture2D::from_rgba8(1, 1, &[255, 255, 255, 255])
        }))
        .ok()?;
        self.fallback_texture = Some(tex.clone());
        Some(tex)
    }

    fn load(&mut self, path: &str) -> i64 {
        match std::fs::read(path) {
            Ok(bytes) => {
                let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    Texture2D::from_file_with_format(&bytes, None)
                }));
                match res {
                    Ok(tex) => {
                        self.textures.push(tex);
                        self.textures.len() as i64
                    }
                    Err(_) => 0,
                }
            }
            Err(err) => {
                eprintln!("[aipo-game-host] warning: could not load texture '{path}': {err}");
                0
            }
        }
    }

    fn get(&mut self, id: i64) -> Option<Texture2D> {
        if id > 0 && (id as usize) <= self.textures.len() {
            Some(self.textures[(id - 1) as usize].clone())
        } else {
            self.get_fallback()
        }
    }
}

static TEXTURE_CACHE: Mutex<TextureCache> = Mutex::new(TextureCache::new());

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct IconKey {
    path_data: String,
    size_px: u32,
    stroke_th_fixed: u32,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

/// Maximum number of distinct rasterized icon textures retained on the GPU.
///
/// The cache key includes the RGBA color, so a theme swap alone enumerates a new
/// key per icon. Without a bound, switching themes repeatedly grows GPU memory
/// without limit. 512 entries covers every icon in the built-in registry across
/// several themes and sizes.
const ICON_CACHE_CAPACITY: usize = 512;

struct IconCache {
    /// LRU-ordered entries. Front is most-recently used, back is eviction victim.
    ///
    /// A hand-rolled list keeps the dependency surface at zero and the working
    /// set small enough that O(n) eviction is cheaper than maintaining a
    /// `HashMap` plus intrusive list.
    entries: std::collections::VecDeque<(IconKey, Texture2D)>,
}

impl IconCache {
    fn new() -> Self {
        Self {
            entries: std::collections::VecDeque::with_capacity(ICON_CACHE_CAPACITY),
        }
    }

    /// Returns the cached texture for `key`, promoting it to most-recently used.
    fn touch(&mut self, key: &IconKey) -> Option<Texture2D> {
        let idx = self.entries.iter().position(|(k, _)| k == key)?;
        let entry = self.entries.remove(idx)?;
        let tex = entry.1.clone();
        self.entries.push_front(entry);
        Some(tex)
    }

    /// Inserts `key`/`tex`, evicting the least-recently used entry if full.
    fn insert(&mut self, key: IconKey, tex: Texture2D) {
        if self.entries.len() >= ICON_CACHE_CAPACITY
            && let Some(victim) = self.entries.pop_back()
        {
            debug_assert!(std::mem::size_of_val(&victim) > 0);
        }
        self.entries.push_front((key, tex));
    }

    /// Number of retained icon textures. Exposed for the cache-eviction test.
    fn len(&self) -> usize {
        self.entries.len()
    }

    fn get_or_render(
        &mut self,
        path_data: &str,
        size_px: u32,
        stroke_width: f32,
        color: Color,
    ) -> Option<Texture2D> {
        let size_px = size_px.clamp(8, 256);
        let key = IconKey {
            path_data: path_data.to_string(),
            size_px,
            stroke_th_fixed: (stroke_width * 10.0) as u32,
            r: (color.r * 255.0).clamp(0.0, 255.0) as u8,
            g: (color.g * 255.0).clamp(0.0, 255.0) as u8,
            b: (color.b * 255.0).clamp(0.0, 255.0) as u8,
            a: (color.a * 255.0).clamp(0.0, 255.0) as u8,
        };

        if let Some(tex) = self.touch(&key) {
            return Some(tex);
        }

        let mut pb = tiny_skia::PathBuilder::new();
        for segment in svgtypes::SimplifyingPathParser::from(path_data) {
            match segment {
                Ok(svgtypes::SimplePathSegment::MoveTo { x, y }) => pb.move_to(x as f32, y as f32),
                Ok(svgtypes::SimplePathSegment::LineTo { x, y }) => pb.line_to(x as f32, y as f32),
                Ok(svgtypes::SimplePathSegment::CurveTo {
                    x1,
                    y1,
                    x2,
                    y2,
                    x,
                    y,
                }) => pb.cubic_to(
                    x1 as f32, y1 as f32, x2 as f32, y2 as f32, x as f32, y as f32,
                ),
                Ok(svgtypes::SimplePathSegment::Quadratic { x1, y1, x, y }) => {
                    pb.quad_to(x1 as f32, y1 as f32, x as f32, y as f32)
                }
                Ok(svgtypes::SimplePathSegment::ClosePath) => pb.close(),
                Err(_) => {}
            }
        }

        let path = pb.finish()?;
        let bounds = path.bounds();
        let max_dim = bounds.width().max(bounds.height()).max(1.0);
        let base_extent = if max_dim <= 24.0 { 24.0 } else { max_dim };
        let scale = (size_px as f32) / base_extent;
        let transform = tiny_skia::Transform::from_scale(scale, scale);

        let mut pixmap = tiny_skia::Pixmap::new(size_px, size_px)?;
        let mut paint = tiny_skia::Paint::default();
        paint.set_color(tiny_skia::Color::from_rgba8(key.r, key.g, key.b, key.a));
        paint.anti_alias = true;

        if stroke_width > 0.0 {
            let stroke = tiny_skia::Stroke {
                width: stroke_width,
                line_cap: tiny_skia::LineCap::Round,
                line_join: tiny_skia::LineJoin::Round,
                ..Default::default()
            };
            pixmap.stroke_path(&path, &paint, &stroke, transform, None);
        } else {
            pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, transform, None);
        }

        let tex = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let t = Texture2D::from_rgba8(size_px as u16, size_px as u16, pixmap.data());
            t.set_filter(FilterMode::Linear);
            t
        }))
        .ok()?;

        self.insert(key, tex.clone());
        Some(tex)
    }
}

static ICON_CACHE: Mutex<Option<IconCache>> = Mutex::new(None);

/// Returns the number of icon textures currently retained in the LRU cache.
///
/// Zero when no icon has been rasterized yet. Used by the cache-eviction test.
pub fn icon_cache_len() -> usize {
    ICON_CACHE
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(IconCache::len))
        .unwrap_or(0)
}

// --- Zoe Typography Engine (Inter Font) ---

/// CPU-side fontdue font for headless-safe text measurement.
static ZOE_FONTDUE: std::sync::OnceLock<fontdue::Font> = std::sync::OnceLock::new();

/// Returns the Inter font instance for CPU-side text measurement (headless-safe).
fn get_zoe_fontdue() -> &'static fontdue::Font {
    ZOE_FONTDUE.get_or_init(|| {
        let bytes = include_bytes!("../assets/fonts/Inter.ttf");
        fontdue::Font::from_bytes(bytes as &[u8], fontdue::FontSettings::default())
            .expect("Failed to load Inter font from embedded bytes")
    })
}

static ZOE_FONT_INITIALIZED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Initializes the Inter font for GPU rendering via macroquad.
/// Must be called from the macroquad main loop (not in headless tests).
/// Safe to call multiple times — only initializes once.
pub fn init_zoe_font() {
    if ZOE_FONT_INITIALIZED.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let bytes = include_bytes!("../assets/fonts/Inter.ttf");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        load_ttf_font_from_bytes(bytes).ok()
    }));
    if let Ok(Some(font)) = result {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            set_default_font(font);
        }));
        ZOE_FONT_INITIALIZED.store(true, std::sync::atomic::Ordering::Relaxed);
        println!("[aipo-game-host] Inter font loaded — subpixel typography active");
    }
}

// --- Zoe GPU SDF Shader Engine (M14) ---
//
// M14 replaces the M9 shader, which had four mathematical defects:
//
//   1. Anti-aliasing used a hardcoded 1.0px band (`clamp(0.5 - dist, 0, 1)`).
//      That is only correct at scale 1:1. Under a viewport camera zoom the
//      distance-per-pixel changes and the edge reads as too fat or too thin.
//      Fixed with a zoom-derived `u_pixel_scale` uniform rather than
//      `fwidth`/`dFdx`: the shader stays on GLSL ES 100 and needs no
//      `GL_OES_standard_derivatives` extension, so it works on every backend
//      the host already supports, including WebGL1.
//
//   2. The border was composited with `mix` over a single SDF
//      (`b_dist = dist + border_width`). Adding the border width to the SDF
//      destroys the `min(max(q.x,q.y),0.0)` term, so the inner corner radius
//      came out wrong, and `mix` is not a correct `over` when the fill is
//      translucent. Now: two independent SDFs, alpha-premultiplied output.
//
//   3. There was no shadow at all; overlays faked one with a hard offset
//      rectangle. Now: two stacked soft-shadow layers with CSS `over` order
//      and gamma compensation, which is what makes a translucent black shadow
//      land at its intended darkness instead of at roughly half of it.
//
//   4. `sd_rounded_box` took a single radius. Now per-corner radii, so
//      asymmetric shapes ("squircles") are expressible.
//
// Note on MSAA: these quads are geometrically full-rasterized and all edge
// anti-aliasing is analytic in the fragment shader, so hardware MSAA adds
// nothing here. Correctness comes from `u_pixel_scale`, not from sample count.

const SDF_VERTEX_SHADER: &str = r#"#version 100
attribute vec3 position;
attribute vec4 color0;
attribute vec2 texcoord;

varying lowp vec2 uv;
varying lowp vec4 color;

uniform mat4 Model;
uniform mat4 Projection;

void main() {
    gl_Position = Projection * Model * vec4(position, 1.0);
    uv = texcoord;
    color = color0 / 255.0;
}
"#;

const SDF_FRAGMENT_SHADER: &str = r#"#version 100
precision mediump float;

varying lowp vec2 uv;
varying lowp vec4 color;

// Size of the quad actually rasterized, in pixels. Larger than the box when
// padding is reserved for an outward focus ring or a blurred shadow.
uniform vec2 u_quad_size;

// Half extents of the BOX (not the quad), in pixels. Rounded corners, the
// border and the shadow are all measured from this.
uniform vec2 u_box_half;

// Per-corner radii, clockwise from top-left: (tl, tr, br, bl).
uniform vec4 u_radii;

uniform float u_pixel_scale;     // 1.0 at 1:1, 1/zoom under a camera
uniform float u_border_width;    // 0 disables the border branch
uniform vec4  u_border_color;
uniform float u_inner_highlight; // 0 disables the chamfer light
uniform vec4  u_shadow1;         // (dy, blur, spread, alpha)
uniform vec4  u_shadow2;         // (dy, blur, spread, alpha)

// Focus ring. 0 width disables the branch. The ring is drawn in the SAME pass
// as the fill, border and shadow, so a focused widget costs no extra draw call.
uniform float u_focus_width;    // 0 disables the ring
uniform vec4  u_focus_color;

// Distance between the box boundary and the ring, in pixels. A ring flush
// against the border reads as a thicker border, not as a focus indicator.
// Must stay equal to `FOCUS_GAP_PX` in this file.
const float FOCUS_GAP = 1.5;

// Signed distance to a rounded box with independent per-corner radii.
float sd_rounded_box(vec2 p, vec2 b, vec4 r) {
    float r_top = (p.x > 0.0) ? r.y : r.x;  // tr : tl
    float r_bot = (p.x > 0.0) ? r.z : r.w;  // br : bl
    float rd    = (p.y < 0.0) ? r_top : r_bot;
    vec2 q = abs(p) - b + vec2(rd);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2(0.0))) - rd;
}

// Coverage of one soft-shadow layer at box-local pixel position `p`.
float shadow_layer(vec2 p, vec2 half_size, vec4 radii, vec4 layer) {
    if (layer.w <= 0.0) { return 0.0; }
    float d = sd_rounded_box(p - vec2(0.0, layer.x), half_size, radii) - layer.z;
    float blur = max(layer.y, 0.5);
    return (1.0 - smoothstep(-blur, blur, d)) * layer.w;
}

void main() {
    vec2 quad_half = u_quad_size * 0.5;
    // Pixel position relative to the BOX center. The quad is centered on the
    // box, so quad-relative and box-relative coincide.
    vec2 p = uv * u_quad_size - quad_half;

    vec2 box_half = u_box_half;
    float max_r = min(box_half.x, box_half.y);
    vec4 radii = clamp(u_radii, vec4(0.0), vec4(max_r));

    float d = sd_rounded_box(p, box_half, radii);

    // (1) FIX: AA width derived from the device scale instead of a fixed 1.0px.
    //     For a true normalized SDF the gradient magnitude is ~1, so the
    //     per-pixel distance is exactly 1/u_pixel_scale.
    float aa = max(1.0 / max(u_pixel_scale, 0.0001), 0.5);
    float inside = 1.0 - smoothstep(-aa, 0.0, d);

    vec4 col = color;

    // (3) NEW: two soft-shadow layers, composited in CSS `over` order.
    if (u_shadow1.w > 0.0 || u_shadow2.w > 0.0) {
        float a1 = shadow_layer(p, box_half, radii, u_shadow1);
        float a2 = shadow_layer(p, box_half, radii, u_shadow2);
        float a_css = 1.0 - (1.0 - a1) * (1.0 - a2);
        // Gamma compensation: a translucent BLACK shadow composited in
        // encoded sRGB needs a 2.2 power, otherwise an alpha of 0.05 lands
        // at roughly half its intended darkness on light surfaces.
        float sa = 1.0 - pow(1.0 - a_css, 2.2);
        col = vec4(0.0, 0.0, 0.0, sa);
    }

    // Fill over the shadow: never shrink the alpha already contributed by
    // the shadow, so a translucent panel still casts a readable shadow.
    if (color.a > 0.0) {
        float fa = color.a * inside;
        col = vec4(mix(col.rgb, color.rgb, fa), max(col.a, fa));
    }

    // (2) FIX: border from an INDEPENDENT SDF, then premultiplied-alpha
    //     output. `abs(d) - w/2` places the stroke centred on the box
    //     boundary, so it follows the same curvature without a seam.
    float bw = u_border_width;
    if (bw > 0.0 && u_border_color.a > 0.0) {
        float stroke_d = abs(d) - bw * 0.5;
        float sa = (1.0 - smoothstep(-aa, aa, stroke_d)) * u_border_color.a;
        float fill_a = color.a * inside;
        float out_a = sa + fill_a * (1.0 - sa);
        vec3 out_rgb = (out_a > 1e-5)
            ? (u_border_color.rgb * sa + color.rgb * fill_a * (1.0 - sa)) / out_a
            : u_border_color.rgb;
        col = vec4(out_rgb, out_a);
    }

    // Focus ring, placed OUTSIDE the box boundary. Its own SDF, so it follows
    // the same corner curvature as the border instead of being a screen-space
    // outline.
    //
    // It composites last, after the fill and border. It used to run first and
    // was then erased: the border block assigned `col` from scratch, and in
    // the ring's annulus neither the 1px stroke nor the fill has any coverage,
    // so the assignment produced alpha 0 and the `discard` below threw the
    // fragment away. The ring was drawn into nothing and no widget ever showed
    // a focus indicator. Compositing it here overlaps nothing, because the
    // ring starts `FOCUS_GAP` outside a box the fill never leaves.
    if (u_focus_width > 0.0 && u_focus_color.a > 0.0) {
        float ring_mid = FOCUS_GAP + u_focus_width * 0.5;
        float fr = abs(d - ring_mid) - u_focus_width * 0.5;
        float fa = (1.0 - smoothstep(-aa, aa, fr)) * u_focus_color.a;
        float out_a = fa + col.a * (1.0 - fa);
        col = vec4(
            (out_a > 1e-5) ? (u_focus_color.rgb * fa + col.rgb * col.a * (1.0 - fa)) / out_a
                           : u_focus_color.rgb,
            out_a
        );
    }

    // 1px top inner highlight: simulates ambient light catching the chamfer.
    if (u_inner_highlight > 0.0 && d <= -aa && col.a > 0.0) {
        float top_y = uv.y * u_quad_size.y - quad_half.y;
        float from_top = (box_half.y) - top_y;
        if (from_top <= (u_border_width + 1.2) && from_top >= 0.0) {
            col.rgb += vec3(0.12 * u_inner_highlight);
        }
    }

    if (col.a <= 0.0) {
        discard;
    }
    gl_FragColor = col;
}
"#;

static ZOE_SDF_MATERIAL: Mutex<Option<Material>> = Mutex::new(None);
static ZOE_SDF_INITIALIZED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// SDF quads that reached a real GPU draw call.
static SDF_DRAWS_ISSUED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// SDF quads rejected before drawing (off-screen, zero-area, or fully
/// transparent). Exposed for tests and for the perf harness.
static SDF_DRAWS_CULLED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Per-instance SDF styling, remembered so consecutive quads only re-upload
/// the uniforms that actually changed. Reset whenever the GL context changes.
#[derive(Clone, Copy, PartialEq)]
struct SdfUniformState {
    quad_size: (f32, f32),
    box_half: (f32, f32),
    radii: [f32; 4],
    pixel_scale: f32,
    border_width: f32,
    border_color: (f32, f32, f32, f32),
    inner_highlight: f32,
    shadow1: (f32, f32, f32, f32),
    shadow2: (f32, f32, f32, f32),
    focus_width: f32,
    focus_color: (f32, f32, f32, f32),
}

impl SdfUniformState {
    const fn empty() -> Self {
        Self {
            // `f32::NAN` never compares equal, so the first quad after a reset
            // always uploads every uniform.
            quad_size: (f32::NAN, f32::NAN),
            box_half: (f32::NAN, f32::NAN),
            radii: [f32::NAN; 4],
            pixel_scale: f32::NAN,
            border_width: f32::NAN,
            border_color: (f32::NAN, f32::NAN, f32::NAN, f32::NAN),
            inner_highlight: f32::NAN,
            shadow1: (f32::NAN, f32::NAN, f32::NAN, f32::NAN),
            shadow2: (f32::NAN, f32::NAN, f32::NAN, f32::NAN),
            focus_width: f32::NAN,
            focus_color: (f32::NAN, f32::NAN, f32::NAN, f32::NAN),
        }
    }
}

static LAST_SDF_UNIFORMS: Mutex<SdfUniformState> = Mutex::new(SdfUniformState::empty());

/// Number of SDF quads that reached the GPU since process start.
pub fn sdf_draws_issued() -> u64 {
    SDF_DRAWS_ISSUED.load(Ordering::Relaxed)
}

/// Number of SDF quads culled before drawing since process start.
pub fn sdf_draws_culled() -> u64 {
    SDF_DRAWS_CULLED.load(Ordering::Relaxed)
}

/// Clears the remembered uniform state. Must be called whenever the GL context
/// is recreated, because a new context invalidates cached uniform locations.
fn reset_sdf_uniform_state() {
    if let Ok(mut prev) = LAST_SDF_UNIFORMS.lock() {
        *prev = SdfUniformState::empty();
    }
}

/// Soft-shadow elevation levels.
///
/// `(dy, blur, spread, alpha)` per level, mirroring Tailwind's discrete
/// anchors. `SHADOW_ANCHORS` is interpolated by `shadow_layers` so a caller
/// can pass a continuous elevation and still get a smooth transition between
/// the two CSS layers. Levels above the last anchor scale the `xl` geometry
/// proportionally.
///
/// These literals are the contract with `SDF_FRAGMENT_SHADER`; the parity test
/// `test_sdf_elevation_anchor_parity` reads the shader source and asserts each
/// row appears verbatim, so the two cannot drift apart.
pub const SHADOW_ANCHORS: [[f32; 4]; 5] = [
    [2.0, 3.0, 0.0, 0.05],    // 0 = shadow-xs
    [4.0, 6.0, -1.0, 0.08],   // 1 = shadow-sm
    [12.0, 16.0, -3.0, 0.10], // 2 = shadow-md
    [24.0, 32.0, -6.0, 0.14], // 3 = shadow-lg
    [40.0, 48.0, -9.0, 0.18], // 4 = shadow-xl
];

/// The raw elevation anchor table. Exposed so tests can assert the invariants
/// the interpolation in `shadow_master` depends on.
pub fn shadow_anchors() -> &'static [[f32; 4]] {
    &SHADOW_ANCHORS
}

/// Gap between a box edge and its focus ring, in logical pixels.
///
/// Duplicated in `SDF_FRAGMENT_SHADER` as `FOCUS_GAP`, because the shader is
/// compiled from a raw string and cannot reference Rust constants. The parity
/// test `test_sdf_focus_gap_parity` fails if the two drift apart.
pub const FOCUS_GAP_PX: f32 = 1.5;

/// Interpolates the master shadow curve at a continuous elevation level.
///
/// Returns `(dy, blur, spread, alpha)` for the requested level. Between
/// anchors the values are linearly interpolated, so raising elevation
/// animates smoothly instead of popping at integer levels. Past the last
/// anchor the geometry scales proportionally so very high elevations keep
/// growing rather than clamping flat.
pub fn shadow_master(elevation: f32) -> [f32; 4] {
    let n = SHADOW_ANCHORS.len() as f32;
    if elevation >= n {
        let t = 1.0 + (elevation - n) * 0.25;
        let a = SHADOW_ANCHORS[SHADOW_ANCHORS.len() - 1];
        return [a[0] * t, a[1] * t, a[2] * t, a[3]];
    }
    let idx = (elevation.max(0.0) as usize).min(SHADOW_ANCHORS.len() - 1);
    let frac = elevation - idx as f32;
    let a = SHADOW_ANCHORS[idx];
    let b = SHADOW_ANCHORS[(idx + 1).min(SHADOW_ANCHORS.len() - 1)];
    [
        a[0] + (b[0] - a[0]) * frac,
        a[1] + (b[1] - a[1]) * frac,
        a[2] + (b[2] - a[2]) * frac,
        a[3] + (b[3] - a[3]) * frac,
    ]
}

/// Derives the two shadow layers from a continuous elevation.
///
/// Returns `(contact, ambient)`, each `(dy, blur, spread, alpha)`.
///
/// Two layers rather than one because a single soft shadow reads as a blur;
/// a tight, darker *contact* layer under a broad, lighter *ambient* layer is
/// what actually reads as depth. The contact layer is the tighter and more
/// opaque of the two, so it is drawn first in the shader's `over` chain.
///
/// An elevation of `0.0` returns two transparent layers, so the shader skips
/// the branch entirely and unelevated widgets cost nothing.
pub fn shadow_layers(elevation: f32) -> ([f32; 4], [f32; 4]) {
    const TRANSPARENT: [f32; 4] = [0.0, 0.0, 0.0, 0.0];
    if elevation <= 0.0 {
        return (TRANSPARENT, TRANSPARENT);
    }

    let m = shadow_master(elevation);
    let (dy, blur, spread, alpha) = (m[0], m[1], m[2], m[3]);

    // Contact: tight and denser, hugging the surface.
    let contact = [dy * 0.35, blur * 0.35, spread * 0.5, (alpha * 1.4).min(1.0)];
    // Ambient: the full master curve, lighter and broader.
    let ambient = [dy, blur, spread, (alpha * 0.65).min(1.0)];

    (contact, ambient)
}

/// Initializes the GPU SDF shader material for analytical UI rendering.
pub fn init_zoe_shaders() {
    if ZOE_SDF_INITIALIZED.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    // A fresh context invalidates the remembered uniform state, so force the
    // first quad to upload everything.
    reset_sdf_uniform_state();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let shader = ShaderSource::Glsl {
            vertex: SDF_VERTEX_SHADER,
            fragment: SDF_FRAGMENT_SHADER,
        };
        let params = MaterialParams {
            pipeline_params: PipelineParams {
                color_blend: Some(BlendState::new(
                    Equation::Add,
                    BlendFactor::Value(BlendValue::SourceAlpha),
                    BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
                )),
                alpha_blend: Some(BlendState::new(
                    Equation::Add,
                    BlendFactor::Value(BlendValue::SourceAlpha),
                    BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
                )),
                ..Default::default()
            },
            uniforms: vec![
                UniformDesc::new("u_quad_size", UniformType::Float2),
                UniformDesc::new("u_box_half", UniformType::Float2),
                UniformDesc::new("u_radii", UniformType::Float4),
                UniformDesc::new("u_pixel_scale", UniformType::Float1),
                UniformDesc::new("u_border_width", UniformType::Float1),
                UniformDesc::new("u_border_color", UniformType::Float4),
                UniformDesc::new("u_inner_highlight", UniformType::Float1),
                UniformDesc::new("u_shadow1", UniformType::Float4),
                UniformDesc::new("u_shadow2", UniformType::Float4),
                UniformDesc::new("u_focus_width", UniformType::Float1),
                UniformDesc::new("u_focus_color", UniformType::Float4),
            ],
            textures: vec![],
        };
        load_material(shader, params).ok()
    }));
    if let Ok(Some(mat)) = result {
        if let Ok(mut lock) = ZOE_SDF_MATERIAL.lock() {
            *lock = Some(mat);
        }
        ZOE_SDF_INITIALIZED.store(true, std::sync::atomic::Ordering::Relaxed);
        println!(
            "[aipo-game-host] GPU SDF analytical UI shader active (M14: scaled AA, 2-layer shadow, premultiplied border, per-corner radii)"
        );
    }
}

// --- Headless-safe Macroquad Wrappers ---

fn safe_is_key_down(key: KeyCode) -> bool {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| is_key_down(key))).unwrap_or(false)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

static CLIP_STACK: Mutex<Vec<ClipRect>> = Mutex::new(Vec::new());

fn current_clip() -> Option<ClipRect> {
    CLIP_STACK
        .lock()
        .ok()
        .and_then(|stack| stack.last().copied())
}

fn safe_mouse_wheel() -> (f32, f32) {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(mouse_wheel)).unwrap_or((0.0, 0.0))
}

fn safe_is_key_pressed(key: KeyCode) -> bool {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| is_key_pressed(key))).unwrap_or(false)
}

fn safe_get_char_pressed() -> Option<char> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(get_char_pressed)).unwrap_or(None)
}

fn safe_mouse_position() -> (f32, f32) {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(mouse_position)).unwrap_or((0.0, 0.0))
}

fn safe_is_mouse_button_down(mb: MouseButton) -> bool {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| is_mouse_button_down(mb)))
        .unwrap_or(false)
}

fn safe_screen_width() -> f32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(screen_width)).unwrap_or(840.0)
}

fn safe_screen_height() -> f32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(screen_height)).unwrap_or(680.0)
}

fn safe_frame_time() -> f32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(get_frame_time)).unwrap_or(1.0 / 60.0)
}

fn safe_draw_rect(x: f32, y: f32, w: f32, h: f32, color: Color) {
    let (draw_x, draw_y, draw_w, draw_h) = if let Some(clip) = current_clip() {
        let x1 = x.max(clip.x);
        let y1 = y.max(clip.y);
        let x2 = (x + w).min(clip.x + clip.w);
        let y2 = (y + h).min(clip.y + clip.h);
        if x2 <= x1 || y2 <= y1 {
            return;
        }
        (x1, y1, x2 - x1, y2 - y1)
    } else {
        (x, y, w, h)
    };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        draw_rectangle(draw_x, draw_y, draw_w, draw_h, color)
    }));
}

fn safe_draw_rect_lines(x: f32, y: f32, w: f32, h: f32, th: f32, color: Color) {
    if let Some(clip) = current_clip() {
        let x1 = x.max(clip.x);
        let y1 = y.max(clip.y);
        let x2 = (x + w).min(clip.x + clip.w);
        let y2 = (y + h).min(clip.y + clip.h);
        if x2 <= x1 || y2 <= y1 {
            return;
        }
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        draw_rectangle_lines(x, y, w, h, th, color)
    }));
}

fn clip_line(
    mut x0: f32,
    mut y0: f32,
    mut x1: f32,
    mut y1: f32,
    clip: ClipRect,
) -> Option<(f32, f32, f32, f32)> {
    const INSIDE: u8 = 0;
    const LEFT: u8 = 1;
    const RIGHT: u8 = 2;
    const BOTTOM: u8 = 4;
    const TOP: u8 = 8;

    let xmin = clip.x;
    let xmax = clip.x + clip.w;
    let ymin = clip.y;
    let ymax = clip.y + clip.h;

    let compute_outcode = |x: f32, y: f32| -> u8 {
        let mut code = INSIDE;
        if x < xmin {
            code |= LEFT;
        } else if x > xmax {
            code |= RIGHT;
        }
        if y < ymin {
            code |= TOP;
        } else if y > ymax {
            code |= BOTTOM;
        }
        code
    };

    let mut code0 = compute_outcode(x0, y0);
    let mut code1 = compute_outcode(x1, y1);

    loop {
        if (code0 | code1) == 0 {
            return Some((x0, y0, x1, y1));
        } else if (code0 & code1) != 0 {
            return None;
        } else {
            let code_out = if code0 != 0 { code0 } else { code1 };
            let mut x = 0.0;
            let mut y = 0.0;

            if (code_out & TOP) != 0 {
                x = x0 + (x1 - x0) * (ymin - y0) / (y1 - y0);
                y = ymin;
            } else if (code_out & BOTTOM) != 0 {
                x = x0 + (x1 - x0) * (ymax - y0) / (y1 - y0);
                y = ymax;
            } else if (code_out & RIGHT) != 0 {
                y = y0 + (y1 - y0) * (xmax - x0) / (x1 - x0);
                x = xmax;
            } else if (code_out & LEFT) != 0 {
                y = y0 + (y1 - y0) * (xmin - x0) / (x1 - x0);
                x = xmin;
            }

            if code_out == code0 {
                x0 = x;
                y0 = y;
                code0 = compute_outcode(x0, y0);
            } else {
                x1 = x;
                y1 = y;
                code1 = compute_outcode(x1, y1);
            }
        }
    }
}

fn safe_draw_line(x1: f32, y1: f32, x2: f32, y2: f32, th: f32, color: Color) {
    let (cx1, cy1, cx2, cy2) = if let Some(clip) = current_clip() {
        if let Some(clipped) = clip_line(x1, y1, x2, y2, clip) {
            clipped
        } else {
            return;
        }
    } else {
        (x1, y1, x2, y2)
    };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        draw_line(cx1, cy1, cx2, cy2, th, color)
    }));
}

fn safe_draw_circle(cx: f32, cy: f32, radius: f32, color: Color) {
    if let Some(clip) = current_clip()
        && (cx + radius < clip.x
            || cx - radius > (clip.x + clip.w)
            || cy + radius < clip.y
            || cy - radius > (clip.y + clip.h))
    {
        return;
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        draw_circle(cx, cy, radius, color)
    }));
}

fn safe_draw_triangle(v1: Vec2, v2: Vec2, v3: Vec2, color: Color) {
    if let Some(clip) = current_clip() {
        let min_x = v1.x.min(v2.x).min(v3.x);
        let max_x = v1.x.max(v2.x).max(v3.x);
        let min_y = v1.y.min(v2.y).min(v3.y);
        let max_y = v1.y.max(v2.y).max(v3.y);
        if max_x < clip.x
            || min_x > (clip.x + clip.w)
            || max_y < clip.y
            || min_y > (clip.y + clip.h)
        {
            return;
        }
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        draw_triangle(v1, v2, v3, color);
    }));
}

fn safe_draw_triangle_lines(v1: Vec2, v2: Vec2, v3: Vec2, th: f32, color: Color) {
    if let Some(clip) = current_clip() {
        let min_x = v1.x.min(v2.x).min(v3.x);
        let max_x = v1.x.max(v2.x).max(v3.x);
        let min_y = v1.y.min(v2.y).min(v3.y);
        let max_y = v1.y.max(v2.y).max(v3.y);
        if max_x < clip.x
            || min_x > (clip.x + clip.w)
            || max_y < clip.y
            || min_y > (clip.y + clip.h)
        {
            return;
        }
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        draw_triangle_lines(v1, v2, v3, th, color);
    }));
}

fn safe_draw_round_rect(x: f32, y: f32, w: f32, h: f32, radius: f32, color: Color) {
    let r = radius.min(w * 0.5).min(h * 0.5).max(0.0);
    if r < 1.0 {
        safe_draw_rect(x, y, w, h, color);
        return;
    }
    // Main body rectangles (clipped via safe_draw_rect)
    safe_draw_rect(x + r, y, w - 2.0 * r, h, color);
    safe_draw_rect(x, y + r, r, h - 2.0 * r, color);
    safe_draw_rect(x + w - r, y + r, r, h - 2.0 * r, color);
    // 4 corner circles (clipped via safe_draw_circle)
    safe_draw_circle(x + r, y + r, r, color);
    safe_draw_circle(x + w - r, y + r, r, color);
    safe_draw_circle(x + r, y + h - r, r, color);
    safe_draw_circle(x + w - r, y + h - r, r, color);
}

fn safe_draw_round_rect_lines(x: f32, y: f32, w: f32, h: f32, radius: f32, th: f32, color: Color) {
    let r = radius.min(w * 0.5).min(h * 0.5).max(0.0);
    if r < 1.0 {
        safe_draw_rect_lines(x, y, w, h, th, color);
        return;
    }
    // 4 straight edges (clipped via safe_draw_line)
    safe_draw_line(x + r, y, x + w - r, y, th, color);
    safe_draw_line(x + r, y + h, x + w - r, y + h, th, color);
    safe_draw_line(x, y + r, x, y + h - r, th, color);
    safe_draw_line(x + w, y + r, x + w, y + h - r, th, color);

    // 4 corner arcs approximation (4 segments each)
    let steps = 4;
    let arc = |cx: f32, cy: f32, start_ang: f32| {
        let step_ang = std::f32::consts::FRAC_PI_2 / (steps as f32);
        for i in 0..steps {
            let a1 = start_ang + (i as f32) * step_ang;
            let a2 = a1 + step_ang;
            let p1x = cx + r * a1.cos();
            let p1y = cy + r * a1.sin();
            let p2x = cx + r * a2.cos();
            let p2y = cy + r * a2.sin();
            safe_draw_line(p1x, p1y, p2x, p2y, th, color);
        }
    };
    use std::f32::consts::PI;
    arc(x + w - r, y + h - r, 0.0); // bottom-right: 0 to PI/2
    arc(x + r, y + h - r, PI * 0.5); // bottom-left: PI/2 to PI
    arc(x + r, y + r, PI); // top-left: PI to 3PI/2
    arc(x + w - r, y + r, PI * 1.5); // top-right: 3PI/2 to 2PI
}

/// Geometry and paint of one analytical SDF quad.
///
/// Grouped into a struct so the draw path stays under clippy's argument-count
/// limit and so the same shape is reused by the M15 instanced batch path.
///
/// `radii` is per-corner `(tl, tr, br, bl)`; `radius` is the shorthand that
/// fills all four. `elevation` selects the soft-shadow level and `focus_width`
/// draws the focus ring in the same pass, so a widget with a border, a shadow
/// and a focus ring still costs exactly one draw call.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SdfQuad {
    /// Left edge, in logical pixels.
    pub x: f32,
    /// Top edge, in logical pixels.
    pub y: f32,
    /// Fill width, in logical pixels.
    pub w: f32,
    /// Fill height, in logical pixels.
    pub h: f32,
    /// Uniform corner radius. Ignored per corner where `radii` is positive.
    pub radius: f32,
    /// Per-corner radii `(tl, tr, br, bl)`; `0.0` means "use `radius`".
    pub radii: [f32; 4],
    /// Border thickness, in logical pixels. `0.0` disables the border.
    pub border_width: f32,
    /// Border color. Fully transparent disables the border.
    pub border_color: Color,
    /// Strength of the inner top highlight that fakes a Bevel.
    pub inner_highlight: f32,
    /// Fill color. Fully transparent means "paint nothing".
    pub bg_color: Color,
    /// Elevation level selecting the soft-shadow curve. `0.0` casts no shadow.
    pub elevation: f32,
    /// Focus-ring thickness, in logical pixels. `0.0` disables the ring.
    pub focus_width: f32,
    /// Focus-ring color. Fully transparent disables the ring.
    pub focus_color: Color,
}

impl SdfQuad {
    /// Uniform corner radii. Used when no per-corner shape is supplied.
    fn uniform_radii(&self) -> [f32; 4] {
        [self.radius; 4]
    }

    /// Per-corner radii, falling back to the uniform radius per corner.
    fn effective_radii(&self) -> [f32; 4] {
        let uniform = self.uniform_radii();
        let mut out = [0.0f32; 4];
        for i in 0..4 {
            out[i] = if self.radii[i] > 0.0 {
                self.radii[i]
            } else {
                uniform[i]
            };
        }
        out
    }

    /// Padding the rasterized quad needs on each side so the outward parts
    /// (shadow blur and an outward focus ring) are not clipped away.
    ///
    /// Public because three call sites must agree on it: the draw path grows
    /// the quad by this much, the clip-culling predicate tests against the
    /// grown rectangle, and tests assert the two have not drifted.
    pub fn padding(&self) -> f32 {
        let (s1, s2) = shadow_layers(self.elevation);
        let blur = s1[1].max(s2[1]);
        let spread = s1[2].min(s2[2]);
        // The ring sits `FOCUS_GAP` away from the box edge, so it reaches one
        // gap further out than its own thickness.
        let focus = if self.focus_width > 0.0 {
            self.focus_width + FOCUS_GAP_PX
        } else {
            0.0
        };
        (blur + spread.abs() + focus + 1.0).max(0.0)
    }
}

/// Number of physical pixels per logical (framebuffer) pixel.
///
/// The SDF anti-aliasing band is specified in logical pixels, so on a HiDPI
/// display one logical pixel covers `dpi_scale` physical pixels. Without this
/// the analytic edge would be `dpi_scale` times too thin on Retina/HiDPI
/// surfaces. Returns 1.0 headless, which is the identity case.
fn pixel_scale() -> f32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let s = macroquad::miniquad::window::dpi_scale();
        if s > 0.0 { s } else { 1.0 }
    }))
    .unwrap_or(1.0)
}

/// Why a quad can be skipped without touching the GPU.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum SdfCull {
    /// Entirely outside the active clip rectangle.
    OffScreen,
    /// Zero or negative rasterized area.
    ZeroArea,
    /// No fill, no border, no shadow and no focus ring: paints nothing.
    Invisible,
}

/// Decides whether a quad is worth a draw call.
///
/// Pure, so the decision can be tested without a GL context. `clip` is the
/// active clip rectangle, if any. `shadow_alpha` is the strongest shadow layer
/// alpha, which the caller derives from the elevation curve.
///
/// The padded rectangle is derived here rather than passed in, so a caller
/// cannot test a clip against a rectangle that differs from the one actually
/// rasterized.
pub fn sdf_cull_reason(
    quad: &SdfQuad,
    shadow_alpha: f32,
    clip: Option<ClipRect>,
) -> Option<SdfCull> {
    let pad = quad.padding();
    if quad.w <= 0.0 || quad.h <= 0.0 {
        return Some(SdfCull::ZeroArea);
    }
    if let Some(c) = clip {
        // A shadow and an outward focus ring bleed past the box, so the test
        // uses the grown rectangle: a box just outside the clip can still
        // paint inside it.
        let x = quad.x - pad;
        let y = quad.y - pad;
        if x + quad.w + pad * 2.0 < c.x
            || x > (c.x + c.w)
            || y + quad.h + pad * 2.0 < c.y
            || y > (c.y + c.h)
        {
            return Some(SdfCull::OffScreen);
        }
    }
    if quad.bg_color.a <= 0.0
        && quad.border_width <= 0.0
        && quad.border_color.a <= 0.0
        && shadow_alpha <= 0.0
        && quad.focus_width <= 0.0
    {
        return Some(SdfCull::Invisible);
    }
    None
}

fn safe_draw_sdf_rect(quad: &SdfQuad) {
    let (x, y, w, h) = (quad.x, quad.y, quad.w, quad.h);
    let radii = quad.effective_radii();
    let uniform_radius = quad.radius;
    let border_width = quad.border_width;
    let border_color = quad.border_color;
    let inner_highlight = quad.inner_highlight;
    let bg_color = quad.bg_color;
    let (shadow1, shadow2) = shadow_layers(quad.elevation);

    let pad = quad.padding();
    // The quad grows symmetrically about the box center; the shader works in
    // box-local coordinates so the offset between quad and box origin cancels.
    let quad_w = w + pad * 2.0;
    let quad_h = h + pad * 2.0;
    let quad_x = x - pad;
    let quad_y = y - pad;

    if sdf_cull_reason(quad, shadow1[3].max(shadow2[3]), current_clip()).is_some() {
        SDF_DRAWS_CULLED.fetch_add(1, Ordering::Relaxed);
        return;
    }

    let drawn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if let Ok(lock) = ZOE_SDF_MATERIAL.lock()
            && let Some(ref mat) = *lock
        {
            let max_r = w.min(h) * 0.5;
            let clamped = [
                radii[0].min(max_r),
                radii[1].min(max_r),
                radii[2].min(max_r),
                radii[3].min(max_r),
            ];
            let focus_w = quad.focus_width;
            let focus_c = quad.focus_color;
            // Consecutive quads usually share most of their styling, so
            // only upload what actually changed. Each `set_uniform` is a GL
            // call, and a screen full of same-styled rows used to issue
            // nine per row even when nothing differed.
            let Ok(mut prev) = LAST_SDF_UNIFORMS.lock() else {
                return false;
            };
            if prev.quad_size != (quad_w, quad_h) {
                mat.set_uniform("u_quad_size", vec2(quad_w, quad_h));
                prev.quad_size = (quad_w, quad_h);
            }
            if prev.box_half != (w * 0.5, h * 0.5) {
                mat.set_uniform("u_box_half", vec2(w * 0.5, h * 0.5));
                prev.box_half = (w * 0.5, h * 0.5);
            }
            if prev.radii != clamped {
                mat.set_uniform(
                    "u_radii",
                    vec4(clamped[0], clamped[1], clamped[2], clamped[3]),
                );
                prev.radii = clamped;
            }
            let ps = pixel_scale();
            if prev.pixel_scale != ps {
                mat.set_uniform("u_pixel_scale", ps);
                prev.pixel_scale = ps;
            }
            if prev.border_width != border_width {
                mat.set_uniform("u_border_width", border_width);
                prev.border_width = border_width;
            }
            let bc = (
                border_color.r,
                border_color.g,
                border_color.b,
                border_color.a,
            );
            if prev.border_color != bc {
                mat.set_uniform("u_border_color", vec4(bc.0, bc.1, bc.2, bc.3));
                prev.border_color = bc;
            }
            if prev.inner_highlight != inner_highlight {
                mat.set_uniform("u_inner_highlight", inner_highlight);
                prev.inner_highlight = inner_highlight;
            }
            let s1 = (shadow1[0], shadow1[1], shadow1[2], shadow1[3]);
            if prev.shadow1 != s1 {
                mat.set_uniform("u_shadow1", vec4(s1.0, s1.1, s1.2, s1.3));
                prev.shadow1 = s1;
            }
            let s2 = (shadow2[0], shadow2[1], shadow2[2], shadow2[3]);
            if prev.shadow2 != s2 {
                mat.set_uniform("u_shadow2", vec4(s2.0, s2.1, s2.2, s2.3));
                prev.shadow2 = s2;
            }
            if prev.focus_width != focus_w {
                mat.set_uniform("u_focus_width", focus_w);
                prev.focus_width = focus_w;
            }
            let fc = (focus_c.r, focus_c.g, focus_c.b, focus_c.a);
            if prev.focus_color != fc {
                mat.set_uniform("u_focus_color", vec4(fc.0, fc.1, fc.2, fc.3));
                prev.focus_color = fc;
            }
            drop(prev);
            gl_use_material(mat);
            draw_rectangle(quad_x, quad_y, quad_w, quad_h, bg_color);
            gl_use_default_material();
            return true;
        }
        false
    }))
    .unwrap_or(false);

    if !drawn {
        // Software fallback: no shader available (headless tests, or a context
        // where material loading failed). Shadows are approximated with a
        // single offset layer, which is what the pre-M14 renderer did for
        // every overlay anyway.
        if quad.elevation > 0.0 {
            let offset = shadow1[0].max(2.0);
            safe_draw_round_rect(
                x,
                y + offset,
                w,
                h,
                uniform_radius,
                Color::new(0.0, 0.0, 0.0, shadow1[3] * 0.8),
            );
        }
        safe_draw_round_rect(x, y, w, h, uniform_radius, bg_color);
        if border_width > 0.0 {
            safe_draw_round_rect_lines(x, y, w, h, uniform_radius, border_width, border_color);
        }
        if quad.focus_width > 0.0 && quad.focus_color.a > 0.0 {
            let t = quad.focus_width;
            safe_draw_round_rect_lines(
                x - t,
                y - t,
                w + t * 2.0,
                h + t * 2.0,
                uniform_radius + t,
                t,
                quad.focus_color,
            );
        }
    }
}

fn safe_draw_text(text: &str, x: f32, y: f32, size: f32, color: Color) {
    if let Some(clip) = current_clip()
        && (y < clip.y || (y - size) > (clip.y + clip.h) || x > (clip.x + clip.w))
    {
        return;
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        draw_text(text, x, y, size, color)
    }));
}

fn safe_clear_background(color: Color) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| clear_background(color)));
}

fn safe_draw_texture(tex: Option<&Texture2D>, x: f32, y: f32, params: DrawTextureParams) {
    if let Some(t) = tex {
        if let Some(clip) = current_clip() {
            let dw = params.dest_size.map(|s| s.x).unwrap_or(t.width());
            let dh = params.dest_size.map(|s| s.y).unwrap_or(t.height());
            if x + dw < clip.x || x > (clip.x + clip.w) || y + dh < clip.y || y > (clip.y + clip.h)
            {
                return;
            }
        }
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            draw_texture_ex(t, x, y, WHITE, params)
        }));
    }
}

fn safe_macroquad_call<F: FnOnce() + std::panic::UnwindSafe>(f: F) {
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let _ = std::panic::catch_unwind(f);
    std::panic::set_hook(prev_hook);
}

fn safe_set_camera(tx: f32, ty: f32, zoom: f32, sw: f32, sh: f32) {
    safe_macroquad_call(|| {
        set_camera(&Camera2D {
            offset: vec2(0.0, 0.0),
            target: vec2(tx, ty),
            rotation: 0.0,
            zoom: vec2((2.0 / sw) * zoom, (2.0 / sh) * zoom),
            render_target: None,
            viewport: None,
        });
    });
}

fn safe_set_camera_viewport(
    tx: f32,
    ty: f32,
    zoom: f32,
    gl_x: i32,
    gl_y: i32,
    gl_w: i32,
    gl_h: i32,
) {
    let vw = (gl_w as f32).max(1.0);
    let vh = (gl_h as f32).max(1.0);
    safe_macroquad_call(|| {
        set_camera(&Camera2D {
            offset: vec2(0.0, 0.0),
            target: vec2(tx, ty),
            rotation: 0.0,
            zoom: vec2((2.0 / vw) * zoom, (2.0 / vh) * zoom),
            render_target: None,
            viewport: Some((gl_x, gl_y, gl_w, gl_h)),
        });
    });
}

fn safe_reset_camera() {
    safe_macroquad_call(set_default_camera);
}

// --- Helper Conversion Functions ---

fn to_f32(val: &Value) -> Result<f32, VmFault> {
    match val {
        Value::Float(f) => Ok(*f as f32),
        Value::Int(n) => Ok(*n as f32),
        other => Err(VmFault::TypeMismatch {
            expected: "Float or Int".to_string(),
            actual: other.type_name().to_string(),
        }),
    }
}

fn to_i64(val: &Value) -> Result<i64, VmFault> {
    match val {
        Value::Int(n) => Ok(*n),
        Value::Float(f) => Ok(*f as i64),
        other => Err(VmFault::TypeMismatch {
            expected: "Int".to_string(),
            actual: other.type_name().to_string(),
        }),
    }
}

fn to_bool(val: &Value) -> Result<bool, VmFault> {
    match val {
        Value::Bool(b) => Ok(*b),
        other => Err(VmFault::TypeMismatch {
            expected: "Bool".to_string(),
            actual: other.type_name().to_string(),
        }),
    }
}

fn to_string(val: &Value) -> Result<String, VmFault> {
    match val {
        Value::String(s) => Ok((**s).clone()),
        other => Ok(other.to_string()),
    }
}

/// Maps integer keycodes to Macroquad's KeyCode.
fn map_keycode(code: i64) -> Option<KeyCode> {
    match code {
        32 => Some(KeyCode::Space),
        256 => Some(KeyCode::Escape),
        257 => Some(KeyCode::Enter),
        258 => Some(KeyCode::Tab),
        259 => Some(KeyCode::Backspace),
        260 => Some(KeyCode::Insert),
        261 => Some(KeyCode::Delete),
        262 => Some(KeyCode::Right),
        263 => Some(KeyCode::Left),
        264 => Some(KeyCode::Down),
        265 => Some(KeyCode::Up),
        340 => Some(KeyCode::LeftShift),
        341 => Some(KeyCode::LeftControl),
        342 => Some(KeyCode::LeftAlt),
        344 => Some(KeyCode::RightShift),
        345 => Some(KeyCode::RightControl),
        346 => Some(KeyCode::RightAlt),
        // ASCII A-Z (65..=90)
        65 => Some(KeyCode::A),
        66 => Some(KeyCode::B),
        67 => Some(KeyCode::C),
        68 => Some(KeyCode::D),
        69 => Some(KeyCode::E),
        70 => Some(KeyCode::F),
        71 => Some(KeyCode::G),
        72 => Some(KeyCode::H),
        73 => Some(KeyCode::I),
        74 => Some(KeyCode::J),
        75 => Some(KeyCode::K),
        76 => Some(KeyCode::L),
        77 => Some(KeyCode::M),
        78 => Some(KeyCode::N),
        79 => Some(KeyCode::O),
        80 => Some(KeyCode::P),
        81 => Some(KeyCode::Q),
        82 => Some(KeyCode::R),
        83 => Some(KeyCode::S),
        84 => Some(KeyCode::T),
        85 => Some(KeyCode::U),
        86 => Some(KeyCode::V),
        87 => Some(KeyCode::W),
        88 => Some(KeyCode::X),
        89 => Some(KeyCode::Y),
        90 => Some(KeyCode::Z),
        // Numbers 0-9 (48..=57)
        48 => Some(KeyCode::Key0),
        49 => Some(KeyCode::Key1),
        50 => Some(KeyCode::Key2),
        51 => Some(KeyCode::Key3),
        52 => Some(KeyCode::Key4),
        53 => Some(KeyCode::Key5),
        54 => Some(KeyCode::Key6),
        55 => Some(KeyCode::Key7),
        56 => Some(KeyCode::Key8),
        57 => Some(KeyCode::Key9),
        _ => None,
    }
}

// --- Host Native Functions ---

/// host_init(width: Int, height: Int, title: String, target_fps: Int) -> Bool
pub fn host_init(_args: &[Value]) -> Result<Value, VmFault> {
    Ok(Value::Bool(true))
}

/// host_should_close() -> Bool
pub fn host_should_close(_args: &[Value]) -> Result<Value, VmFault> {
    Ok(Value::Bool(safe_is_key_pressed(KeyCode::Escape)))
}

/// host_close() -> None
pub fn host_close(_args: &[Value]) -> Result<Value, VmFault> {
    Ok(Value::None)
}

/// host_key_down(key_code: Int) -> Bool
pub fn host_key_down(args: &[Value]) -> Result<Value, VmFault> {
    if args.is_empty() {
        return Ok(Value::Bool(false));
    }
    let code = to_i64(&args[0])?;
    let pressed = map_keycode(code).is_some_and(safe_is_key_down);
    Ok(Value::Bool(pressed))
}

/// host_key_pressed(key_code: Int) -> Bool
pub fn host_key_pressed(args: &[Value]) -> Result<Value, VmFault> {
    if args.is_empty() {
        return Ok(Value::Bool(false));
    }
    let code = to_i64(&args[0])?;
    let pressed = map_keycode(code).is_some_and(safe_is_key_pressed);
    Ok(Value::Bool(pressed))
}

/// host_get_char_pressed() -> String
pub fn host_get_char_pressed(_args: &[Value]) -> Result<Value, VmFault> {
    if let Some(ch) = safe_get_char_pressed() {
        Ok(Value::String(std::rc::Rc::new(ch.to_string())))
    } else {
        Ok(Value::String(std::rc::Rc::new(String::new())))
    }
}

/// host_mouse_x() -> Float
pub fn host_mouse_x(_args: &[Value]) -> Result<Value, VmFault> {
    let (mx, _) = safe_mouse_position();
    Ok(Value::Float(mx as f64))
}

/// host_mouse_y() -> Float
pub fn host_mouse_y(_args: &[Value]) -> Result<Value, VmFault> {
    let (_, my) = safe_mouse_position();
    Ok(Value::Float(my as f64))
}

/// host_mouse_btn(button: Int) -> Bool (0 = Left, 1 = Right, 2 = Middle)
pub fn host_mouse_btn(args: &[Value]) -> Result<Value, VmFault> {
    let btn_idx = if args.is_empty() {
        0
    } else {
        to_i64(&args[0])?
    };
    let mb = match btn_idx {
        1 => MouseButton::Right,
        2 => MouseButton::Middle,
        _ => MouseButton::Left,
    };
    Ok(Value::Bool(safe_is_mouse_button_down(mb)))
}

/// host_screen_width() -> Float
pub fn host_screen_width(_args: &[Value]) -> Result<Value, VmFault> {
    Ok(Value::Float(safe_screen_width() as f64))
}

/// host_screen_height() -> Float
pub fn host_screen_height(_args: &[Value]) -> Result<Value, VmFault> {
    Ok(Value::Float(safe_screen_height() as f64))
}

/// host_frame_time() -> Float
pub fn host_frame_time(_args: &[Value]) -> Result<Value, VmFault> {
    Ok(Value::Float(safe_frame_time() as f64))
}

/// host_clear_background(r: Float, g: Float, b: Float)
pub fn host_clear_background(args: &[Value]) -> Result<Value, VmFault> {
    let r = if !args.is_empty() {
        to_f32(&args[0])?
    } else {
        0.0
    };
    let g = if args.len() > 1 {
        to_f32(&args[1])?
    } else {
        0.0
    };
    let b = if args.len() > 2 {
        to_f32(&args[2])?
    } else {
        0.0
    };
    safe_clear_background(Color::new(r, g, b, 1.0));
    Ok(Value::None)
}

/// host_mouse_wheel_x() -> Float
pub fn host_mouse_wheel_x(_args: &[Value]) -> Result<Value, VmFault> {
    let (wx, _) = safe_mouse_wheel();
    Ok(Value::Float(wx as f64))
}

/// host_mouse_wheel_y() -> Float
pub fn host_mouse_wheel_y(_args: &[Value]) -> Result<Value, VmFault> {
    let (_, wy) = safe_mouse_wheel();
    Ok(Value::Float(wy as f64))
}

/// host_push_clip_rect(x, y, w, h)
pub fn host_push_clip_rect(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() < 4 {
        return Err(VmFault::TypeMismatch {
            expected: "4 arguments (x, y, w, h)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let x = to_f32(&args[0])?;
    let y = to_f32(&args[1])?;
    let w = to_f32(&args[2])?;
    let h = to_f32(&args[3])?;

    if let Ok(mut stack) = CLIP_STACK.lock() {
        let new_clip = if let Some(parent) = stack.last() {
            let x1 = x.max(parent.x);
            let y1 = y.max(parent.y);
            let x2 = (x + w).min(parent.x + parent.w);
            let y2 = (y + h).min(parent.y + parent.h);
            ClipRect {
                x: x1,
                y: y1,
                w: (x2 - x1).max(0.0),
                h: (y2 - y1).max(0.0),
            }
        } else {
            ClipRect { x, y, w, h }
        };
        stack.push(new_clip);
    }
    Ok(Value::None)
}

/// host_pop_clip_rect()
pub fn host_pop_clip_rect(_args: &[Value]) -> Result<Value, VmFault> {
    if let Ok(mut stack) = CLIP_STACK.lock() {
        stack.pop();
    }
    Ok(Value::None)
}

/// host_begin_frame(r: Float, g: Float, b: Float)
pub fn host_begin_frame(args: &[Value]) -> Result<Value, VmFault> {
    if let Ok(mut stack) = CLIP_STACK.lock() {
        stack.clear();
    }
    host_clear_background(args)
}

/// host_end_frame()
pub fn host_end_frame(_args: &[Value]) -> Result<Value, VmFault> {
    if let Ok(mut stack) = CLIP_STACK.lock() {
        stack.clear();
    }
    Ok(Value::None)
}

/// host_draw_rect(x, y, w, h, r, g, b, a)
pub fn host_draw_rect(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() < 8 {
        return Err(VmFault::TypeMismatch {
            expected: "8 arguments (x, y, w, h, r, g, b, a)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let x = to_f32(&args[0])?;
    let y = to_f32(&args[1])?;
    let w = to_f32(&args[2])?;
    let h = to_f32(&args[3])?;
    let r = to_f32(&args[4])?;
    let g = to_f32(&args[5])?;
    let b = to_f32(&args[6])?;
    let a = to_f32(&args[7])?;
    safe_draw_rect(x, y, w, h, Color::new(r, g, b, a));
    Ok(Value::None)
}

/// host_draw_rect_lines(x, y, w, h, thickness, r, g, b, a)
pub fn host_draw_rect_lines(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() < 9 {
        return Err(VmFault::TypeMismatch {
            expected: "9 arguments (x, y, w, h, thickness, r, g, b, a)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let x = to_f32(&args[0])?;
    let y = to_f32(&args[1])?;
    let w = to_f32(&args[2])?;
    let h = to_f32(&args[3])?;
    let th = to_f32(&args[4])?;
    let r = to_f32(&args[5])?;
    let g = to_f32(&args[6])?;
    let b = to_f32(&args[7])?;
    let a = to_f32(&args[8])?;
    safe_draw_rect_lines(x, y, w, h, th, Color::new(r, g, b, a));
    Ok(Value::None)
}

/// host_draw_circle(cx, cy, radius, r, g, b, a)
pub fn host_draw_circle(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() < 7 {
        return Err(VmFault::TypeMismatch {
            expected: "7 arguments (cx, cy, radius, r, g, b, a)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let cx = to_f32(&args[0])?;
    let cy = to_f32(&args[1])?;
    let radius = to_f32(&args[2])?;
    let r = to_f32(&args[3])?;
    let g = to_f32(&args[4])?;
    let b = to_f32(&args[5])?;
    let a = to_f32(&args[6])?;
    safe_draw_circle(cx, cy, radius, Color::new(r, g, b, a));
    Ok(Value::None)
}

/// host_draw_round_rect(x, y, w, h, radius, r, g, b, a)
pub fn host_draw_round_rect(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() < 9 {
        return Err(VmFault::TypeMismatch {
            expected: "9 arguments (x, y, w, h, radius, r, g, b, a)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let x = to_f32(&args[0])?;
    let y = to_f32(&args[1])?;
    let w = to_f32(&args[2])?;
    let h = to_f32(&args[3])?;
    let radius = to_f32(&args[4])?;
    let r = to_f32(&args[5])?;
    let g = to_f32(&args[6])?;
    let b = to_f32(&args[7])?;
    let a = to_f32(&args[8])?;
    safe_draw_round_rect(x, y, w, h, radius, Color::new(r, g, b, a));
    Ok(Value::None)
}

/// host_draw_round_rect_lines(x, y, w, h, radius, thickness, r, g, b, a)
pub fn host_draw_round_rect_lines(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() < 10 {
        return Err(VmFault::TypeMismatch {
            expected: "10 arguments (x, y, w, h, radius, thickness, r, g, b, a)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let x = to_f32(&args[0])?;
    let y = to_f32(&args[1])?;
    let w = to_f32(&args[2])?;
    let h = to_f32(&args[3])?;
    let radius = to_f32(&args[4])?;
    let th = to_f32(&args[5])?;
    let r = to_f32(&args[6])?;
    let g = to_f32(&args[7])?;
    let b = to_f32(&args[8])?;
    let a = to_f32(&args[9])?;
    safe_draw_round_rect_lines(x, y, w, h, radius, th, Color::new(r, g, b, a));
    Ok(Value::None)
}

/// host_draw_sdf_rect(x, y, w, h, radius, border_w, border_r, border_g, border_b, border_a, inner_hl, bg_r, bg_g, bg_b, bg_a)
/// host_draw_sdf_rect(x, y, w, h, radius, border_w, border_r, border_g, border_b, border_a, inner_hl, bg_r, bg_g, bg_b, bg_a)
/// host_draw_sdf_rect_v2(x, y, w, h, radius, elevation, focus_w, focus_r, focus_g, focus_b, focus_a, border_w, border_r, border_g, border_b, border_a, inner_hl, bg_r, bg_g, bg_b, bg_a)
pub fn host_draw_sdf_rect(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() < 15 {
        return Err(VmFault::TypeMismatch {
            expected: "15 arguments (x, y, w, h, radius, border_w, border_r, border_g, border_b, border_a, inner_hl, bg_r, bg_g, bg_b, bg_a)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let x = to_f32(&args[0])?;
    let y = to_f32(&args[1])?;
    let w = to_f32(&args[2])?;
    let h = to_f32(&args[3])?;
    let radius = to_f32(&args[4])?;
    let border_w = to_f32(&args[5])?;
    let border_r = to_f32(&args[6])?;
    let border_g = to_f32(&args[7])?;
    let border_b = to_f32(&args[8])?;
    let border_a = to_f32(&args[9])?;
    let inner_hl = to_f32(&args[10])?;
    let bg_r = to_f32(&args[11])?;
    let bg_g = to_f32(&args[12])?;
    let bg_b = to_f32(&args[13])?;
    let bg_a = to_f32(&args[14])?;

    safe_draw_sdf_rect(&SdfQuad {
        x,
        y,
        w,
        h,
        radius,
        radii: [0.0; 4],
        border_width: border_w,
        border_color: Color::new(border_r, border_g, border_b, border_a),
        inner_highlight: inner_hl,
        bg_color: Color::new(bg_r, bg_g, bg_b, bg_a),
        elevation: 0.0,
        focus_width: 0.0,
        focus_color: Color::new(0.0, 0.0, 0.0, 0.0),
    });
    Ok(Value::None)
}

/// M14 extended SDF draw: adds soft-shadow elevation and a focus ring that
/// composites in the same pass as the fill and border.
///
/// Argument order keeps the legacy tail intact so the two natives can coexist:
///   x, y, w, h, radius, elevation, focus_w,
///   focus_r, focus_g, focus_b, focus_a,
///   border_w, border_r, border_g, border_b, border_a, inner_hl,
///   bg_r, bg_g, bg_b, bg_a
pub fn host_draw_sdf_rect_v2(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() < 21 {
        return Err(VmFault::TypeMismatch {
            expected: "21 arguments (x, y, w, h, radius, elevation, focus_w, focus_r, focus_g, focus_b, focus_a, border_w, border_r, border_g, border_b, border_a, inner_hl, bg_r, bg_g, bg_b, bg_a)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let x = to_f32(&args[0])?;
    let y = to_f32(&args[1])?;
    let w = to_f32(&args[2])?;
    let h = to_f32(&args[3])?;
    let radius = to_f32(&args[4])?;
    let elevation = to_f32(&args[5])?;
    let focus_w = to_f32(&args[6])?;
    let focus_r = to_f32(&args[7])?;
    let focus_g = to_f32(&args[8])?;
    let focus_b = to_f32(&args[9])?;
    let focus_a = to_f32(&args[10])?;
    let border_w = to_f32(&args[11])?;
    let border_r = to_f32(&args[12])?;
    let border_g = to_f32(&args[13])?;
    let border_b = to_f32(&args[14])?;
    let border_a = to_f32(&args[15])?;
    let inner_hl = to_f32(&args[16])?;
    let bg_r = to_f32(&args[17])?;
    let bg_g = to_f32(&args[18])?;
    let bg_b = to_f32(&args[19])?;
    let bg_a = to_f32(&args[20])?;

    safe_draw_sdf_rect(&SdfQuad {
        x,
        y,
        w,
        h,
        radius,
        radii: [0.0; 4],
        border_width: border_w,
        border_color: Color::new(border_r, border_g, border_b, border_a),
        inner_highlight: inner_hl,
        bg_color: Color::new(bg_r, bg_g, bg_b, bg_a),
        elevation,
        focus_width: focus_w,
        focus_color: Color::new(focus_r, focus_g, focus_b, focus_a),
    });
    Ok(Value::None)
}

/// host_draw_sdf_rect_corners(x, y, w, h, tl, tr, br, bl, elevation, focus_w, focus_r, focus_g, focus_b, focus_a, border_w, border_r, border_g, border_b, border_a, inner_hl, bg_r, bg_g, bg_b, bg_a)
///
/// Per-corner radii variant, for asymmetric "squircle" shapes.
pub fn host_draw_sdf_rect_corners(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() < 24 {
        return Err(VmFault::TypeMismatch {
            expected: "24 arguments (x, y, w, h, tl, tr, br, bl, elevation, focus_w, focus_r, focus_g, focus_b, focus_a, border_w, border_r, border_g, border_b, border_a, inner_hl, bg_r, bg_g, bg_b, bg_a)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let a = |i: usize| -> Result<f32, VmFault> { to_f32(&args[i]) };
    let quad = SdfQuad {
        x: a(0)?,
        y: a(1)?,
        w: a(2)?,
        h: a(3)?,
        radius: 0.0,
        radii: [a(4)?, a(5)?, a(6)?, a(7)?],
        elevation: a(8)?,
        focus_width: a(9)?,
        focus_color: Color::new(a(10)?, a(11)?, a(12)?, a(13)?),
        border_width: a(14)?,
        border_color: Color::new(a(15)?, a(16)?, a(17)?, a(18)?),
        inner_highlight: a(19)?,
        bg_color: Color::new(a(20)?, a(21)?, a(22)?, a(23)?),
    };
    safe_draw_sdf_rect(&quad);
    Ok(Value::None)
}

/// host_draw_bezier(x1, y1, cx1, cy1, cx2, cy2, x2, y2, thickness, r, g, b, a)
pub fn host_draw_bezier(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() < 13 {
        return Err(VmFault::TypeMismatch {
            expected: "13 arguments (x1, y1, cx1, cy1, cx2, cy2, x2, y2, thickness, r, g, b, a)"
                .to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let x1 = to_f32(&args[0])?;
    let y1 = to_f32(&args[1])?;
    let cx1 = to_f32(&args[2])?;
    let cy1 = to_f32(&args[3])?;
    let cx2 = to_f32(&args[4])?;
    let cy2 = to_f32(&args[5])?;
    let x2 = to_f32(&args[6])?;
    let y2 = to_f32(&args[7])?;
    let th = to_f32(&args[8])?;
    let r = to_f32(&args[9])?;
    let g = to_f32(&args[10])?;
    let b = to_f32(&args[11])?;
    let a = to_f32(&args[12])?;

    let color = Color::new(r, g, b, a);
    let steps = 24;
    let mut prev_x = x1;
    let mut prev_y = y1;

    for i in 1..=steps {
        let t = (i as f32) / (steps as f32);
        let u = 1.0 - t;
        let cur_x = u * u * u * x1 + 3.0 * u * u * t * cx1 + 3.0 * u * t * t * cx2 + t * t * t * x2;
        let cur_y = u * u * u * y1 + 3.0 * u * u * t * cy1 + 3.0 * u * t * t * cy2 + t * t * t * y2;
        safe_draw_line(prev_x, prev_y, cur_x, cur_y, th, color);
        prev_x = cur_x;
        prev_y = cur_y;
    }
    Ok(Value::None)
}

/// host_draw_line(x1, y1, x2, y2, thickness, r, g, b, a)
pub fn host_draw_line(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() < 9 {
        return Err(VmFault::TypeMismatch {
            expected: "9 arguments (x1, y1, x2, y2, thickness, r, g, b, a)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let x1 = to_f32(&args[0])?;
    let y1 = to_f32(&args[1])?;
    let x2 = to_f32(&args[2])?;
    let y2 = to_f32(&args[3])?;
    let th = to_f32(&args[4])?;
    let r = to_f32(&args[5])?;
    let g = to_f32(&args[6])?;
    let b = to_f32(&args[7])?;
    let a = to_f32(&args[8])?;
    safe_draw_line(x1, y1, x2, y2, th, Color::new(r, g, b, a));
    Ok(Value::None)
}

/// host_draw_triangle(x1, y1, x2, y2, x3, y3, r, g, b, a)
pub fn host_draw_triangle(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() < 10 {
        return Err(VmFault::TypeMismatch {
            expected: "10 arguments (x1, y1, x2, y2, x3, y3, r, g, b, a)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let x1 = to_f32(&args[0])?;
    let y1 = to_f32(&args[1])?;
    let x2 = to_f32(&args[2])?;
    let y2 = to_f32(&args[3])?;
    let x3 = to_f32(&args[4])?;
    let y3 = to_f32(&args[5])?;
    let r = to_f32(&args[6])?;
    let g = to_f32(&args[7])?;
    let b = to_f32(&args[8])?;
    let a = to_f32(&args[9])?;
    safe_draw_triangle(
        vec2(x1, y1),
        vec2(x2, y2),
        vec2(x3, y3),
        Color::new(r, g, b, a),
    );
    Ok(Value::None)
}

/// host_draw_triangle_lines(x1, y1, x2, y2, x3, y3, thickness, r, g, b, a)
pub fn host_draw_triangle_lines(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() < 11 {
        return Err(VmFault::TypeMismatch {
            expected: "11 arguments (x1, y1, x2, y2, x3, y3, thickness, r, g, b, a)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let x1 = to_f32(&args[0])?;
    let y1 = to_f32(&args[1])?;
    let x2 = to_f32(&args[2])?;
    let y2 = to_f32(&args[3])?;
    let x3 = to_f32(&args[4])?;
    let y3 = to_f32(&args[5])?;
    let th = to_f32(&args[6])?;
    let r = to_f32(&args[7])?;
    let g = to_f32(&args[8])?;
    let b = to_f32(&args[9])?;
    let a = to_f32(&args[10])?;
    safe_draw_triangle_lines(
        vec2(x1, y1),
        vec2(x2, y2),
        vec2(x3, y3),
        th,
        Color::new(r, g, b, a),
    );
    Ok(Value::None)
}

/// host_draw_text(text, x, y, font_size, r, g, b)
/// Numeric weight (CSS-style 100..=900) to synthetic emboldening offset in
/// pixels at a given font size.
///
/// Macroquad's text pipeline rasterizes glyphs from a single static font
/// instance and exposes no hook for OpenType variation coordinates, so the
/// `wght` axis of the bundled Inter variable font cannot be addressed at
/// draw time. Weight is therefore applied as a stroke-like emboldening: the
/// glyph is drawn once normally, then re-drawn with a small offset whose
/// magnitude tracks the requested weight. This is the same technique fontdue
/// itself uses for `FontSettings::embolden`.
///
/// Returns `0.0` for weights at or below normal, so regular text renders
/// through the single-pass fast path.
fn embolden_offset(weight: i64, font_size: f32) -> f32 {
    if weight <= 500 {
        return 0.0;
    }
    // 500 -> 0, 700 -> ~1/24 em, 900 -> ~1/16 em. Sub-pixel, clamped to 2px:
    // beyond that the glyph counters start to fill in and read as blurry
    // rather than bold.
    let t = ((weight - 500) as f32) / 400.0;
    (t * font_size / 20.0).min(2.0)
}

/// Resolves the `font_weight` prop value to a CSS-style numeric weight.
///
/// Accepts keyword strings (the historical Aipo prop contract) and raw
/// numbers. Unknown values fall back to normal.
fn resolve_font_weight(val: &Value) -> i64 {
    match val {
        Value::Int(n) => *n,
        Value::Float(f) => *f as i64,
        Value::String(s) => match s.to_ascii_lowercase().as_str() {
            "thin" | "hairline" => 100,
            "extralight" | "ultralight" => 200,
            "light" => 300,
            "normal" | "regular" | "book" => 400,
            "medium" => 500,
            "semibold" | "demibold" => 600,
            "bold" => 700,
            "extrabold" | "ultrabold" => 800,
            "black" | "heavy" => 900,
            _ => 400,
        },
        _ => 400,
    }
}

/// host_draw_text(text, x, y, font_size, r, g, b)
pub fn host_draw_text(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() < 7 {
        return Err(VmFault::TypeMismatch {
            expected: "7 arguments (text, x, y, font_size, r, g, b)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let text = to_string(&args[0])?;
    let x = to_f32(&args[1])?;
    let y = to_f32(&args[2])?;
    let size = to_f32(&args[3])?;
    let r = to_f32(&args[4])?;
    let g = to_f32(&args[5])?;
    let b = to_f32(&args[6])?;

    // Optional 8th argument: font weight (see `resolve_font_weight`).
    let weight = if args.len() >= 8 {
        resolve_font_weight(&args[7])
    } else {
        400
    };

    let color = Color::new(r, g, b, 1.0);
    let offset = embolden_offset(weight, size);

    if offset > 0.0 {
        // Four passes at the compass points approximate a dilated stroke and
        // read as a heavier weight at UI sizes. Diagonals are omitted: they
        // add cost without visible benefit below ~24px.
        for (dx, dy) in [(offset, 0.0), (-offset, 0.0), (0.0, offset), (0.0, -offset)] {
            safe_draw_text(&text, x + dx, y + dy, size, color);
        }
    }
    safe_draw_text(&text, x, y, size, color);
    Ok(Value::None)
}

/// host_measure_text(text: String, font_size: Float) -> Float
/// host_measure_text(text: String, font_size: Float, font_weight: Int) -> Float
///
/// Returns the exact pixel width of the given text at the specified font size.
/// Uses the fontdue CPU-side rasterizer (headless-safe, no GPU required).
///
/// The optional third argument applies the same synthetic emboldening as
/// `host_draw_text`, so measurement and rendering agree for bold text.
pub fn host_measure_text(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() < 2 {
        return Err(VmFault::TypeMismatch {
            expected: "2 arguments (text, font_size), or 3 with font_weight".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let text = to_string(&args[0])?;
    let font_size = to_f32(&args[1])?;

    if text.is_empty() {
        return Ok(Value::Float(0.0));
    }

    let font = get_zoe_fontdue();
    let mut width = 0.0f32;
    let mut prev_char: Option<char> = None;

    for ch in text.chars() {
        // Add kerning between adjacent characters
        if let Some(prev) = prev_char
            && let Some(kern) = font.horizontal_kern(prev, ch, font_size)
        {
            width += kern;
        }
        let metrics = font.metrics(ch, font_size);
        width += metrics.advance_width;
        prev_char = Some(ch);
    }

    // Bold advances grow by the emboldening offset, matching the draw path.
    if args.len() >= 3 {
        let offset = embolden_offset(resolve_font_weight(&args[2]), font_size);
        if offset > 0.0 {
            width += offset * 2.0;
        }
    }

    Ok(Value::Float(width as f64))
}

/// host_font_metrics(font_size: Float) -> Dict
/// Returns font metrics for baseline-aligned text rendering:
/// { "ascent": Float, "descent": Float, "line_gap": Float, "line_height": Float }
pub fn host_font_metrics(args: &[Value]) -> Result<Value, VmFault> {
    let font_size = if args.is_empty() {
        14.0
    } else {
        to_f32(&args[0])?
    };

    let font = get_zoe_fontdue();
    let lm = font.horizontal_line_metrics(font_size);

    let (ascent, descent, line_gap, line_height) = match lm {
        Some(m) => (m.ascent, m.descent, m.line_gap, m.new_line_size),
        None => (font_size * 0.8, font_size * -0.2, 0.0, font_size * 1.2),
    };

    let entries = vec![
        (
            Value::String(Rc::new("ascent".to_string())),
            Value::Float(ascent as f64),
        ),
        (
            Value::String(Rc::new("descent".to_string())),
            Value::Float(descent as f64),
        ),
        (
            Value::String(Rc::new("line_gap".to_string())),
            Value::Float(line_gap as f64),
        ),
        (
            Value::String(Rc::new("line_height".to_string())),
            Value::Float(line_height as f64),
        ),
    ];
    let dict_map = aipo_vm::DictMap::from_entries(entries);
    Ok(Value::Dict(Rc::new(RefCell::new(dict_map))))
}

/// host_load_texture(path: String) -> Int
pub fn host_load_texture(args: &[Value]) -> Result<Value, VmFault> {
    if args.is_empty() {
        return Ok(Value::Int(0));
    }
    let path = to_string(&args[0])?;
    let mut cache = TEXTURE_CACHE
        .lock()
        .map_err(|_| VmFault::CapabilityDenied {
            capability: "host.texture".to_string(),
            operation: "texture_cache_lock".to_string(),
        })?;
    let id = cache.load(&path);
    Ok(Value::Int(id))
}

/// host_draw_sprite(tex_id, x, y, w, h, rot, flip_x)
pub fn host_draw_sprite(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() < 7 {
        return Err(VmFault::TypeMismatch {
            expected: "7 arguments (tex_id, x, y, w, h, rot, flip_x)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let id = to_i64(&args[0])?;
    let x = to_f32(&args[1])?;
    let y = to_f32(&args[2])?;
    let w = to_f32(&args[3])?;
    let h = to_f32(&args[4])?;
    let rot = to_f32(&args[5])?;
    let flip_x = to_bool(&args[6])?;

    let mut cache = TEXTURE_CACHE
        .lock()
        .map_err(|_| VmFault::CapabilityDenied {
            capability: "host.texture".to_string(),
            operation: "texture_cache_lock".to_string(),
        })?;
    let tex = cache.get(id);

    safe_draw_texture(
        tex.as_ref(),
        x,
        y,
        DrawTextureParams {
            dest_size: Some(vec2(w, h)),
            rotation: rot,
            flip_x,
            ..Default::default()
        },
    );
    Ok(Value::None)
}

/// host_draw_sprite_subrect(tex_id, sx, sy, sw, sh, dx, dy, dw, dh, flip_x)
pub fn host_draw_sprite_subrect(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() < 10 {
        return Err(VmFault::TypeMismatch {
            expected: "10 arguments (tex_id, sx, sy, sw, sh, dx, dy, dw, dh, flip_x)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let id = to_i64(&args[0])?;
    let sx = to_f32(&args[1])?;
    let sy = to_f32(&args[2])?;
    let sw = to_f32(&args[3])?;
    let sh = to_f32(&args[4])?;
    let dx = to_f32(&args[5])?;
    let dy = to_f32(&args[6])?;
    let dw = to_f32(&args[7])?;
    let dh = to_f32(&args[8])?;
    let flip_x = to_bool(&args[9])?;

    let mut cache = TEXTURE_CACHE
        .lock()
        .map_err(|_| VmFault::CapabilityDenied {
            capability: "host.texture".to_string(),
            operation: "texture_cache_lock".to_string(),
        })?;
    let tex = cache.get(id);

    safe_draw_texture(
        tex.as_ref(),
        dx,
        dy,
        DrawTextureParams {
            source: Some(Rect::new(sx, sy, sw, sh)),
            dest_size: Some(vec2(dw, dh)),
            flip_x,
            ..Default::default()
        },
    );
    Ok(Value::None)
}

/// host_draw_icon_path(path_data: String, x: Float, y: Float, size: Float, stroke_width: Float, r: Float, g: Float, b: Float, a: Float)
pub fn host_draw_icon_path(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() < 9 {
        return Err(VmFault::TypeMismatch {
            expected: "9 arguments (path_data, x, y, size, stroke_width, r, g, b, a)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let path_data = to_string(&args[0])?;
    let x = to_f32(&args[1])?;
    let y = to_f32(&args[2])?;
    let size = to_f32(&args[3])?;
    let stroke_width = to_f32(&args[4])?;
    let r = to_f32(&args[5])?;
    let g = to_f32(&args[6])?;
    let b = to_f32(&args[7])?;
    let a = to_f32(&args[8])?;

    let size_u32 = (size.round() as u32).clamp(8, 256);
    let color = Color::new(r, g, b, a);

    let tex_opt = if let Ok(mut cache_lock) = ICON_CACHE.lock() {
        let cache = cache_lock.get_or_insert_with(IconCache::new);
        cache.get_or_render(&path_data, size_u32, stroke_width, color)
    } else {
        None
    };

    if let Some(ref tex) = tex_opt {
        safe_draw_texture(
            Some(tex),
            x,
            y,
            DrawTextureParams {
                dest_size: Some(vec2(size, size)),
                ..Default::default()
            },
        );
    }

    Ok(Value::None)
}

/// host_set_camera(target_x: Float, target_y: Float, zoom: Float)
pub fn host_set_camera(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() < 3 {
        return Err(VmFault::TypeMismatch {
            expected: "3 arguments (target_x, target_y, zoom)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let tx = to_f32(&args[0])?;
    let ty = to_f32(&args[1])?;
    let z = to_f32(&args[2])?.max(0.01);

    let sw = safe_screen_width();
    let sh = safe_screen_height();

    safe_set_camera(tx, ty, z, sw, sh);
    Ok(Value::None)
}

/// host_reset_camera()
pub fn host_reset_camera(_args: &[Value]) -> Result<Value, VmFault> {
    safe_reset_camera();
    Ok(Value::None)
}

/// host_set_viewport_camera(vx, vy, vw, vh, target_x, target_y, zoom)
pub fn host_set_viewport_camera(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() < 7 {
        return Err(VmFault::TypeMismatch {
            expected: "7 arguments (vx, vy, vw, vh, target_x, target_y, zoom)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let vx = to_f32(&args[0])?;
    let vy = to_f32(&args[1])?;
    let vw = to_f32(&args[2])?.max(1.0);
    let vh = to_f32(&args[3])?.max(1.0);
    let tx = to_f32(&args[4])?;
    let ty = to_f32(&args[5])?;
    let z = to_f32(&args[6])?.max(0.001);

    let sh = safe_screen_height();
    let gl_x = vx as i32;
    let gl_y = (sh - (vy + vh)).max(0.0) as i32;
    let gl_w = vw as i32;
    let gl_h = vh as i32;

    safe_set_camera_viewport(tx, ty, z, gl_x, gl_y, gl_w, gl_h);
    Ok(Value::None)
}

/// host_load_sound(path: String) -> Int
pub fn host_load_sound(args: &[Value]) -> Result<Value, VmFault> {
    if args.is_empty() {
        return Ok(Value::Int(0));
    }
    let path = to_string(&args[0])?;
    let mut audio = AUDIO.lock().map_err(|_| VmFault::CapabilityDenied {
        capability: "host.audio".to_string(),
        operation: "audio_lock".to_string(),
    })?;
    let id = audio.load_file(&path);
    Ok(Value::Int(id))
}

/// host_play_sound(sound_id: Int, volume: Float, pitch: Float) -> None
pub fn host_play_sound(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() < 3 {
        return Err(VmFault::TypeMismatch {
            expected: "3 arguments (sound_id, volume, pitch)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let id = to_i64(&args[0])?;
    let volume = to_f32(&args[1])?;
    let _pitch = to_f32(&args[2])?;
    let audio = AUDIO.lock().map_err(|_| VmFault::CapabilityDenied {
        capability: "host.audio".to_string(),
        operation: "audio_lock".to_string(),
    })?;
    audio.play(id, volume);
    Ok(Value::None)
}

/// host_play_preset(name: String, volume: Float, pitch: Float) -> None
pub fn host_play_preset(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() < 3 {
        return Err(VmFault::TypeMismatch {
            expected: "3 arguments (name, volume, pitch)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let name = to_string(&args[0])?;
    let volume = to_f32(&args[1])?;
    let pitch = to_f32(&args[2])?;
    let mut audio = AUDIO.lock().map_err(|_| VmFault::CapabilityDenied {
        capability: "host.audio".to_string(),
        operation: "audio_lock".to_string(),
    })?;
    audio.play_preset(&name, volume, pitch);
    Ok(Value::None)
}

/// host_synth_sound(wave_type: String, start_freq: Float, freq_slide: Float, duration: Float, volume: Float) -> Int
pub fn host_synth_sound(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() < 5 {
        return Err(VmFault::TypeMismatch {
            expected: "5 arguments (wave_type, start_freq, freq_slide, duration, volume)"
                .to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let wave_type = to_string(&args[0])?;
    let start_freq = to_f32(&args[1])?;
    let freq_slide = to_f32(&args[2])?;
    let duration = to_f32(&args[3])?;
    let volume = to_f32(&args[4])?;

    let config = SynthConfig {
        wave_type,
        start_freq,
        freq_slide,
        duration,
        duty_cycle: 0.5,
        volume,
    };

    let mut audio = AUDIO.lock().map_err(|_| VmFault::CapabilityDenied {
        capability: "host.audio".to_string(),
        operation: "audio_lock".to_string(),
    })?;
    let id = audio.synth_sound(&config);
    Ok(Value::Int(id))
}

/// host_stop_sound(sound_id: Int) -> None
pub fn host_stop_sound(args: &[Value]) -> Result<Value, VmFault> {
    if args.is_empty() {
        return Ok(Value::None);
    }
    let id = to_i64(&args[0])?;
    let audio = AUDIO.lock().map_err(|_| VmFault::CapabilityDenied {
        capability: "host.audio".to_string(),
        operation: "audio_lock".to_string(),
    })?;
    audio.stop(id);
    Ok(Value::None)
}

/// host_play_music(sound_id: Int, volume: Float, is_loop: Bool) -> None
pub fn host_play_music(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() < 3 {
        return Err(VmFault::TypeMismatch {
            expected: "3 arguments (sound_id, volume, is_loop)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let id = to_i64(&args[0])?;
    let volume = to_f32(&args[1])?;
    let is_loop = to_bool(&args[2])?;
    let mut audio = AUDIO.lock().map_err(|_| VmFault::CapabilityDenied {
        capability: "host.audio".to_string(),
        operation: "audio_lock".to_string(),
    })?;
    audio.play_music(id, volume, is_loop);
    Ok(Value::None)
}

/// host_stop_music() -> None
pub fn host_stop_music(_args: &[Value]) -> Result<Value, VmFault> {
    let mut audio = AUDIO.lock().map_err(|_| VmFault::CapabilityDenied {
        capability: "host.audio".to_string(),
        operation: "audio_lock".to_string(),
    })?;
    audio.stop_music();
    Ok(Value::None)
}

/// Type signature of a host native function callback.
type NativeFn = fn(&[Value]) -> Result<Value, VmFault>;

/// Entry representing a registered host native function (name, arity, callback).
type NativeEntry = (&'static str, usize, NativeFn);

/// Table of all host functions with name, arity, and native function pointer.
const NATIVES: &[NativeEntry] = &[
    ("host_init", 4, host_init),
    ("host_should_close", 0, host_should_close),
    ("host_close", 0, host_close),
    ("host_key_down", 1, host_key_down),
    ("host_key_pressed", 1, host_key_pressed),
    ("host_get_char_pressed", 0, host_get_char_pressed),
    ("host_mouse_x", 0, host_mouse_x),
    ("host_mouse_y", 0, host_mouse_y),
    ("host_mouse_btn", 1, host_mouse_btn),
    ("host_mouse_wheel_x", 0, host_mouse_wheel_x),
    ("host_mouse_wheel_y", 0, host_mouse_wheel_y),
    ("host_push_clip_rect", 4, host_push_clip_rect),
    ("host_pop_clip_rect", 0, host_pop_clip_rect),
    ("host_screen_width", 0, host_screen_width),
    ("host_screen_height", 0, host_screen_height),
    ("host_frame_time", 0, host_frame_time),
    ("host_clear_background", 3, host_clear_background),
    ("host_begin_frame", 3, host_begin_frame),
    ("host_end_frame", 0, host_end_frame),
    ("host_draw_rect", 8, host_draw_rect),
    ("host_draw_rect_lines", 9, host_draw_rect_lines),
    ("host_draw_round_rect", 9, host_draw_round_rect),
    ("host_draw_round_rect_lines", 10, host_draw_round_rect_lines),
    ("host_draw_sdf_rect", 15, host_draw_sdf_rect),
    ("host_draw_sdf_rect_v2", 21, host_draw_sdf_rect_v2),
    ("host_draw_sdf_rect_corners", 24, host_draw_sdf_rect_corners),
    ("host_draw_circle", 7, host_draw_circle),
    ("host_draw_line", 9, host_draw_line),
    ("host_draw_bezier", 13, host_draw_bezier),
    ("host_draw_triangle", 10, host_draw_triangle),
    ("host_draw_triangle_lines", 11, host_draw_triangle_lines),
    ("host_draw_text", 7, host_draw_text),
    ("host_measure_text", 2, host_measure_text),
    // Optional-argument overloads. The prelude records the minimum arity, so
    // the 8-argument form of draw_text and 3-argument form of measure_text
    // are declared under distinct names for static resolution.
    ("host_draw_text_weighted", 8, host_draw_text),
    ("host_measure_text_weighted", 3, host_measure_text),
    ("host_font_metrics", 1, host_font_metrics),
    ("host_load_texture", 1, host_load_texture),
    ("host_draw_sprite", 7, host_draw_sprite),
    ("host_draw_sprite_subrect", 10, host_draw_sprite_subrect),
    ("host_draw_icon_path", 9, host_draw_icon_path),
    ("host_set_camera", 3, host_set_camera),
    ("host_reset_camera", 0, host_reset_camera),
    ("host_set_viewport_camera", 7, host_set_viewport_camera),
    ("host_load_sound", 1, host_load_sound),
    ("host_play_sound", 3, host_play_sound),
    ("host_play_preset", 3, host_play_preset),
    ("host_synth_sound", 5, host_synth_sound),
    ("host_stop_sound", 1, host_stop_sound),
    ("host_play_music", 3, host_play_music),
    ("host_stop_music", 0, host_stop_music),
];

/// Registers the game host native functions into the semantic analyzer's Prelude surface.
pub fn register_surface_symbols(surface: &mut PreludeSurface) {
    for (name, arity, _) in NATIVES {
        surface.add_function(name, *arity, *arity);
        // Also register __aipo_game_* canonical alias
        let canonical_alias = format!("__aipo_game_{}", name.trim_start_matches("host_"));
        surface.add_function(&canonical_alias, *arity, *arity);
    }
    surface.add_variable("game");
    surface.add_variable("egui");
}

/// Registers the game host native functions into the Aipo VM instance.
pub fn register_vm_natives(vm: &mut Vm) {
    let mut game_dict_entries = Vec::new();

    for (name, arity, func) in NATIVES {
        let native_val = Value::native(*name, *arity, *func);
        // Define as top-level global
        vm.define_global(*name, native_val.clone());

        // Define canonical __aipo_game_* alias
        let canonical_alias = format!("__aipo_game_{}", name.trim_start_matches("host_"));
        vm.define_global(canonical_alias, native_val.clone());

        // Add to game module dict
        let clean_key = name.trim_start_matches("host_");
        game_dict_entries.push((Value::String(Rc::new(clean_key.to_string())), native_val));
    }

    let dict_map = aipo_vm::DictMap::from_entries(game_dict_entries);
    vm.define_global("game", Value::Dict(Rc::new(RefCell::new(dict_map))));

    // Install and register agnostic egui host profile
    aipo_egui::install_egui(aipo_egui::EguiService::new());
    aipo_egui::register_egui(vm);
}

fn render_epaint_shape(shape: &egui::epaint::Shape) {
    match shape {
        egui::epaint::Shape::Vec(children) => {
            for child in children {
                render_epaint_shape(child);
            }
        }
        egui::epaint::Shape::Rect(rect_shape) => {
            let r = rect_shape.rect;
            let c = rect_shape.fill;
            if c.a() > 0 {
                let col = Color::new(
                    f32::from(c.r()) / 255.0,
                    f32::from(c.g()) / 255.0,
                    f32::from(c.b()) / 255.0,
                    f32::from(c.a()) / 255.0,
                );
                safe_draw_rect(r.min.x, r.min.y, r.width(), r.height(), col);
            }
            if rect_shape.stroke.width > 0.0 && rect_shape.stroke.color.a() > 0 {
                let sc = rect_shape.stroke.color;
                let stroke_col = Color::new(
                    f32::from(sc.r()) / 255.0,
                    f32::from(sc.g()) / 255.0,
                    f32::from(sc.b()) / 255.0,
                    f32::from(sc.a()) / 255.0,
                );
                safe_draw_rect_lines(
                    r.min.x,
                    r.min.y,
                    r.width(),
                    r.height(),
                    rect_shape.stroke.width,
                    stroke_col,
                );
            }
        }
        egui::epaint::Shape::Text(text_shape) => {
            let pos = text_shape.pos;
            let text = text_shape.galley.text();
            let mut c = text_shape.fallback_color;
            if c.a() == 0 {
                c = egui::Color32::from_rgb(220, 225, 235);
            }
            let font_size = (text_shape.galley.size().y * 0.95).clamp(14.0, 36.0);
            let text_col = Color::new(
                f32::from(c.r()) / 255.0,
                f32::from(c.g()) / 255.0,
                f32::from(c.b()) / 255.0,
                f32::from(c.a()) / 255.0,
            );
            safe_draw_text(text, pos.x, pos.y + font_size * 0.85, font_size, text_col);
        }
        egui::epaint::Shape::LineSegment { points, stroke } => {
            let c = stroke.color;
            let col = Color::new(
                f32::from(c.r()) / 255.0,
                f32::from(c.g()) / 255.0,
                f32::from(c.b()) / 255.0,
                f32::from(c.a()) / 255.0,
            );
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                draw_line(
                    points[0].x,
                    points[0].y,
                    points[1].x,
                    points[1].y,
                    stroke.width,
                    col,
                );
            }));
        }
        egui::epaint::Shape::Path(path) => {
            if path.points.len() >= 2 {
                let c = match &path.stroke.color {
                    egui::epaint::ColorMode::Solid(c) => *c,
                    egui::epaint::ColorMode::UV(_) => egui::Color32::WHITE,
                };
                let col = Color::new(
                    f32::from(c.r()) / 255.0,
                    f32::from(c.g()) / 255.0,
                    f32::from(c.b()) / 255.0,
                    f32::from(c.a()) / 255.0,
                );
                for i in 0..(path.points.len() - 1) {
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        draw_line(
                            path.points[i].x,
                            path.points[i].y,
                            path.points[i + 1].x,
                            path.points[i + 1].y,
                            path.stroke.width,
                            col,
                        );
                    }));
                }
                if path.closed && path.points.len() > 2 {
                    let last = path.points.len() - 1;
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        draw_line(
                            path.points[last].x,
                            path.points[last].y,
                            path.points[0].x,
                            path.points[0].y,
                            path.stroke.width,
                            col,
                        );
                    }));
                }
            }
        }
        egui::epaint::Shape::Circle(circle) => {
            let c = circle.fill;
            let col = Color::new(
                f32::from(c.r()) / 255.0,
                f32::from(c.g()) / 255.0,
                f32::from(c.b()) / 255.0,
                f32::from(c.a()) / 255.0,
            );
            safe_draw_circle(circle.center.x, circle.center.y, circle.radius, col);
        }
        _ => {}
    }
}

/// Renders the shapes generated by the active egui session on top of the screen.
pub fn render_egui_overlay() {
    let _ = aipo_egui::with_egui("egui", "render_egui_overlay", |service| {
        let Some(handle) = service.last_active_handle.or(service.active_handle) else {
            return Ok(());
        };
        let Some(session) = service.sessions.get(handle) else {
            return Ok(());
        };
        let Some(ref output) = session.last_output else {
            return Ok(());
        };

        for clipped in &output.shapes {
            render_epaint_shape(&clipped.shape);
        }
        Ok(())
    });
}
