# Reconnect dropped Modbus clients

## Why
A client tab reconnects on its own.
Today the user restarts the tab by hand.

## Changes
- ferrowl-modbus client_core — retry with backoff (MB-R-161)
