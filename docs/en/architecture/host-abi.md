# Host ABI & sandboxing

The **Host ABI (`aipo-host`)** defines the isolation and security interface between Aipo scripts
and the environment that hosts the runtime (whether a game, a backend application or an
intelligent agent).

---

## The deny-by-default permission model

By default, an Aipo script runs in a completely isolated environment:
- It cannot read environment variables (`env`).
- It cannot read or write the file system (`fs`).
- It cannot query the operating-system clock (`clock`).
- It cannot open network connections.

For a resource to be accessible, the host application in Rust must register an explicit
permission in the capability tree (`CapabilitySet`):

```rust
// Example in the Rust host: granting only clock and read access to a restricted directory
let mut caps = CapabilitySet::new();
caps.grant("clock");
caps.grant("fs.read");
```

If the script tries to access a function without the corresponding permission, the runtime
raises the deterministic failure `AIPO_RT_CAPABILITY_DENIED`.

---

## Generational handles against use-after-free

For complex host objects (such as UI windows, game entities or database connections), Aipo
uses the generational handle table (`HandleTable`):

- Each handle contains a **slot index** and a **generation number**.
- When the host object is destroyed or recycled, the slot's generation is incremented.
- Any later attempt by the script to access the old handle is rejected with
  `AIPO_RT_STALE_HANDLE`, eliminating dangling pointers and memory corruption.
