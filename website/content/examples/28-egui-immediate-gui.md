---
title: "GUI com egui"
description: Exemplo completo do repositório Aipo, com requisitos do host identificados.
---

# GUI com egui

**Requer integração com o host de jogo/GUI:** as APIs de `host_*` e `egui` não fazem parte da CLI mínima. Fonte: [`examples/28_egui_immediate_gui.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/examples/28_egui_immediate_gui.aipo).

## Executar

```bash
cargo run -p aipo-game-host -- examples/28_egui_immediate_gui.aipo
```

## Código completo

```aipo
# Exemplo 28: Immediate-Mode GUI Agnóstico com egui no Aipo Game Host
# Execute com: cargo run -p aipo-game-host -- examples/28_egui_immediate_gui.aipo

var ctx = egui.create_context()
var volume = 75.0
var som_ativo = true
var perfil = "AipoDev"
var cliques = 0
var progresso = 0.35

fn setup() {
    io.println("Ambiente gráfico Aipo + egui inicializado com sucesso!")
}

fn update(dt) {
    progresso = progresso + dt * 0.1
    if progresso > 1.0 {
        progresso = 0.0
    }
}

fn draw() {
    # 1. Fundo suave
    host_clear_background(0.06, 0.07, 0.11)

    let sw = host_screen_width()
    let sh = host_screen_height()
    let mx = host_mouse_x()
    let my = host_mouse_y()
    let mdown = host_mouse_btn(0)

    # 2. Inicia o frame imediato do egui
    egui.begin_frame(ctx, {
        "width": sw,
        "height": sh,
        "mouse_x": mx,
        "mouse_y": my,
        "mouse_down": mdown,
        "dt": 0.016
    })

    # 3. Janela 1: Preferências e Controles Interativos
    egui.begin_window("Configurações do Jogo", 40.0, 40.0, 360.0, 360.0)
    egui.heading("Painel de Preferências")
    egui.separator()

    egui.label("Status: Motor Ativo (60 FPS)")
    egui.label("Backend: Agnóstico (Renderizado via Macroquad)")
    egui.label(f"Mouse: ({Int(mx)}, {Int(my)}) | Pressionado: {mdown}")

    egui.separator()
    volume = egui.slider("Volume Principal", volume, 0.0, 100.0)
    som_ativo = egui.checkbox("Efeitos Sonoros", som_ativo)
    perfil = egui.text_edit("Perfil", perfil)

    if egui.button("Clique Aqui para Interagir") {
        cliques = cliques + 1
        host_play_preset("coin", 0.8, 1.0)
    }
    egui.label(f"Total de cliques: {cliques}")

    egui.separator()
    egui.label("Barra de Progresso:")
    egui.progress_bar(progresso)

    egui.end_window()

    # 4. Janela 2: Informações de Arquitetura ADP-010
    egui.begin_window("Arquitetura ADP-010", 430.0, 40.0, 370.0, 240.0)
    egui.heading("Agnostic Egui Host")
    egui.separator()
    egui.label("• Generational Handles (Zero Leaks)")
    egui.label("• #![forbid(unsafe_code)] estrito em 100% do código")
    egui.label("• Extração de primitivas vetoriais agnósticas")
    egui.label("• Sandbox deny-by-default com capacidade 'egui'")
    egui.separator()
    if egui.button("Zerar Contador de Cliques") {
        cliques = 0
    }
    egui.end_window()

    # 5. Finaliza o frame do egui (o overlay renderiza os shapes automaticamente)
    var output = egui.end_frame(ctx)

    # 6. Rodapé informativo
    host_draw_text("Aipo Language + egui — Interface gráfica imediata a 60 FPS", 30.0, sh - 25.0, 16.0, 0.6, 0.75, 0.95)
}
```

Consulte o [código do host](https://github.com/poppyTM/aipo-lang/tree/main/crates/aipo-game-host) se o programa utilizar janela, entrada ou renderização. O código publicado não equivale à comprovação de execução neste commit.

[Todos os exemplos](/examples/) · [Manual](/manual/)
