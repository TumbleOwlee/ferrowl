# lb — architecture summary (rev 1)

Decide: 3 new components in ferrowl-ocpp; an `Allocator` over a tree of `Node`s

Interfaces: 3 new — sketches in architecture.md `## Interfaces`

| Component | Kind | Crate | Change | Role |
|---|---|---|---|---|
| `Allocator` | struct | ferrowl-ocpp | new | splits a limit over the tree |
| `Device` | trait | ferrowl-ocpp | new | demand and capability of one device |

Public surface:
- config key `site_limit_a`

Open: none
