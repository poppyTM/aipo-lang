---
title: "Jogo interativo"
description: Exemplo completo do repositório Aipo, com requisitos do host identificados.
---

# Jogo interativo

**Requer integração com o host de jogo/GUI:** as APIs de `host_*` e `egui` não fazem parte da CLI mínima. Fonte: [`examples/26_interactive_game.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/examples/26_interactive_game.aipo).

## Executar

```bash
cargo run -p aipo-game-host -- examples/26_interactive_game.aipo
```

## Código completo

```aipo
# 26_interactive_game.aipo — Real-time Interactive 2D Game in Aipo
# Run with: cargo run -p aipo-game-host -- examples/26_interactive_game.aipo
#
# Demonstrates 60 FPS GPU game loop, real-time keyboard input,
# dynamic player movement, coin collection, and procedural HUD.

var player_x = 420.0
var player_y = 340.0
var player_vx = 0.0
var player_vy = 0.0
var player_size = 28.0

var coin_x = 250.0
var coin_y = 200.0
var coin_timer = 0.0

var score = 0
var game_time = 0.0

fn setup() {
    io.println("Aipo 2D Game initialized from .aipo script!")
}

fn update(dt) {
    game_time = game_time + dt
    coin_timer = coin_timer + dt * 4.0

    let accel = 1400.0
    let friction = 8.0

    # Input: Arrow keys or WASD (Raylib/GLFW Keycodes)
    # Right: 262 or 68 ('D')
    if host_key_down(262) or host_key_down(68) {
        player_vx = player_vx + accel * dt
    }
    # Left: 263 or 65 ('A')
    if host_key_down(263) or host_key_down(65) {
        player_vx = player_vx - accel * dt
    }
    # Up: 265 or 87 ('W')
    if host_key_down(265) or host_key_down(87) {
        player_vy = player_vy - accel * dt
    }
    # Down: 264 or 83 ('S')
    if host_key_down(264) or host_key_down(83) {
        player_vy = player_vy + accel * dt
    }

    # Apply velocity and damping
    player_x = player_x + player_vx * dt
    player_y = player_y + player_vy * dt
    player_vx = player_vx - player_vx * friction * dt
    player_vy = player_vy - player_vy * friction * dt

    # Screen boundary constraints
    let sw = host_screen_width()
    let sh = host_screen_height()

    if player_x < player_size {
        player_x = player_size
        if player_vx < -150.0 {
            host_play_preset("hit", 0.6, 1.2)
        }
        player_vx = 0.0
    }
    if player_x > sw - player_size {
        player_x = sw - player_size
        if player_vx > 150.0 {
            host_play_preset("hit", 0.6, 1.2)
        }
        player_vx = 0.0
    }
    if player_y < player_size + 40.0 {
        player_y = player_size + 40.0
        if player_vy < -150.0 {
            host_play_preset("hit", 0.6, 1.2)
        }
        player_vy = 0.0
    }
    if player_y > sh - player_size - 40.0 {
        player_y = sh - player_size - 40.0
        if player_vy > 150.0 {
            host_play_preset("hit", 0.6, 1.2)
        }
        player_vy = 0.0
    }

    # Coin pickup collision check
    let dx = player_x - coin_x
    let dy = player_y - coin_y
    let dist_sq = dx * dx + dy * dy

    if dist_sq < 32.0 * 32.0 {
        score = score + 100
        host_play_preset("coin", 0.9, 1.0)
        # Relocate coin procedurally
        coin_x = 100.0 + (Float(score * 37 % 600))
        coin_y = 100.0 + (Float(score * 53 % 450))
    }
}

fn draw() {
    # 1. Background clear
    host_clear_background(0.05, 0.06, 0.10)

    let sw = host_screen_width()
    let sh = host_screen_height()

    # 2. Draw arena border
    host_draw_rect_lines(20.0, 40.0, sw - 40.0, sh - 80.0, 2.0, 0.20, 0.25, 0.38, 1.0)

    # 3. Draw coin with pulsing radius
    let pulse_rad = 12.0 + math.sin(coin_timer) * 3.0
    host_draw_circle(coin_x, coin_y, pulse_rad + 4.0, 1.0, 0.85, 0.2, 0.35)
    host_draw_circle(coin_x, coin_y, pulse_rad, 1.0, 0.85, 0.15, 1.0)
    host_draw_circle(coin_x - 2.0, coin_y - 2.0, pulse_rad * 0.35, 1.0, 1.0, 0.6, 0.9)

    # 4. Draw player avatar
    let half = player_size * 0.5
    host_draw_rect(player_x - half, player_y - half, player_size, player_size, 0.15, 0.85, 0.50, 1.0)
    host_draw_rect_lines(player_x - half, player_y - half, player_size, player_size, 1.5, 0.40, 1.0, 0.70, 1.0)

    # Draw player eyes
    host_draw_circle(player_x - 4.0, player_y - 3.0, 2.5, 0.05, 0.15, 0.08, 1.0)
    host_draw_circle(player_x + 4.0, player_y - 3.0, 2.5, 0.05, 0.15, 0.08, 1.0)

    # 5. Draw HUD header
    host_draw_text("AIPO 2D ENGINE — LIVE SCRIPT EXECUTION", 30.0, 28.0, 20.0, 1.0, 1.0, 1.0)

    let score_text = "SCORE: " + String(score)
    host_draw_text(score_text, sw - 180.0, 28.0, 20.0, 0.3, 0.85, 1.0)

    # 6. Controls footer
    host_draw_text("WASD / Setas: Mover jogador  |  [F5 / Ctrl+R]: Hot Reload do script  |  [ESC]: Sair", 30.0, sh - 18.0, 15.0, 0.6, 0.65, 0.75)
}
```

Consulte o [código do host](https://github.com/poppyTM/aipo-lang/tree/main/crates/aipo-game-host) se o programa utilizar janela, entrada ou renderização. O código publicado não equivale à comprovação de execução neste commit.

[Todos os exemplos](/examples/) · [Manual](/manual/)
