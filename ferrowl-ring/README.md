# Ring Crate

This crate provides `Ring<T, CAP>`: a fixed-capacity circular buffer generic over the element
type. Pushing into a full ring evicts the oldest item. Storage is a single inline
`[Option<T>; CAP]` array — no per-push heap allocation for the buffer itself.

Previously a log-specific buffer (`ferrowl-log`); the timestamping and line truncation that lived
here now belong to the caller (e.g. the per-module log pane wraps `Ring<(u64, String), N>`).
