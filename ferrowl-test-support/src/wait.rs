use std::time::Duration;

/// Evaluates `ready` immediately, retries every `poll`, and panics naming
/// `what` once `deadline` has elapsed. The deadline is wall-clock so it must
/// leave room for coverage-instrumented builds on a loaded machine.
pub async fn wait_until<T>(
    what: &str,
    poll: Duration,
    deadline: Duration,
    mut ready: impl FnMut() -> Option<T>,
) -> T {
    wait_until_async(what, poll, deadline, async || ready()).await
}

/// Like [`wait_until`], for a condition that must `.await`.
pub async fn wait_until_async<T>(
    what: &str,
    poll: Duration,
    deadline: Duration,
    mut ready: impl AsyncFnMut() -> Option<T>,
) -> T {
    let end = tokio::time::Instant::now() + deadline;
    loop {
        if let Some(t) = ready().await {
            return t;
        }
        assert!(
            tokio::time::Instant::now() < end,
            "{what}: not reached within {deadline:?}"
        );
        tokio::time::sleep(poll).await;
    }
}

/// Like [`wait_until`], for synchronous tests; sleeps the calling thread.
pub fn wait_until_blocking<T>(
    what: &str,
    poll: Duration,
    deadline: Duration,
    mut ready: impl FnMut() -> Option<T>,
) -> T {
    let end = std::time::Instant::now() + deadline;
    loop {
        if let Some(t) = ready() {
            return t;
        }
        assert!(
            std::time::Instant::now() < end,
            "{what}: not reached within {deadline:?}"
        );
        std::thread::sleep(poll);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: fn(u64) -> Duration = Duration::from_millis;

    #[tokio::test(start_paused = true)]
    async fn ut_wait_until_returns_once_ready() {
        let start = tokio::time::Instant::now();
        let v = wait_until("x", MS(20), Duration::from_secs(10), || {
            (start.elapsed() >= MS(9_500)).then_some(42u8)
        })
        .await;
        assert_eq!(v, 42);
        assert!(start.elapsed() >= MS(9_500));
    }

    #[tokio::test(start_paused = true)]
    async fn ut_wait_until_honours_poll_interval() {
        let start = tokio::time::Instant::now();
        let mut calls = 0;
        wait_until("x", MS(100), Duration::from_secs(10), || {
            calls += 1;
            (calls == 4).then_some(())
        })
        .await;
        assert_eq!(calls, 4);
        assert_eq!(start.elapsed(), MS(300));
    }

    #[tokio::test(start_paused = true)]
    #[should_panic(expected = "bind: not reached within 2s")]
    async fn ut_wait_until_panics_after_deadline() {
        wait_until("bind", MS(20), Duration::from_secs(2), || None::<u8>).await;
    }

    #[tokio::test(start_paused = true)]
    async fn ut_wait_until_async_drives_an_awaiting_condition() {
        let start = tokio::time::Instant::now();
        let mut n = 0;
        let v = wait_until_async("x", MS(5), Duration::from_secs(10), async || {
            tokio::task::yield_now().await;
            n += 1;
            (n == 3).then_some(n)
        })
        .await;
        assert_eq!(v, 3);
        assert_eq!(start.elapsed(), MS(10));
    }

    #[tokio::test(start_paused = true)]
    #[should_panic(expected = "settle: not reached within 3s")]
    async fn ut_wait_until_async_panics_after_deadline() {
        wait_until_async("settle", MS(5), Duration::from_secs(3), async || None::<u8>).await;
    }

    #[test]
    fn ut_wait_until_blocking_returns_once_ready() {
        let start = std::time::Instant::now();
        let v = wait_until_blocking("x", MS(50), Duration::from_secs(5), || {
            (start.elapsed() >= MS(2_500)).then_some(7u8)
        });
        assert_eq!(v, 7);
    }

    #[test]
    #[should_panic(expected = "flag: not reached within 50ms")]
    fn ut_wait_until_blocking_panics_after_deadline() {
        wait_until_blocking("flag", MS(1), MS(50), || None::<u8>);
    }
}
