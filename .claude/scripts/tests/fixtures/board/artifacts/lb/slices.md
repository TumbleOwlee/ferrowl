# lb — slices (rev 1)

## Slice 1: Walking skeleton
Goal: Two EVSEs share a site limit.
Depends on: none
Scenarios: S1
Entries: R1
Adds: `Device`, `Allocator`, `Node` with minimal bodies
Deferred: none

## Slice 2: Batteries
Goal: A battery covers demand above the limit.
Depends on: 1
Scenarios: S2
Entries: R2, E1
Adds: battery `Device` impl
Deferred: rounding of offered current

## Slice 3: Nested groups
Goal: Groups cap their members inside the site.
Depends on: 1, 2
Scenarios: S3
Entries: R3
Adds: group `Node` with a cap
Deferred: master–slave setups; rounding at group level
