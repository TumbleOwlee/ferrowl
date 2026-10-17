# demo — architecture summary (rev 1)

Decide: 1 new, 1 changed components in ferrowl-modbus; `ReconnectPolicy` owned by the client task

Interfaces: 1 new, 1 changed — sketches in architecture.md `## Interfaces`

| Component | Kind | Crate | Change | Role |
|---|---|---|---|---|
| `ReconnectPolicy` | struct | ferrowl-modbus | new | backoff 1s to 30s |
| `ConnectedCell` | struct | ferrowl-modbus | changed | holds a three-state `ConnState` |

Public surface:
- `ClientError::Reconnecting`

Open: none
