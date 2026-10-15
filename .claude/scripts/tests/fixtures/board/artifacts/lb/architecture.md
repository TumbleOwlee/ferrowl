# lb — architecture (rev 1)

## Summary
Goal: A site limit is shared across chargers and batteries.
Shape: an `Allocator` over a tree of `Node`s

## Interfaces
### `Device` — new
```rust
pub trait Device {
    fn demand(&self) -> Amps;
    // a comment line
    fn can_discharge(&self) -> bool;
}
```

### `Allocator` — new
```rust
pub struct Allocator { root: Node }
```

## Interactions
### S1 — Two EVSEs share a limit
1. `Allocator::allocate` — splits 32 A
