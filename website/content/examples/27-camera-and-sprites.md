---
title: "Câmera e sprites 2D"
description: Exemplo completo do repositório Aipo, com requisitos do host identificados.
---

# Câmera e sprites 2D

**Requer integração com o host de jogo/GUI:** as APIs de `host_*` e `egui` não fazem parte da CLI mínima. Fonte: [`examples/27_camera_and_sprites.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/examples/27_camera_and_sprites.aipo).

## Executar

```bash
cargo run -p aipo-game-host -- examples/27_camera_and_sprites.aipo
```

## Código completo

```aipo
# 27_camera_and_sprites.aipo — 2D World with Camera Follow & Visual Effects
# Run with: cargo run -p aipo-game-host -- examples/27_camera_and_sprites.aipo
#
# Demonstrates 2D camera tracking (target follow + zoom), large world boundaries,
# particle effects, collectible gems, and screen-space HUD overlay.

var player_x = 500.0
var player_y = 500.0
var player_vx = 0.0
var player_vy = 0.0
var player_size = 32.0

var cam_x = 500.0
var cam_y = 500.0
var cam_zoom = 1.0

# Collectibles: [x, y, collected]
var gems = [
    [300.0, 300.0, 0],
    [700.0, 300.0, 0],
    [300.0, 700.0, 0],
    [700.0, 700.0, 0],
    [500.0, 200.0, 0],
    [500.0, 800.0, 0],
    [200.0, 500.0, 0],
    [800.0, 500.0, 0]
]

var score = 0
var total_gems = 8
var game_time = 0.0

fn setup() {
    io.println("2D Camera & World demo initialized!")
    io.println("Controls: WASD / Arrows to Move, Z/X to Zoom In/Out")
}

fn update(dt) {
    game_time = game_time + dt

    let speed = 1200.0
    let friction = 7.0

    # Controls: WASD or Arrows
    if host_key_down(262) or host_key_down(68) { # Right / D
        player_vx = player_vx + speed * dt
    }
    if host_key_down(263) or host_key_down(65) { # Left / A
        player_vx = player_vx - speed * dt
    }
    if host_key_down(265) or host_key_down(87) { # Up / W
        player_vy = player_vy - speed * dt
    }
    if host_key_down(264) or host_key_down(83) { # Down / S
        player_vy = player_vy + speed * dt
    }

    # Camera Zoom: Z (90) to zoom in, X (88) to zoom out
    if host_key_down(90) {
        cam_zoom = cam_zoom + 0.8 * dt
    }
    if host_key_down(88) {
        cam_zoom = cam_zoom - 0.8 * dt
    }
    if cam_zoom < 0.5 {
        cam_zoom = 0.5
    }
    if cam_zoom > 2.0 {
        cam_zoom = 2.0
    }

    # Apply physics
    player_x = player_x + player_vx * dt
    player_y = player_y + player_vy * dt
    player_vx = player_vx - player_vx * friction * dt
    player_vy = player_vy - player_vy * friction * dt

    # World bounds [0, 1000]
    if player_x < 50.0 { player_x = 50.0 }
    if player_x > 950.0 { player_x = 950.0 }
    if player_y < 50.0 { player_y = 50.0 }
    if player_y > 950.0 { player_y = 950.0 }

    # Smooth camera follow (lerp)
    cam_x = cam_x + (player_x - cam_x) * dt * 6.0
    cam_y = cam_y + (player_y - cam_y) * dt * 6.0

    # Check gem collection
    var i = 0
    while i < total_gems {
        let gem = gems[i]
        if gem[2] == 0 {
            let gx = gem[0]
            let gy = gem[1]
            let dx = player_x - gx
            let dy = player_y - gy
            let dist_sq = dx * dx + dy * dy
            if dist_sq < 1400.0 {
                gem[2] = 1
                score = score + 100
                if score >= 800 {
                    host_play_preset("powerup", 1.0, 1.0)
                } else {
                    host_play_preset("coin", 0.9, 1.0)
                }
            }
        }
        i = i + 1
    }
}

fn draw() {
    # 1. World Space Rendering with Camera
    host_clear_background(0.08, 0.08, 0.12)
    host_set_camera(cam_x, cam_y, cam_zoom)

    # World Boundary grid
    host_draw_rect_lines(0.0, 0.0, 1000.0, 1000.0, 4.0, 0.3, 0.3, 0.5, 0.8)

    # Grid tiles
    var gx = 100.0
    while gx < 1000.0 {
        host_draw_rect_lines(gx, 0.0, 1.0, 1000.0, 1.0, 0.15, 0.15, 0.22, 0.5)
        gx = gx + 100.0
    }
    var gy = 100.0
    while gy < 1000.0 {
        host_draw_rect_lines(0.0, gy, 1000.0, 1.0, 1.0, 0.15, 0.15, 0.22, 0.5)
        gy = gy + 100.0
    }

    # Gems in world space
    var j = 0
    while j < total_gems {
        let gem = gems[j]
        if gem[2] == 0 {
            let gx = gem[0]
            let gy = gem[1]
            # Pulsating gem glow
            host_draw_circle(gx, gy, 14.0, 1.0, 0.8, 0.2, 0.9)
            host_draw_circle(gx, gy, 8.0, 1.0, 1.0, 0.6, 1.0)
        }
        j = j + 1
    }

    # Player in world space
    let px = player_x - player_size / 2.0
    let py = player_y - player_size / 2.0
    host_draw_rect(px, py, player_size, player_size, 0.2, 0.8, 0.4, 1.0)
    host_draw_rect_lines(px, py, player_size, player_size, 2.0, 0.8, 1.0, 0.8, 1.0)

    # 2. Screen Space Rendering (HUD Overlay)
    host_reset_camera()

    host_draw_rect(10.0, 10.0, 320.0, 70.0, 0.0, 0.0, 0.0, 0.65)
    host_draw_rect_lines(10.0, 10.0, 320.0, 70.0, 1.5, 0.4, 0.4, 0.7, 0.9)

    let score_str = f"Score: {score} / 800"
    host_draw_text(score_str, 24.0, 36.0, 22.0, 1.0, 1.0, 1.0)

    let zoom_percent = math.round(cam_zoom * 100.0)
    let zoom_str = f"Zoom: {zoom_percent}% (Z/X to adjust)"
    host_draw_text(zoom_str, 24.0, 64.0, 18.0, 0.7, 0.8, 1.0)

    if score >= 800 {
        let sw = host_screen_width()
        let sh = host_screen_height()
        host_draw_text("ALL GEMS COLLECTED! VICTORY!", sw / 2.0 - 200.0, sh / 2.0, 28.0, 1.0, 0.9, 0.2)
    }
}
```

Consulte o [código do host](https://github.com/poppyTM/aipo-lang/tree/main/crates/aipo-game-host) se o programa utilizar janela, entrada ou renderização. O código publicado não equivale à comprovação de execução neste commit.

[Todos os exemplos](/examples/) · [Manual](/manual/)
