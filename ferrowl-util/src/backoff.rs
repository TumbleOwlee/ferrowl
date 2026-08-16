//! Shared exponential-backoff retry driver, extracted so every reconnecting task in the
//! workspace (Modbus client, Modbus server x6 transports, OCPP CS, OCPP CSMS) runs the same
//! policy and the same retry/backoff/abort state machine instead of re-implementing it per
//! call site.

use std::future::Future;
use std::time::Duration;

/// The reconnect backoff policy (MB-R-051): starts at `initial`, doubles after each failed
/// attempt, capped at `max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackoffPolicy {
    pub initial: Duration,
    pub max: Duration,
}

impl Default for BackoffPolicy {
    fn default() -> Self {
        Self {
            initial: Duration::from_secs(1),
            max: Duration::from_secs(30),
        }
    }
}

/// Outcome of one attempt, as reported by the caller's `attempt` closure passed to
/// [`run_with_backoff`].
#[derive(Debug)]
pub enum AttemptOutcome<E> {
    /// The task ended on its own terms (graceful terminate/channel close observed by the
    /// caller's own attempt) — stop retrying, [`run_with_backoff`] returns `Ok(())`.
    Done,
    /// The attempt failed. `reconnect` is this attempt's freshly-read config flag (callers
    /// re-read it every attempt so an edit takes effect without a restart). `reset` says
    /// whether the backoff should drop back to `initial` before the wait that follows *this*
    /// failure (something useful happened during this run before it failed).
    Failed {
        error: E,
        reconnect: bool,
        reset: bool,
    },
}

/// Drives a caller's `attempt` closure in a loop, applying [`BackoffPolicy`] between failures
/// and calling `wait_abortable` to sleep out each backoff. `wait_abortable(backoff)` must race
/// the sleep against whatever abort signal the caller owns (a command channel's `recv()`, e.g.)
/// and return `true` if aborted early, `false` if the sleep elapsed naturally; returning `true`
/// ends `run_with_backoff` with `Ok(())`.
///
/// With `reconnect: false` on a failed attempt, the loop ends immediately with `Err(error)`,
/// without ever calling `wait_abortable`.
pub async fn run_with_backoff<E, A, Fut, W, WFut>(
    policy: BackoffPolicy,
    mut attempt: A,
    mut wait_abortable: W,
) -> Result<(), E>
where
    A: FnMut() -> Fut,
    Fut: Future<Output = AttemptOutcome<E>>,
    W: FnMut(Duration) -> WFut,
    WFut: Future<Output = bool>,
{
    let mut backoff = policy.initial;
    loop {
        match attempt().await {
            AttemptOutcome::Done => return Ok(()),
            AttemptOutcome::Failed {
                error,
                reconnect,
                reset,
            } => {
                if !reconnect {
                    return Err(error);
                }
                // `reset` applies before the wait it governs: a run that got something useful
                // done before failing drops straight back to `initial` for *this* wait, not
                // just the next one.
                if reset {
                    backoff = policy.initial;
                }
                if wait_abortable(backoff).await {
                    return Ok(());
                }
                backoff = (backoff * 2).min(policy.max);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// MB-R-051 — the reconnect backoff starts at 1s, doubles after each failed attempt, and is
    /// capped at 30s. `BackoffPolicy::default()` carries that policy for every caller.
    #[test]
    fn ut_backoff_default_policy_matches_mb_r_051() {
        let policy = BackoffPolicy::default();
        assert_eq!(policy.initial, Duration::from_secs(1));
        assert_eq!(policy.max, Duration::from_secs(30));
    }

    #[tokio::test]
    /// MB-R-051 — repeated failures (no reset) double the wait each time, capped at 30s.
    async fn ut_run_with_backoff_doubles_and_caps() {
        let waits: Arc<Mutex<Vec<Duration>>> = Arc::new(Mutex::new(Vec::new()));
        let waits_recorder = waits.clone();
        let calls = Arc::new(Mutex::new(0u32));
        let calls_counter = calls.clone();

        let attempt = || {
            let calls_counter = calls_counter.clone();
            async move {
                *calls_counter.lock().unwrap() += 1;
                AttemptOutcome::Failed::<&'static str> {
                    error: "boom",
                    reconnect: true,
                    reset: false,
                }
            }
        };
        let wait_abortable = move |backoff: Duration| {
            let waits_recorder = waits_recorder.clone();
            let calls = calls.clone();
            async move {
                waits_recorder.lock().unwrap().push(backoff);
                // Abort once the 8th wait has been recorded, ending the loop.
                *calls.lock().unwrap() >= 8
            }
        };

        let result = run_with_backoff(BackoffPolicy::default(), attempt, wait_abortable).await;
        assert_eq!(result, Ok(()));

        let secs: Vec<u64> = waits
            .lock()
            .unwrap()
            .iter()
            .map(Duration::as_secs)
            .collect();
        assert_eq!(secs, vec![1, 2, 4, 8, 16, 30, 30, 30]);
    }

    #[tokio::test]
    /// MB-R-051/MB-R-132 — a `reset: true` outcome drops the wait that follows *that* failure
    /// back to `initial`; a `reset: false` outcome keeps doubling from wherever backoff last
    /// landed.
    async fn ut_run_with_backoff_resets_on_reset_flag() {
        let resets = [false, true, false];
        let index = Arc::new(Mutex::new(0usize));
        let index_for_attempt = index.clone();
        let attempt = move || {
            let index_for_attempt = index_for_attempt.clone();
            async move {
                let mut i = index_for_attempt.lock().unwrap();
                let reset = resets.get(*i).copied().unwrap_or(false);
                *i += 1;
                AttemptOutcome::Failed::<&'static str> {
                    error: "boom",
                    reconnect: true,
                    reset,
                }
            }
        };

        let waits: Arc<Mutex<Vec<Duration>>> = Arc::new(Mutex::new(Vec::new()));
        let waits_recorder = waits.clone();
        let wait_abortable = move |backoff: Duration| {
            let waits_recorder = waits_recorder.clone();
            async move {
                let mut w = waits_recorder.lock().unwrap();
                w.push(backoff);
                w.len() >= 4
            }
        };

        let result = run_with_backoff(BackoffPolicy::default(), attempt, wait_abortable).await;
        assert_eq!(result, Ok(()));

        let secs: Vec<u64> = waits
            .lock()
            .unwrap()
            .iter()
            .map(Duration::as_secs)
            .collect();
        assert_eq!(secs, vec![1, 1, 2, 4]);
    }

    #[tokio::test]
    /// MB-R-134 (server)/OC-R-048 (CS) — `reconnect: false` ends the loop with the failure,
    /// without ever waiting out a backoff.
    async fn ut_run_with_backoff_stops_when_reconnect_disabled() {
        let attempt = || async {
            AttemptOutcome::Failed::<&'static str> {
                error: "boom",
                reconnect: false,
                reset: false,
            }
        };
        let wait_called = Arc::new(Mutex::new(false));
        let wait_called_clone = wait_called.clone();
        let wait_abortable = move |_backoff: Duration| {
            let wait_called_clone = wait_called_clone.clone();
            async move {
                *wait_called_clone.lock().unwrap() = true;
                false
            }
        };

        let result = run_with_backoff(BackoffPolicy::default(), attempt, wait_abortable).await;
        assert_eq!(result, Err("boom"));
        assert!(!*wait_called.lock().unwrap());
    }

    #[tokio::test]
    /// MB-R-133/OC-R-106 (via the driver's success contract) — a graceful `Done` outcome ends
    /// the loop with `Ok(())` on the very first attempt.
    async fn ut_run_with_backoff_returns_ok_on_done() {
        let attempt = || async { AttemptOutcome::<&'static str>::Done };
        let wait_abortable = |_backoff: Duration| async { false };

        let result = run_with_backoff(BackoffPolicy::default(), attempt, wait_abortable).await;
        assert_eq!(result, Ok(()));
    }

    #[tokio::test]
    /// MB-R-133/MB-R-054/OC-R-106 — an abort signal returned from `wait_abortable` ends the loop
    /// with `Ok(())` right away, after exactly one failed attempt.
    async fn ut_run_with_backoff_aborts_wait_immediately() {
        let calls = Arc::new(Mutex::new(0u32));
        let calls_for_attempt = calls.clone();
        let attempt = move || {
            let calls_for_attempt = calls_for_attempt.clone();
            async move {
                *calls_for_attempt.lock().unwrap() += 1;
                AttemptOutcome::Failed::<&'static str> {
                    error: "boom",
                    reconnect: true,
                    reset: false,
                }
            }
        };
        let wait_abortable = |_backoff: Duration| async { true };

        let result = run_with_backoff(BackoffPolicy::default(), attempt, wait_abortable).await;
        assert_eq!(result, Ok(()));
        assert_eq!(*calls.lock().unwrap(), 1);
    }
}
