# Buttons & Selection Controls

Interactive components for triggering actions and toggling states.

---

## `button`

SDF button with inner top highlight, variant styling, and optional icons:

```text
zoe.button("Save Changes", on_save, {
    "variant": "primary",
    "icon": "lucide:save"
})

zoe.button("Delete Entity", on_delete, {
    "variant": "danger",
    "icon": "lucide:trash"
})
```

---

## `segmented_group`

Pill toggle group for mutually exclusive options:

```text
let mode = zoe.use_state("2d")

zoe.segmented_group([
    { "id": "2d", "label": "2D", "icon": "lucide:layers" },
    { "id": "3d", "label": "3D", "icon": "lucide:box" }
], mode)
```
