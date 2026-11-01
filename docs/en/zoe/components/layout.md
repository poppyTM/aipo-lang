# Layout Containers

Structural containers for spatial arrangement with the Leona layout engine.

---

## `column`, `row`, `stack`, `center`

```text
# Vertical Column
zoe.column({ "gap": 10.0, "padding": 16.0 }, [
    zoe.label("Item A"),
    zoe.label("Item B")
])

# Horizontal Row with Font Baseline Alignment
zoe.row({ "align_items": "baseline", "gap": 8.0 }, [
    zoe.icon("lucide:star", { "size": 16.0 }),
    zoe.label("Featured", { "font_size": 14.0 })
])

# Overlapping Stack
zoe.stack({ "width": 400.0, "height": 300.0 }, [
    background_layer,
    foreground_badge
])
```

---

## `split_view` and `viewport`

- **`split_view`**: Resizable split pane with horizontal or vertical dividers.
- **`viewport`**: Hardware-clipped rendering container for 2D/3D scenes and camera projections.
