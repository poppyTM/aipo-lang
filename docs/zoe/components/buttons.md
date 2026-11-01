# Botões e Controles de Seleção

Componentes interativos para disparar ações e alternar modos.

---

## `button`

Botão com suporte a variantes táteis, ícones à esquerda e relevo em GPU com inner highlight de 1px.

```text
# Botão Simples
zoe.button("Salvar")

# Botão Primário com Ação e Ícone
zoe.button("Executar Jogo", on_play, {
    "variant": "primary",
    "icon": "lucide:play",
    "background": zoe.color.green
})

# Botão de Perigo
zoe.button("Excluir Entidade", on_delete, {
    "variant": "danger"
})
```

### Propriedades:
- `variant`: `"primary"` | `"secondary"` | `"ghost"` | `"danger"`
- `icon`: Identificador do ícone (ex: `"lucide:play"`, `"lucide:trash"`)
- `height`: Altura do botão (padrão: 32px)
- `tooltip`: Dica flutuante exibida ao pousar o cursor

---

## `segmented_group`

Controle em formato de pílula (*pill group*) para seleção mutuamente exclusiva de opções.

```text
let modo = zoe.use_state("2d")

zoe.segmented_group(
    [
        { "id": "2d", "label": "2D", "icon": "lucide:layers" },
        { "id": "3d", "label": "3D", "icon": "lucide:box" }
    ],
    modo
)
```

O item ativo recebe elevação visual e fundo contrastante automaticamente.
