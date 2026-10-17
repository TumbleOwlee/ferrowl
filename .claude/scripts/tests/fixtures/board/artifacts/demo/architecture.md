# demo — architecture (rev 1)

## Components

| Component | Kind | Crate › module | Change | Role |
|---|---|---|---|---|
| `ReconnectPolicy` | struct | ferrowl-modbus › client::reconnect | new | backoff 1s to 30s, capped by `max_backoff` |
| `ConnectedCell` | struct | ferrowl-modbus › client | changed | holds a three-state `ConnState` (`Connected`, `Reconnecting`, `Disconnected`) the tab renders |

## Interfaces

### `ReconnectPolicy` — new
```rust
pub struct ReconnectPolicy { pub max_backoff: Duration }
```
