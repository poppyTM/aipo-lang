# Reactivity & Signals in Zoe UI

Zoe UI adopts a signal-based reactivity model, delivering clean ergonomics without virtual DOM overhead.

---

## 1. `use_state` and `set_state`

Declare local reactive state using `zoe.use_state`:

```text
import aipo.zoe as zoe

fn view() {
    let name = zoe.use_state("Adventurer")
    let hp = zoe.use_state(100.0)

    return zoe.column({ "gap": 8.0 }, [
        zoe.label(f"Hero: {name.get()} (HP: {hp.get()})"),
        zoe.button("Take Damage", _ => {
            let next_hp = hp.get() - 15.0
            zoe.set_state(hp, if next_hp < 0.0 then 0.0 else next_hp)
        })
    ])
}
```

---

## 2. `use_memo`

Cache expensive calculations dependent on other signals:

```text
let total_items = zoe.use_memo(fn() {
    return inventory.get().len()
}, [inventory.get()])
```

---

## 3. `use_tween`

Animate smoothly with built-in tweening:

```text
let anim_progress = zoe.use_tween(0.0, 100.0, 0.5, "ease_out")
let current_val = anim_progress.get()
```
