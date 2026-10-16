# Share the backoff helper with the RTU client

## Goal
Both Modbus clients retry with the same backoff.
Today the RTU client keeps its own copy of the delay calculation.

## Found
- cause
  - ferrowl-modbus rtu_client.rs duplicates `next_delay` from client_core.rs
- constraint
  - one helper, both clients call it
