//! Bounded per-script event queues fed by a shared stream, and the wake flag that ends the sim
//! thread's sleep.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Condvar, Mutex};

/// Decides whether an event belongs in a queue.
pub type EventSelector<E> = Arc<dyn Fn(&E) -> bool + Send + Sync>;

/// Events a queue holds before the oldest is dropped.
const QUEUE_CAPACITY: usize = 4096;

struct StreamInner<E> {
    next_seq: u64,
    queues: Vec<Arc<QueueShared<E>>>,
}

struct QueueShared<E> {
    selector: EventSelector<E>,
    wake: SimWake,
    events: Mutex<VecDeque<E>>,
}

/// Fan-out of events to the attached queues. Lock order: stream, queue, wake.
pub struct EventStream<E> {
    inner: Arc<Mutex<StreamInner<E>>>,
}

impl<E> Clone for EventStream<E> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<E: Clone + Send + 'static> EventStream<E> {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(StreamInner {
                next_seq: 1,
                queues: Vec::new(),
            })),
        }
    }

    /// Takes the next sequence number; `build` runs only if a queue is attached.
    pub fn publish_with(&self, build: impl FnOnce(u64) -> E) {
        let mut inner = self.inner.lock();
        let seq = inner.next_seq;
        inner.next_seq += 1;
        if inner.queues.is_empty() {
            return;
        }
        let event = build(seq);
        for queue in &inner.queues {
            queue.offer(event.clone());
        }
    }

    pub fn attach(&self, queue: &ScriptQueue<E>) {
        self.inner.lock().queues.push(queue.shared.clone());
    }
}

// Unbounded so `impl<E> Drop for QueueFeed<E>` can call it: a Drop impl cannot add bounds.
impl<E> EventStream<E> {
    pub fn detach(&self, queue: &ScriptQueue<E>) {
        self.inner
            .lock()
            .queues
            .retain(|q| !Arc::ptr_eq(q, &queue.shared));
    }
}

impl<E> QueueShared<E> {
    /// Queues `event` if selected, dropping the oldest at capacity, and wakes the sim thread.
    fn offer(&self, event: E) {
        if !(self.selector)(&event) {
            return;
        }
        {
            let mut events = self.events.lock();
            if events.len() >= QUEUE_CAPACITY {
                events.pop_front();
            }
            events.push_back(event);
        }
        self.wake.notify();
    }
}

/// The events selected for one script, oldest first.
pub struct ScriptQueue<E> {
    shared: Arc<QueueShared<E>>,
}

impl<E: Clone + Send + 'static> ScriptQueue<E> {
    pub fn new(selector: EventSelector<E>, wake: SimWake) -> Self {
        Self {
            shared: Arc::new(QueueShared {
                selector,
                wake,
                events: Mutex::new(VecDeque::new()),
            }),
        }
    }

    pub fn pop(&self) -> Option<E> {
        self.shared.events.lock().pop_front()
    }
}

/// Wakes a sleeping sim thread when an event is queued.
#[derive(Clone, Default)]
pub struct SimWake {
    inner: Arc<(Mutex<bool>, Condvar)>,
}

impl SimWake {
    pub fn notify(&self) {
        let (flag, condvar) = &*self.inner;
        *flag.lock() = true;
        condvar.notify_one();
    }

    /// Waits until notified or `timeout` passes, then clears the flag.
    pub fn wait(&self, timeout: Duration) {
        let (flag, condvar) = &*self.inner;
        let mut notified = flag.lock();
        if !*notified {
            condvar.wait_for(&mut notified, timeout);
        }
        *notified = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    fn accept_all() -> EventSelector<u64> {
        Arc::new(|_| true)
    }

    fn attached(selector: EventSelector<u64>) -> (EventStream<u64>, ScriptQueue<u64>) {
        let stream = EventStream::new();
        let queue = ScriptQueue::new(selector, SimWake::default());
        stream.attach(&queue);
        (stream, queue)
    }

    #[test]
    /// SC-R-095 — a queue holds 4096 events before dropping any.
    fn ut_queue_holds_4096_events() {
        let (stream, queue) = attached(accept_all());
        for _ in 0..4096 {
            stream.publish_with(|seq| seq);
        }
        let mut seen = Vec::new();
        while let Some(e) = queue.pop() {
            seen.push(e);
        }
        assert_eq!(seen.len(), 4096);
        assert_eq!(seen.first(), Some(&1));
        assert_eq!(seen.last(), Some(&4096));
    }

    #[test]
    /// SC-R-096 — a full queue drops its oldest event for the new one, and publishing never
    /// blocks on a queue nobody drains.
    fn ut_full_queue_drops_oldest_and_never_blocks() {
        let (stream, queue) = attached(accept_all());
        let start = Instant::now();
        for _ in 0..4097 {
            stream.publish_with(|seq| seq);
        }
        assert!(start.elapsed() < Duration::from_secs(2));
        assert_eq!(queue.pop(), Some(2));
        let mut last = 0;
        while let Some(e) = queue.pop() {
            last = e;
        }
        assert_eq!(last, 4097);
    }

    #[test]
    /// With no queue attached an event takes its sequence number and builds nothing.
    fn ut_publish_takes_seq_with_no_queue_and_builds_nothing() {
        let stream = EventStream::<u64>::new();
        let built = AtomicBool::new(false);
        stream.publish_with(|seq| {
            built.store(true, Ordering::Relaxed);
            seq
        });
        assert!(!built.load(Ordering::Relaxed));
        let queue = ScriptQueue::new(accept_all(), SimWake::default());
        stream.attach(&queue);
        stream.publish_with(|seq| seq);
        assert_eq!(queue.pop(), Some(2));
    }

    #[test]
    /// An event the selector rejects is not queued.
    fn ut_selector_rejects_without_queueing() {
        let (stream, queue) = attached(Arc::new(|e: &u64| e % 2 == 0));
        for _ in 0..4 {
            stream.publish_with(|seq| seq);
        }
        assert_eq!(queue.pop(), Some(2));
        assert_eq!(queue.pop(), Some(4));
        assert_eq!(queue.pop(), None);
    }

    #[test]
    /// A detached queue receives nothing.
    fn ut_detached_queue_receives_nothing() {
        let (stream, queue) = attached(accept_all());
        stream.publish_with(|seq| seq);
        stream.detach(&queue);
        stream.publish_with(|seq| seq);
        assert_eq!(queue.pop(), Some(1));
        assert_eq!(queue.pop(), None);
    }

    #[test]
    /// SC-R-013 — a notify ends a wait before its timeout.
    fn ut_wake_ends_wait_early() {
        let wake = SimWake::default();
        let other = wake.clone();
        let start = Instant::now();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            other.notify();
        });
        wake.wait(Duration::from_secs(30));
        assert!(start.elapsed() < Duration::from_secs(5));
        handle.join().unwrap();
    }
}
