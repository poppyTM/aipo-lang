# Navegação e Listas Hierárquicas

Componentes para alternância de abas e navegação em árvores de dados.

---

## `tab_view` e `tab_bar`

Conjunto de abas profissionais com transição de conteúdo:

```text
let aba_ativa = zoe.use_state("hierarquia")

zoe.tab_view(
    {
        "active": aba_ativa,
        "tabs": [
            { "id": "hierarquia", "label": "Hierarquia" },
            { "id": "assets", "label": "Arquivos" }
        ]
    },
    {
        "hierarquia": arvore_cenas,
        "assets": lista_arquivos
    }
)
```

---

## `hierarchy_tree`

Árvore de cena inspirada na Unity e Godot, com linhas de indentação, carets de expansão/colapso, badges semânticos de tipo (`3D`, `2D`, `CAM`, `LGT`) e seleção em linha inteira (*Fitts's Law*):

```text
let entidade_selecionada = zoe.use_state("heroi")

let estrutura = [
    {
        "id": "root",
        "label": "CenaPrincipal",
        "type": "3d",
        "children": [
            { "id": "camera", "label": "Camera3D", "type": "camera" },
            { "id": "heroi", "label": "Jogador", "type": "3d" }
        ]
    }
]

zoe.hierarchy_tree({
    "nodes": estrutura,
    "selected": entidade_selecionada
})
```

---

## `virtual_list`

Lista virtualizada de alto desempenho com janela deslizante de itens $O(1)$. Gera somente os elementos visíveis no viewport, suportando coleções de 100.000+ linhas sem degradação de FPS.
