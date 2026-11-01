# Campos de Entrada (Inputs)

Campos interativos calibrados para ferramentas técnicas e edição numérica.

---

## `scrubber_input`

Controle de precisão inspirado no Blender e Unreal Engine. Permite clicar e arrastar horizontalmente para alterar valores com suporte a modificadores táteis:
- **Arrasto padrão**: Incremento regular ($\times 0.2\times\text{step}$).
- <kbd>Shift</kbd> + **Arrasto**: Modo de micro-precisão ($\times 0.02\times\text{step}$).
- <kbd>Ctrl</kbd> + **Arrasto**: Salto rápido ($\times 2.0\times\text{step}$).

```text
let pos_x = zoe.use_state(0.0)

zoe.scrubber_input({
    "label": "X",
    "label_color": zoe.color.peach,
    "value": pos_x,
    "step": 1.0,
    "min": -500.0,
    "max": 500.0,
    "precision": 1,
    "width": "flex"
})
```

---

## `text_input`

Campo de texto alfanumérico com cursor piscante e gerenciamento de foco:

```text
let nome_entidade = zoe.use_state("Player")

zoe.text_input({
    "value": nome_entidade,
    "placeholder": "Digite o nome..."
})
```

---

## `dropdown_select` e `color_picker`

- **`dropdown_select`**: Seletor com menu suspenso em camada flutuante (*overlay*).
- **`color_picker`**: Seletor de cores da paleta Catppuccin com preview imediato.
