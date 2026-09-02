# Non-Functional Requirements

Cross-cutting requirements belonging to no single area: platforms, toolchain, performance posture, security, versioning, testing conventions.

IDs stable, append-only (`NF-R-nnn`). See [`README.md`](./README.md).

Added via workflow in [`AGENTS.md`](../../AGENTS.md) — gate 1 approves "shall" text before code is written. Nothing here precedes that approval.

---

## Platforms and toolchain

*(Empty. First requirement lands via gate 1 — e.g. supported platforms, minimum language version + where declared, pinned toolchain.)*

## Performance

*(Empty. State a posture, not an unmeasurable number: what may allocate, what must not block, what's explicitly not optimised.)*

## Security

*(Empty. Input trust boundaries, what must never panic/allocate unbounded, dependency policy, unsafe-code policy.)*

## Versioning and release

*(Empty. Versioning scheme, what constitutes a breaking change, changelog policy.)*

## Testing conventions

*(Empty. Test naming, coverage floor + where enforced, port/filesystem discipline, what may not run in CI.)*
