# aipo.egui

Pacote oficial da linguagem **Aipo** para construção de interfaces gráficas em modo imediato (**Immediate-Mode GUI / IMGUI**) alimentado pelo motor **egui** de alta performance.

---

## Filosofia & Características

1. **Agnóstico de Plataforma e Motor Gráfico:**
   Diferente de bibliotecas acopladas a uma game engine específica, `aipo.egui` opera como um motor de UI puro. Ele recebe dimensões e entradas (mouse/teclado) e produz uma lista estruturada de formas geométricas (`shapes`) que pode ser desenhada por qualquer renderizador (GPU Miniquad, WebAssembly Canvas, Skia, ou em testes headless).

2. **Sintaxe Imediata Simples e Direta:**
   Sem necessidade de classes complexas ou reconciliação de árvores de componentes:
   ```text
   import aipo.egui as gui

   var ctx = gui.new_context()

   gui.begin_frame(ctx, { "width": 800.0, "height": 600.0, "dt": 0.016 })

   gui.window("Configurações", fn() {
       gui.heading("Painel")
       gui.separator()
       gui.label("Status: Operacional")

       if gui.button("Salvar") {
           io.println("Salvo!")
       }

       state.volume = gui.slider("Volume", state.volume, 0.0, 100.0)
       state.ativo = gui.checkbox("Habilitar", state.ativo)
   })

   var output = gui.end_frame(ctx)
   ```

3. **Handles Geracionais com Proteção em Tempo de Execução:**
   Sessões e janelas gerenciam seus ciclos de vida com segurança; acessos a contextos destruídos falham com código de erro limpo `AIPO_RT_STALE_HANDLE`.

---

## Instalação

Adicione ao seu `aipo.toml`:

```toml
[dependencies]
"aipo.egui" = { path = "packages/aipo-egui" }
```

---

## Módulos do Pacote

- [`lib.aipo`](src/lib.aipo): Ponto de entrada exportando o ciclo de vida (`new_context`, `destroy`, `begin_frame`, `end_frame`), containers (`window`, `window_at`) e widgets (`label`, `heading`, `separator`, `button`, `checkbox`, `slider`, `text_edit`, `progress_bar`, `wants_pointer`, `wants_keyboard`).
- [`ffi.aipo`](src/ffi.aipo): Contrato de integração FFI para o host Rust `crates/aipo-egui`.
