# Navigation & Tree Lists

Components for tab switching and hierarchical data inspection.

---

## `tab_view`

Professional tab views with content switching:

```text
let active_tab = zoe.use_state("scene")

zoe.tab_view(
    {
        "active": active_tab,
        "tabs": [
            { "id": "scene", "label": "Scene" },
            { "id": "assets", "label": "Assets" }
        ]
    },
    {
        "scene": scene_tree,
        "assets": asset_list
    }
)
```

---

## `hierarchy_tree`

Scene hierarchy tree with rail guide lines, expand/collapse carets, semantic type badges (`3D`, `2D`, `CAM`, `LGT`), and full-width row selection:

```text
let selected = zoe.use_state("player")

let nodes = [
    {
        "id": "root",
        "label": "RootScene",
        "type": "3d",
        "children": [
            { "id": "camera", "label": "MainCamera", "type": "camera" },
            { "id": "player", "label": "Player", "type": "3d" }
        ]
    }
]

zoe.hierarchy_tree({
    "nodes": nodes,
    "selected": selected
})
```

---

## `virtual_list`

Virtual scrolling list providing $O(1)$ windowed element rendering for large collections of 100,000+ items.
