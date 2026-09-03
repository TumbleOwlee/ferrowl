//! Wall-clock helpers.

use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch.
///
/// A system clock set before the epoch (misconfigured host) yields 0 rather than killing a
/// running simulation, hence `unwrap_or_default` rather than `expect`.
pub fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::now_unix_ms;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// The helper tracks the system clock: its value sits between two direct readings taken
    /// around the call.
    #[test]
    fn ut_now_unix_ms_tracks_system_clock() {
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let got = now_unix_ms();
        let after = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        assert!(
            before <= got && got <= after,
            "{before} <= {got} <= {after}"
        );
    }
}
