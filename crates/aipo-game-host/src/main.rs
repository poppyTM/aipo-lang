//! Native desktop game host for Aipo using Miniquad/Macroquad backend.
//!
//! Provides hardware-accelerated 2D rendering, real-time input handling,
//! dynamic `.aipo` script execution with hot-reload, and a built-in 60 FPS Snake game demo.

#![forbid(unsafe_code)]

use aipo_game_host::{audio_system::AUDIO, host_bridge};

use aipo_vm::Value;
use macroquad::prelude::*;
use std::path::Path;

/// Direction of the snake on the 2D grid.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Moving up (negative Y)
    Up,
    /// Moving down (positive Y)
    Down,
    /// Moving left (negative X)
    Left,
    /// Moving right (positive X)
    Right,
}

/// A 2D grid coordinate point.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct GridPoint {
    /// X coordinate
    pub x: i32,
    /// Y coordinate
    pub y: i32,
}

/// Visual particle effect for apple pickups.
pub struct Particle {
    /// Current world position X
    pub x: f32,
    /// Current world position Y
    pub y: f32,
    /// Velocity X
    pub vx: f32,
    /// Velocity Y
    pub vy: f32,
    /// Remaining life duration (seconds)
    pub life: f32,
    /// Particle color
    pub color: Color,
}

/// State of the native interactive Snake Game.
pub struct SnakeGameState {
    /// Grid width in cells
    pub grid_w: i32,
    /// Grid height in cells
    pub grid_h: i32,
    /// Cell pixel size
    pub cell_size: f32,
    /// Snake body segments from head to tail
    pub body: Vec<GridPoint>,
    /// Current moving direction
    pub dir: Direction,
    /// Next queued direction from input
    pub next_dir: Direction,
    /// Current apple coordinate
    pub apple: GridPoint,
    /// Current player score
    pub score: u32,
    /// Best score in current session
    pub high_score: u32,
    /// Whether the game is over
    pub game_over: bool,
    /// Whether the game is paused
    pub paused: bool,
    /// Step timer accumulator
    pub move_timer: f32,
    /// Seconds per grid step (lower = faster)
    pub step_interval: f32,
    /// Squash & stretch scale factor for head
    pub head_scale: f32,
    /// Active visual particles
    pub particles: Vec<Particle>,
}

impl SnakeGameState {
    /// Creates a new fresh Snake game state.
    pub fn new(grid_w: i32, grid_h: i32, cell_size: f32) -> Self {
        let start_x = grid_w / 4;
        let start_y = grid_h / 2;

        Self {
            grid_w,
            grid_h,
            cell_size,
            body: vec![
                GridPoint {
                    x: start_x,
                    y: start_y,
                },
                GridPoint {
                    x: start_x - 1,
                    y: start_y,
                },
                GridPoint {
                    x: start_x - 2,
                    y: start_y,
                },
            ],
            dir: Direction::Right,
            next_dir: Direction::Right,
            apple: GridPoint {
                x: grid_w / 2,
                y: start_y,
            },
            score: 0,
            high_score: 0,
            game_over: false,
            paused: false,
            move_timer: 0.0,
            step_interval: 0.11, // ~9 moves per second for responsive feel
            head_scale: 1.0,
            particles: Vec::new(),
        }
    }

    /// Resets the game session while preserving high score.
    pub fn restart(&mut self) {
        let best = self.high_score.max(self.score);
        let start_x = self.grid_w / 4;
        let start_y = self.grid_h / 2;

        self.body = vec![
            GridPoint {
                x: start_x,
                y: start_y,
            },
            GridPoint {
                x: start_x - 1,
                y: start_y,
            },
            GridPoint {
                x: start_x - 2,
                y: start_y,
            },
        ];
        self.dir = Direction::Right;
        self.next_dir = Direction::Right;
        self.apple = GridPoint {
            x: self.grid_w / 2,
            y: start_y,
        };
        self.score = 0;
        self.high_score = best;
        self.game_over = false;
        self.paused = false;
        self.move_timer = 0.0;
        self.step_interval = 0.11;
        self.head_scale = 1.0;
        self.particles.clear();
        if let Ok(mut audio) = AUDIO.lock() {
            audio.play_preset("powerup", 0.6, 1.2);
        }
    }

    /// Handles keyboard input with immediate 180-degree turn rejection.
    pub fn handle_input(&mut self) {
        if is_key_pressed(KeyCode::R) {
            self.restart();
            return;
        }

        if is_key_pressed(KeyCode::Space) {
            self.paused = !self.paused;
            return;
        }

        if self.game_over || self.paused {
            return;
        }

        let prev_dir = self.next_dir;
        if (is_key_pressed(KeyCode::Up) || is_key_pressed(KeyCode::W))
            && self.dir != Direction::Down
        {
            self.next_dir = Direction::Up;
        } else if (is_key_pressed(KeyCode::Down) || is_key_pressed(KeyCode::S))
            && self.dir != Direction::Up
        {
            self.next_dir = Direction::Down;
        } else if (is_key_pressed(KeyCode::Left) || is_key_pressed(KeyCode::A))
            && self.dir != Direction::Right
        {
            self.next_dir = Direction::Left;
        } else if (is_key_pressed(KeyCode::Right) || is_key_pressed(KeyCode::D))
            && self.dir != Direction::Left
        {
            self.next_dir = Direction::Right;
        }

        if self.next_dir != prev_dir
            && let Ok(mut audio) = AUDIO.lock()
        {
            audio.play_preset("click", 0.35, 1.2);
        }
    }

    /// Advances game simulation by delta time (dt).
    pub fn update(&mut self, dt: f32) {
        // Update particles
        for p in &mut self.particles {
            p.x += p.vx * dt;
            p.y += p.vy * dt;
            p.life -= dt;
        }
        self.particles.retain(|p| p.life > 0.0);

        // Smooth head scale back to 1.0 (Squash & stretch settle)
        if self.head_scale > 1.0 {
            self.head_scale = (self.head_scale - dt * 3.0).max(1.0);
        }

        if self.game_over || self.paused {
            return;
        }

        self.move_timer += dt;
        if self.move_timer < self.step_interval {
            return;
        }
        self.move_timer = 0.0;
        self.dir = self.next_dir;

        let head = self.body[0];
        let next_pos = match self.dir {
            Direction::Up => GridPoint {
                x: head.x,
                y: head.y - 1,
            },
            Direction::Down => GridPoint {
                x: head.x,
                y: head.y + 1,
            },
            Direction::Left => GridPoint {
                x: head.x - 1,
                y: head.y,
            },
            Direction::Right => GridPoint {
                x: head.x + 1,
                y: head.y,
            },
        };

        // Wall collision check
        if next_pos.x < 0
            || next_pos.x >= self.grid_w
            || next_pos.y < 0
            || next_pos.y >= self.grid_h
        {
            self.game_over = true;
            if let Ok(mut audio) = AUDIO.lock() {
                audio.play_preset("explosion", 0.9, 1.0);
            }
            return;
        }

        // Self body collision check
        if self.body.contains(&next_pos) {
            self.game_over = true;
            if let Ok(mut audio) = AUDIO.lock() {
                audio.play_preset("explosion", 0.9, 1.0);
            }
            return;
        }

        self.body.insert(0, next_pos);

        // Apple collection check
        if next_pos == self.apple {
            self.score += 10;
            self.head_scale = 1.4; // Squash & stretch juice
            if let Ok(mut audio) = AUDIO.lock() {
                audio.play_preset("coin", 0.85, 1.0);
            }

            // Spawn particles
            let px = (next_pos.x as f32 + 0.5) * self.cell_size;
            let py = (next_pos.y as f32 + 0.5) * self.cell_size;
            for _ in 0..12 {
                let angle = rand::gen_range(0.0, std::f32::consts::TAU);
                let speed = rand::gen_range(40.0, 140.0);
                self.particles.push(Particle {
                    x: px,
                    y: py,
                    vx: angle.cos() * speed,
                    vy: angle.sin() * speed,
                    life: rand::gen_range(0.2, 0.5),
                    color: Color::new(1.0, 0.85, 0.2, 1.0),
                });
            }

            // Relocate apple
            self.spawn_apple();

            // Slightly accelerate game speed as score grows
            if self.step_interval > 0.06 {
                self.step_interval -= 0.002;
            }
        } else {
            self.body.pop();
        }
    }

    fn spawn_apple(&mut self) {
        loop {
            let rx = rand::gen_range(1, self.grid_w - 2);
            let ry = rand::gen_range(1, self.grid_h - 2);
            let candidate = GridPoint { x: rx, y: ry };
            if !self.body.contains(&candidate) {
                self.apple = candidate;
                break;
            }
        }
    }

    /// Renders the complete game scene to the GPU buffer.
    pub fn draw(&self, offset_x: f32, offset_y: f32) {
        // Background playfield
        let board_w = self.grid_w as f32 * self.cell_size;
        let board_h = self.grid_h as f32 * self.cell_size;

        draw_rectangle(
            offset_x,
            offset_y,
            board_w,
            board_h,
            Color::new(0.07, 0.08, 0.12, 1.0),
        );
        draw_rectangle_lines(
            offset_x,
            offset_y,
            board_w,
            board_h,
            2.0,
            Color::new(0.2, 0.24, 0.35, 1.0),
        );

        // Draw grid dots
        for gx in 0..self.grid_w {
            for gy in 0..self.grid_h {
                let dot_x = offset_x + gx as f32 * self.cell_size + self.cell_size * 0.5;
                let dot_y = offset_y + gy as f32 * self.cell_size + self.cell_size * 0.5;
                draw_circle(dot_x, dot_y, 1.0, Color::new(0.12, 0.14, 0.20, 1.0));
            }
        }

        // Draw apple
        let apple_cx = offset_x + (self.apple.x as f32 + 0.5) * self.cell_size;
        let apple_cy = offset_y + (self.apple.y as f32 + 0.5) * self.cell_size;
        draw_circle(
            apple_cx,
            apple_cy,
            self.cell_size * 0.42,
            Color::new(0.95, 0.22, 0.26, 1.0),
        );
        draw_circle(
            apple_cx - 2.0,
            apple_cy - 2.0,
            self.cell_size * 0.14,
            Color::new(1.0, 0.5, 0.5, 0.8),
        );

        // Draw snake body
        for (i, seg) in self.body.iter().enumerate() {
            let px = offset_x + seg.x as f32 * self.cell_size;
            let py = offset_y + seg.y as f32 * self.cell_size;

            if i == 0 {
                // Head with squash & stretch scaling
                let size = self.cell_size * self.head_scale;
                let center_x = px + self.cell_size * 0.5;
                let center_y = py + self.cell_size * 0.5;
                draw_rectangle(
                    center_x - size * 0.5 + 1.0,
                    center_y - size * 0.5 + 1.0,
                    size - 2.0,
                    size - 2.0,
                    Color::new(0.15, 0.85, 0.45, 1.0),
                );

                // Eyes indicating direction
                let eye_color = Color::new(0.04, 0.12, 0.06, 1.0);
                let (e1_x, e1_y, e2_x, e2_y) = match self.dir {
                    Direction::Right => (
                        center_x + 3.0,
                        center_y - 4.0,
                        center_x + 3.0,
                        center_y + 4.0,
                    ),
                    Direction::Left => (
                        center_x - 3.0,
                        center_y - 4.0,
                        center_x - 3.0,
                        center_y + 4.0,
                    ),
                    Direction::Up => (
                        center_x - 4.0,
                        center_y - 3.0,
                        center_x + 4.0,
                        center_y - 3.0,
                    ),
                    Direction::Down => (
                        center_x - 4.0,
                        center_y + 3.0,
                        center_x + 4.0,
                        center_y + 3.0,
                    ),
                };
                draw_circle(e1_x, e1_y, 2.0, eye_color);
                draw_circle(e2_x, e2_y, 2.0, eye_color);
            } else {
                // Body segment with smooth gradient
                let factor = 1.0 - (i as f32 / (self.body.len() as f32 + 5.0));
                let col = Color::new(0.12 * factor, 0.70 * factor, 0.38 * factor, 1.0);
                draw_rectangle(
                    px + 2.0,
                    py + 2.0,
                    self.cell_size - 4.0,
                    self.cell_size - 4.0,
                    col,
                );
            }
        }

        // Draw particles
        for p in &self.particles {
            draw_circle(
                offset_x + p.x,
                offset_y + p.y,
                2.5 * (p.life / 0.5),
                p.color,
            );
        }

        // HUD Header
        draw_text(
            "AIPO SNAKE GAME — MINIOUAD NATIVE HOST",
            offset_x,
            offset_y - 20.0,
            20.0,
            WHITE,
        );
        let score_txt = format!(
            "SCORE: {:04}   HIGH: {:04}   LENGTH: {}",
            self.score,
            self.high_score.max(self.score),
            self.body.len()
        );
        draw_text(
            &score_txt,
            offset_x + board_w - 380.0,
            offset_y - 20.0,
            18.0,
            Color::new(0.3, 0.8, 1.0, 1.0),
        );

        // Controls footer
        let footer_txt =
            "Setas/WASD: Mover | Espaco: Pausar | R: Reiniciar | Passe um arquivo .aipo para rodar";
        draw_text(
            footer_txt,
            offset_x,
            offset_y + board_h + 24.0,
            15.0,
            Color::new(0.6, 0.65, 0.75, 1.0),
        );

        // Overlay: Game Over
        if self.game_over {
            draw_rectangle(
                offset_x,
                offset_y,
                board_w,
                board_h,
                Color::new(0.0, 0.0, 0.0, 0.75),
            );
            draw_text(
                "GAME OVER!",
                offset_x + board_w * 0.5 - 110.0,
                offset_y + board_h * 0.5 - 20.0,
                38.0,
                Color::new(0.95, 0.2, 0.25, 1.0),
            );
            draw_text(
                "Pressione [ R ] para jogar novamente",
                offset_x + board_w * 0.5 - 165.0,
                offset_y + board_h * 0.5 + 24.0,
                20.0,
                WHITE,
            );
        } else if self.paused {
            draw_rectangle(
                offset_x,
                offset_y,
                board_w,
                board_h,
                Color::new(0.0, 0.0, 0.0, 0.55),
            );
            draw_text(
                "PAUSADO",
                offset_x + board_w * 0.5 - 75.0,
                offset_y + board_h * 0.5,
                34.0,
                Color::new(1.0, 0.85, 0.2, 1.0),
            );
        }
    }
}

fn window_conf() -> Conf {
    Conf {
        window_title: "Aipo 2D Engine — Native Miniquad Host (60 FPS)".to_string(),
        window_width: 840,
        window_height: 680,
        window_resizable: true,
        high_dpi: true,
        ..Default::default()
    }
}

/// Discovers a callable function by trying candidates in globals or module function table.
fn find_callable(
    vm: &aipo_vm::Vm,
    module: &aipo_bytecode::BytecodeModule,
    names: &[&str],
) -> Option<Value> {
    for &name in names {
        if let Some(val) = vm.globals.get(name) {
            match val {
                Value::Function { .. } | Value::Closure(_) => return Some(val.clone()),
                _ => {}
            }
        }
        if let Some(info) = module.function_by_name(name) {
            return Some(Value::Function {
                entry_ip: info.entry_ip as u32,
                arity: info.params as u16,
                is_async: info.is_async,
            });
        }
    }
    None
}

/// Script runtime container managing the VM instance, compiled module, and lifecycle functions.
struct ScriptRuntime {
    module: aipo_bytecode::BytecodeModule,
    vm: aipo_vm::Vm,
    update_fn: Option<Value>,
    draw_fn: Option<Value>,
    last_error: Option<String>,
}

impl ScriptRuntime {
    fn load(path: &Path) -> Result<Self, String> {
        let mut host_surface = aipo_cli::prelude_surface();
        host_bridge::register_surface_symbols(&mut host_surface);

        let (_source, module) =
            aipo_cli::compile_file(path, Some(&host_surface)).map_err(|(src, diags)| {
                let mut buf = Vec::new();
                let mut err_buf = Vec::new();
                aipo_cli::emit_diagnostics(
                    aipo_diagnostics::MessageFormat::Human,
                    &src,
                    &diags,
                    &mut buf,
                    &mut err_buf,
                );
                String::from_utf8_lossy(&err_buf).into_owned()
            })?;

        let (mut vm, _) = aipo_cli::standard_environment();
        aipo_cli::register_module_symbols(&mut vm, &module);
        host_bridge::register_vm_natives(&mut vm);

        // Execute top-level script statements
        if let Err(err) = vm.run(&module) {
            return Err(format!("Initialization error: {err}"));
        }

        // Call init() if defined
        if let Some(init_fn) = find_callable(&vm, &module, &["init", "on_init", "setup"])
            && let Err(err) = vm.invoke(&module, init_fn, &[])
        {
            return Err(format!("init() error: {err}"));
        }

        let update_fn = find_callable(&vm, &module, &["update", "step", "on_update"]);
        let draw_fn = find_callable(&vm, &module, &["draw", "render", "on_draw"]);

        Ok(Self {
            module,
            vm,
            update_fn,
            draw_fn,
            last_error: None,
        })
    }
}

async fn run_script_game(script_path: &Path) {
    // Initialize Inter font for GPU rendering (replaces default ProggyClean)
    host_bridge::init_zoe_font();
    // Initialize GPU SDF analytical UI shaders (M9)
    host_bridge::init_zoe_shaders();

    let mut runtime_res = ScriptRuntime::load(script_path);

    loop {
        let dt = get_frame_time();

        // Hot reload script on F5 or Ctrl+R
        if is_key_pressed(KeyCode::F5)
            || (is_key_down(KeyCode::LeftControl) && is_key_pressed(KeyCode::R))
        {
            println!(
                "[aipo-game-host] Reloading script '{}'...",
                script_path.display()
            );
            runtime_res = ScriptRuntime::load(script_path);
        }

        // Exit on ESC
        if is_key_pressed(KeyCode::Escape) {
            break;
        }

        match &mut runtime_res {
            Ok(runtime) => {
                // Call update(dt) if available
                if let Some(ref update_fn) = runtime.update_fn {
                    let args = [Value::Float(dt as f64)];
                    if let Err(err) = runtime.vm.invoke(&runtime.module, update_fn.clone(), &args) {
                        runtime.last_error = Some(format!("Runtime error in update(): {err}"));
                    }
                }

                // Call draw() if available
                if let Some(ref draw_fn) = runtime.draw_fn {
                    if let Err(err) = runtime.vm.invoke(&runtime.module, draw_fn.clone(), &[]) {
                        runtime.last_error = Some(format!("Runtime error in draw(): {err}"));
                    }
                } else if runtime.update_fn.is_none() {
                    // Default screen if script only has top-level execution
                    clear_background(Color::new(0.06, 0.07, 0.10, 1.0));
                    draw_text("AIPO GAME HOST — RUNNING", 30.0, 50.0, 26.0, WHITE);
                    let info = format!("Script '{}' loaded and executed.", script_path.display());
                    draw_text(&info, 30.0, 90.0, 18.0, Color::new(0.4, 0.8, 1.0, 1.0));
                    draw_text(
                        "Define 'fn update(dt)' and 'fn draw()' for an interactive 60 FPS loop.",
                        30.0,
                        125.0,
                        16.0,
                        Color::new(0.7, 0.75, 0.85, 1.0),
                    );
                    draw_text(
                        "[F5 / Ctrl+R]: Recarregar Script | [ESC]: Fechar",
                        30.0,
                        screen_height() - 30.0,
                        16.0,
                        Color::new(0.5, 0.9, 0.6, 1.0),
                    );
                }

                // Render egui shapes overlay if any were generated this frame
                host_bridge::render_egui_overlay();

                // Display runtime error banner if one occurred
                if let Some(ref err_msg) = runtime.last_error {
                    draw_rectangle(
                        10.0,
                        screen_height() - 65.0,
                        screen_width() - 20.0,
                        48.0,
                        Color::new(0.85, 0.15, 0.15, 0.9),
                    );
                    draw_text(err_msg, 20.0, screen_height() - 35.0, 16.0, WHITE);
                }
            }
            Err(err_msg) => {
                clear_background(Color::new(0.12, 0.04, 0.04, 1.0));
                draw_text(
                    "AIPO SCRIPT ERROR",
                    30.0,
                    50.0,
                    28.0,
                    Color::new(0.95, 0.25, 0.25, 1.0),
                );
                let lines: Vec<&str> = err_msg.lines().take(18).collect();
                for (i, line) in lines.iter().enumerate() {
                    draw_text(line, 30.0, 95.0 + (i as f32 * 20.0), 15.0, WHITE);
                }
                draw_text(
                    "[F5 / Ctrl+R] para tentar recarregar após corrigir o código",
                    30.0,
                    screen_height() - 30.0,
                    16.0,
                    Color::new(0.95, 0.8, 0.2, 1.0),
                );
            }
        }

        next_frame().await;
    }
}

async fn run_builtin_snake() {
    let mut game = SnakeGameState::new(32, 24, 22.0);

    let board_w = game.grid_w as f32 * game.cell_size;
    let board_h = game.grid_h as f32 * game.cell_size;

    loop {
        let dt = get_frame_time();

        // Process inputs
        game.handle_input();

        // Advance simulation
        game.update(dt);

        // Render frame
        clear_background(Color::new(0.04, 0.04, 0.06, 1.0));

        let offset_x = ((screen_width() - board_w) * 0.5).max(20.0);
        let offset_y = ((screen_height() - board_h) * 0.5).max(40.0);

        game.draw(offset_x, offset_y);

        next_frame().await;
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Check if an .aipo script was passed as command line argument
    let script_arg = args.iter().skip(1).find(|arg| !arg.starts_with('-'));

    if let Some(script_path) = script_arg {
        let path = Path::new(script_path);
        println!("[aipo-game-host] Starting Aipo game: {}", path.display());
        run_script_game(path).await;
    } else {
        println!("[aipo-game-host] No script specified. Running built-in Snake Game demo.");
        println!(
            "[aipo-game-host] Tip: Run with `aipo-game-host <path/to/game.aipo>` to execute your custom game!"
        );
        run_builtin_snake().await;
    }
}
