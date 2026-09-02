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

/// Waits out a fixed `backoff` window while draining `receiver`, for a retry loop that must keep
/// accepting and logging commands during the wait rather than leaving them queued on the channel.
///
/// Returns `true` if the wait ended early because a terminate-matching command (per
/// `is_terminate`) arrived or the channel closed (`None`) — the caller should stop retrying.
/// Returns `false` if the full `backoff` elapsed with no such command — the caller should
/// attempt again.
///
/// The deadline is computed once, before the loop, and awaited with `sleep_until` rather than
/// re-running `sleep(backoff)` on every iteration: the loop re-enters after every dropped
/// non-terminate command, and restarting the sleep on each one would let a chatty caller extend
/// the backoff indefinitely instead of bounding it at `backoff`.
///
/// Any command that does not match `is_terminate` is logged via `log` with `dropped_msg` and the
/// wait continues.
pub async fn wait_backoff<T, LFut>(
    receiver: &mut tokio::sync::mpsc::Receiver<T>,
    backoff: Duration,
    dropped_msg: &str,
    is_terminate: impl Fn(&T) -> bool,
    log: impl Fn(String) -> LFut,
) -> bool
where
    LFut: Future<Output = ()>,
{
    let deadline = tokio::time::Instant::now() + backoff;
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => return false,
            cmd = receiver.recv() => match cmd {
                None => return true,
                Some(cmd) if is_terminate(&cmd) => return true,
                Some(_) => {
                    log(dropped_msg.to_string()).await;
                }
            },
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
    /// MB-R-051, MB-R-132 — a `reset: true` outcome drops the wait that follows *that* failure
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
    /// MB-R-134 (server)/OC-R-135 (CS) — `reconnect: false` ends the loop with the failure,
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
    /// MB-R-133, MB-R-054, OC-R-106 — an abort signal returned from `wait_abortable` ends the loop
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

    #[derive(Debug, PartialEq, Eq)]
    enum TestCmd {
        Terminate,
        Other,
    }

    /// The deadline elapsing with no command arriving returns `false`.
    #[tokio::test(start_paused = true)]
    async fn ut_wait_backoff_returns_false_when_deadline_elapses() {
        let (_tx, mut rx) = tokio::sync::mpsc::channel::<TestCmd>(1);
        let result = wait_backoff(
            &mut rx,
            Duration::from_secs(5),
            "dropped",
            |c: &TestCmd| matches!(c, TestCmd::Terminate),
            |_msg| async {},
        );
        tokio::pin!(result);
        tokio::time::advance(Duration::from_secs(5)).await;
        assert!(!result.await);
    }

    /// A terminate-matching command ends the wait early with `true`.
    #[tokio::test(start_paused = true)]
    async fn ut_wait_backoff_returns_true_on_terminate_command() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<TestCmd>(1);
        tx.send(TestCmd::Terminate).await.unwrap();
        let result = wait_backoff(
            &mut rx,
            Duration::from_secs(5),
            "dropped",
            |c: &TestCmd| matches!(c, TestCmd::Terminate),
            |_msg| async {},
        )
        .await;
        assert!(result);
    }

    /// A closed channel (sender dropped, no command) ends the wait early with `true`.
    #[tokio::test(start_paused = true)]
    async fn ut_wait_backoff_returns_true_on_closed_channel() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<TestCmd>(1);
        drop(tx);
        let result = wait_backoff(
            &mut rx,
            Duration::from_secs(5),
            "dropped",
            |c: &TestCmd| matches!(c, TestCmd::Terminate),
            |_msg| async {},
        )
        .await;
        assert!(result);
    }

    /// A non-terminate command logs `dropped_msg` and keeps waiting rather than returning.
    #[tokio::test(start_paused = true)]
    async fn ut_wait_backoff_logs_and_continues_on_non_terminate_command() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<TestCmd>(2);
        let logged: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let logged_for_closure = logged.clone();
        tx.send(TestCmd::Other).await.unwrap();
        let result = wait_backoff(
            &mut rx,
            Duration::from_secs(5),
            "Command dropped: test.",
            |c: &TestCmd| matches!(c, TestCmd::Terminate),
            move |msg| {
                let logged_for_closure = logged_for_closure.clone();
                async move { logged_for_closure.lock().unwrap().push(msg) }
            },
        );
        tokio::pin!(result);
        tokio::time::advance(Duration::from_secs(5)).await;
        assert!(!result.await);
        assert_eq!(
            *logged.lock().unwrap(),
            vec!["Command dropped: test.".to_string()]
        );
    }

    /// Repeated non-terminate commands do not extend the deadline past the original backoff:
    /// the `sleep_until` deadline is computed once, so draining commands throughout the window
    /// still returns `false` at exactly the original 5s mark, not later.
    #[tokio::test(start_paused = true)]
    async fn ut_wait_backoff_deadline_not_extended_by_repeated_commands() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<TestCmd>(4);
        let result = wait_backoff(
            &mut rx,
            Duration::from_secs(5),
            "dropped",
            |c: &TestCmd| matches!(c, TestCmd::Terminate),
            |_msg| async {},
        );
        tokio::pin!(result);

        // The future must be polled before any clock movement: `wait_backoff` establishes its
        // deadline on first poll, so leaving it unpolled here would silently start the clock at
        // whatever time the first `poll!` happens to land on and make every assertion below
        // vacuous.
        let start = tokio::time::Instant::now();
        assert!(futures_util::poll!(&mut result).is_pending());

        // A chatty command every second for 4 seconds, each one polled through so it is actually
        // consumed by the loop rather than left queued on the channel.
        for _ in 0..4 {
            tokio::time::advance(Duration::from_secs(1)).await;
            tx.try_send(TestCmd::Other).unwrap();
            assert!(futures_util::poll!(&mut result).is_pending());
        }

        tokio::time::advance(Duration::from_millis(900)).await;
        assert!(
            futures_util::poll!(&mut result).is_pending(),
            "must still be pending just before the original 5s deadline"
        );

        tokio::time::advance(Duration::from_millis(100)).await;
        assert!(!result.await);

        // Resolution timed against the original deadline, not merely "eventually": an
        // implementation that restarted its sleep per dropped command would land at 9s.
        assert_eq!(
            start.elapsed(),
            Duration::from_secs(5),
            "four dropped commands must not push the deadline out"
        );
    }
}
