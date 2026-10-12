---
title: "Snake no terminal"
description: Exemplo completo do repositório Aipo, com requisitos do host identificados.
---

# Snake no terminal

Programa de terminal que usa o runtime Aipo. Fonte: [`examples/25_snake_game.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/examples/25_snake_game.aipo).

## Executar

```bash
cargo run -p aipo-cli -- run examples/25_snake_game.aipo
```

## Código completo

```aipo
# 25_snake_game.aipo — Classic Snake Game Simulation
# Demonstrates structs, methods with `var self`, lists, 2D coordinates,
# collision logic, scoring, and terminal visualization.

struct Point {
    x
    y
}

struct SnakeGame {
    width
    height
    var body = []
    var dir_x = 1
    var dir_y = 0
    var apple = Point{ x: 5, y: 3 }
    var score = 0
    var is_game_over = false
    var step_count = 0
}

SnakeGame:is_snake_body(self, x, y) {
        var idx = 0
        each segment in self.body {
            if segment.x == x and segment.y == y {
                return true
            }
            idx = idx + 1
        }
        return false
    }

    SnakeGame:change_direction(var self, dx, dy) {
        # Prevent 180-degree immediate reversal into own neck
        if (dx != 0 and self.dir_x == -dx) or (dy != 0 and self.dir_y == -dy) {
            return false
        }
        self.dir_x = dx
        self.dir_y = dy
        return true
    }

    SnakeGame:step(var self) {
        if self.is_game_over {
            return false
        }

        self.step_count = self.step_count + 1
        let head = self.body[0]
        let next_x = head.x + self.dir_x
        let next_y = head.y + self.dir_y

        # 1. Wall Collision Check
        if next_x < 0 or next_x >= self.width or next_y < 0 or next_y >= self.height {
            self.is_game_over = true
            return false
        }

        # 2. Self Collision Check
        if self.is_snake_body(next_x, next_y) {
            self.is_game_over = true
            return false
        }

        let new_head = Point{ x: next_x, y: next_y }
        self.body.insert(0, new_head)

        # 3. Apple Eaten Check
        if next_x == self.apple.x and next_y == self.apple.y {
            self.score = self.score + 10
            # Relocate apple to fixed next spot for deterministic testing
            if self.apple.x == 5 and self.apple.y == 3 {
                self.apple = Point{ x: 8, y: 3 }
            } elif self.apple.x == 8 and self.apple.y == 3 {
                self.apple = Point{ x: 8, y: 6 }
            } else {
                self.apple = Point{ x: 2, y: 2 }
            }
        } else {
            # Move tail forward (keep length constant)
            self.body.remove_last()
        }

        return true
    }

    SnakeGame:render(self) {
        io.println(f"=== Snake Step: {self.step_count} | Score: {self.score} | Length: {len(self.body)} ===")

        # Top border
        var top_border = "+"
        var col = 0
        while col < self.width {
            top_border = top_border + "-"
            col = col + 1
        }
        top_border = top_border + "+"
        io.println(top_border)

        # Grid rows
        var y = 0
        while y < self.height {
            var row_str = "|"
            var x = 0
            while x < self.width {
                let head = self.body[0]
                if x == head.x and y == head.y {
                    row_str = row_str + "@" # Head
                } elif self.is_snake_body(x, y) {
                    row_str = row_str + "o" # Body
                } elif x == self.apple.x and y == self.apple.y {
                    row_str = row_str + "*" # Apple
                } else {
                    row_str = row_str + "." # Empty space
                }
                x = x + 1
            }
            row_str = row_str + "|"
            io.println(row_str)
            y = y + 1
        }

        # Bottom border
        io.println(top_border)
        if self.is_game_over {
            io.println("GAME OVER! A cobra colidiu!")
        } else {
            io.println("Status: Viva e em movimento!")
        }
        io.println("")
    }

# --- Inicialização da Partida ---
var game = SnakeGame{
    width: 12,
    height: 8,
    body: [
        Point{ x: 3, y: 3 },
        Point{ x: 2, y: 3 },
        Point{ x: 1, y: 3 }
    ],
    dir_x: 1,
    dir_y: 0,
    apple: Point{ x: 5, y: 3 }
}

io.println("--- ESTADO INICIAL DO JOGO ---")
game.render()

# 1. Avançar para a direita até comer a primeira maçã (x=5, y=3)
io.println("-> Movendo para a direita...")
game.step()
game.render()

io.println("-> Comendo a primeira maca...")
game.step()
game.render()

# 2. Virar para a direita em direção à segunda maçã (x=8, y=3)
io.println("-> Continuando em direcao a segunda maca...")
game.step()
game.step()
game.step()
game.render()

# 3. Virar para baixo (dy = 1) em direção à terceira maçã (x=8, y=6)
io.println("-> Virando para baixo...")
game.change_direction(0, 1)
game.step()
game.step()
game.step()
game.render()

io.println(f"Fim da simulacao: Pontuacao final = {game.score}, Tamanho = {len(game.body)}")
```

Consulte o [código do host](https://github.com/poppyTM/aipo-lang/tree/main/crates/aipo-game-host) se o programa utilizar janela, entrada ou renderização. O código publicado não equivale à comprovação de execução neste commit.

[Todos os exemplos](/examples/) · [Manual](/manual/)
