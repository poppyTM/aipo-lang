# aipo.game

Pacote oficial da linguagem **Aipo** para desenvolvimento ágil de jogos 2D, com arquitetura orientada a **Atores e Cenas**, **Comportamentos (*Behaviors*) plugáveis em 1 linha**, **Sistema de Nós Visuais anti-espaguete** e simulação determinística de alta performance.

Inspirado nas melhores ideias de fluxo e facilidade de motores como **Construct 3**, **ct.js** e **GameMaker Studio**, o `aipo.game` remove toda a cerimônia técnica de game dev sem sacrificar a robustez e a tipagem forte do ecossistema Aipo.

---

## Características Principais

1. **Atores e Cenas Claros:**
   Cada elemento do jogo é um `Actor` modular com ciclo de vida intuitivo (`on_create`, `on_update`, `on_collision`, `on_destroy`), posicionado dentro de uma `Scene` com suporte a câmeras e camadas de paralaxe.
2. **Behaviors Pré-Fabricados em 1 Linha:**
   Adicione movimentação completa de plataforma (`Platformer`), movimentação top-down (`TopDown`), física de projétil (`Bullet`) ou barreiras intransponíveis (`Solid`) instantaneamente.
3. **Simulação Determinística:**
   Graças ao sistema de tipos da Aipo e ao controle estrito de efeitos, a simulação matemática do jogo é 100% determinística — habilitando **Rollback Netcode** nativo para multiplayer sem esforço de sincronização e saves/replays de poucos kilobytes.
4. **Sistema de Nós Visuais Anti-Espaguete:**
   Esquema arquitetural "Gatilho $\to$ Filtro $\to$ Ação" que previne emaranhados de fios e compila bidirecionalmente para código `.aipo` transparente e limpo.
5. **Backend Multiplataforma Nativo:**
   Compatível com o runtime Rust nativo (Desktop via Skia/Miniquad com GPU acelerada) e WebAssembly (WebGL2/Canvas 2D para navegadores) a partir da mesma base de código.

---

## Instalação

Adicione ao seu `aipo.toml`:

```toml
[dependencies]
"aipo.game" = { path = "packages/aipo-game" }
```

---

## Exemplo Rápido: Nave vs Asteroides

```text
import aipo.game as g

# Ator Jogador
let Nave = g.actor("Nave", {
    sprite: "nave.png",
    behaviors: [
        g.behaviors.TopDown(240.0, true),
        g.behaviors.KeepInScreen(0.0)
    ],

    on_create: fn(actor) {
        actor.custom["vida"] = 100
        actor.custom["cooldown"] = 0.0
    },

    on_update: fn(actor, dt) {
        actor.custom["cooldown"] -= dt
        if g.input.key_down("Space") and actor.custom["cooldown"] <= 0.0 {
            g.spawn(Laser, actor.x, actor.y - 20.0)
            g.audio.play("laser.wav")
            actor.custom["cooldown"] = 0.15
        }
    },

    on_collision: fn(actor, other) {
        if other.type_name == "Asteroide" {
            actor.custom["vida"] -= 25
            other.is_alive = false
        }
    }
})

# Ator Laser
let Laser = g.actor("Laser", {
    sprite: "laser.png",
    behaviors: [
        g.behaviors.Bullet(600.0, -90.0),
        g.behaviors.DestroyOutsideScreen(50.0)
    ]
})

# Ator Asteroide
let Asteroide = g.actor("Asteroide", {
    sprite: "asteroide.png",
    behaviors: [
        g.behaviors.Bullet(120.0, 90.0),
        g.behaviors.DestroyOutsideScreen(50.0)
    ]
})

# Cena Principal
let Jogo = g.scene("Espaco", {
    width: 800.0,
    height: 600.0,
    background: "#0a0a12",

    on_load: fn(scene) {
        g.spawn(Nave, 400.0, 520.0)
    }
})

# Inicialização
fn main() {
    g.start({
        "title": "Space Defender — Aipo Game",
        "width": 800,
        "height": 600,
        "initial_scene": Jogo
    })
}
```

---

## Subsistemas Avançados

### 1. In-Game UI / HUD em Modo Imediato (`g.ui`)
Interface imediata de altíssimo desempenho integrada diretamente no laço de renderização do jogo:
- `g.ui.button(x, y, w, h, text) -> Bool`: botões responsivos com estados normal, hover e pressionado.
- `g.ui.health_bar(x, y, w, h, current, max)`: barra de vida dinâmica com transição visual automática de cores (verde $\to$ amarelo $\to$ vermelho).
- `g.ui.progress_bar(x, y, w, h, current, max, r, g, b)`: barra genérica para recursos (estamina, mana, escudo, progresso).
- `g.ui.panel(x, y, w, h, title)`: janelas e molduras decorativas para inventários, diálogos e painéis de pause.
- `g.ui.label(x, y, text, size, r, g, b)`: renderização tipográfica com sombra de alto contraste.
- `g.ui.badge(x, y, text, r, g, b)`: tags e contadores em formato de pílula arredondada.

### 2. Grid Tilemap 2D e Colisão Contínua (`g.tilemap`)
Gerenciamento de mundos baseados em malhas de blocos e colisão de alta velocidade:
- `g.tilemap.create_tilemap(cols, rows, tile_size, tex_id, tileset_cols)`: criação e alocação de mapas tabulares.
- `g.tilemap.set_solid(map, col, row, is_solid)` / `is_solid_cell(map, col, row)`: marcação e consulta $O(1)$ de solidez.
- `g.tilemap.resolve_box_collision(map, x, y, w, h, vx, vy, dt)`: resolvedor de colisão contínua (*swept AABB*) com separação de eixos $X$ e $Y$, evitando atravessamento de paredes (*tunneling*) mesmo em velocidades extremas.
- `g.tilemap.draw_tilemap(map, cam_x, cam_y, zoom, screen_w, screen_h)`: renderização com descarte automático de blocos fora da visão da câmera (*viewport frustum culling*).
- `g.tilemap.raycast(map, x1, y1, x2, y2) -> Dict`: algoritmo DDA de traçado de raios em grade discreta para linhas de visada, tiros e sensores de IA.

### 3. Máquina de Estados e Animação de Sprites (`g.animation`)
Controle quadro-a-quadro de spritesheets com suporte a transições e espelhamento horizontal:
- `g.animation.create_clip(name, frames, fps, is_looping, tile_w, tile_h, tileset_cols)`: definição de clipe de animação com recortes automáticos de spritesheet.
- `g.animation.create_animator(texture_id)`: instanciação do animador para um ator ou entidade gráfica.
- `g.animation.add_clip(anim, clip)` / `g.animation.play(anim, clip_name, reset_if_playing)`: registro de clipes e despacho de estados (ex: "idle", "run", "attack").
- `g.animation.update(anim, dt)`: avanço temporal exato por taxa de quadros (*FPS*), com transição suave e travamento em último quadro para clipes não-contínuos (*one-shot*).
- `g.animation.draw(anim, x, y, width, height, rotation, flip_x)`: renderização na GPU com recorte UV preciso e espelhamento em $X$ instantâneo.
- `g.animation.get_current_frame(anim)` / `is_finished(anim)` / `current_clip(anim)`: introspecção de estado para lógica de jogo.

### 4. Sistema de Partículas 2D e Efeitos Físicos (`g.particles`)
Gerador de partículas de alta densidade com simulação física, arrasto e atenuação de opacidade:
- `g.particles.create_emitter(x, y, max_particles)`: emissor com limite configurável de saturação e alocação dinâmica protegida.
- `g.particles.emit_preset(emitter, preset_name, x, y, count)`: presets de efeitos de jogo pré-configurados (`"sparks"`, `"explosion"`, `"smoke"`, `"dust"`, `"coins"`, `"trail"`).
- `g.particles.emit(emitter, count, config)`: emissão sob medida customizando velocidade, ângulo, vida útil, curvas de tamanho, cor RGBA, gravidade e arrasto.
- `g.particles.update(emitter, dt)`: atualização integrada de cinemática e descarte determinístico de partículas expiradas.
- `g.particles.draw(emitter)`: desenho na GPU com cálculo contínuo de escala e atenuação linear de alpha.

---

## Execução Nativa a 60 FPS (`aipo-game-host`)

O runtime nativo em `crates/aipo-game-host` permite compilar e executar qualquer jogo `.aipo` diretamente na GPU:

```bash
# Executa a demonstração de Animação de Sprites e Partículas 2D
cargo run -p aipo-game-host -- packages/aipo-game/examples/animation_and_particles.aipo

# Executa a demonstração de Tilemap 2D, Câmera e HUD Imediato
cargo run -p aipo-game-host -- packages/aipo-game/examples/tilemap_and_hud.aipo

# Executa exemplos interativos adicionais em tempo real
cargo run -p aipo-game-host -- examples/26_interactive_game.aipo
cargo run -p aipo-game-host -- examples/27_camera_and_sprites.aipo

# Executa o Snake Game nativo embutido
cargo run -p aipo-game-host
```



