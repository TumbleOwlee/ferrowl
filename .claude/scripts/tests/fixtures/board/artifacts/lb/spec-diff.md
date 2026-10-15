# lb — spec diff (rev 2)

## Summary
Goal: A site limit is shared across chargers and batteries.
Area: ocpp — requirements.md `## Load balancing`

## S1 — Two EVSEs share a limit
Given a 32 A site limit and two charging EVSEs
When both request 32 A
Then each is offered 16 A

## S2 — Battery discharges at peak
Given a battery and one EVSE under a 32 A limit
When the EVSE requests 40 A
Then the battery covers the 8 A above the limit

## S3 — Nested group cap
Given a group capped at 20 A inside a 32 A site
When three EVSEs in the group charge
Then the group's EVSEs share 20 A

## R1 — new · requirements `## Load balancing`
The allocator never offers more than the site limit in total.

## R2 — new · requirements `## Load balancing`
A battery may discharge to cover demand above the site limit.

## R3 — new · requirements `## Load balancing`
A group's members never exceed the group's cap in total.

## E1 — new · edge-cases `## Load balancing`
A device that stops reporting keeps its last allocation for 60 s.

## Other spec changes
- ocpp/api-contract.md › Config keys › `site_limit_a` row (R1)
- ocpp/api-contract.md › Config keys › `group.cap_a` row (R3)
- ocpp/api-contract.md › Config keys › `legacy_limit` row (R12)
- ocpp/api-contract.md › Notes › units are amperes throughout

## Deferred
- rounding of offered current — implementation
- master–slave setups — plan
