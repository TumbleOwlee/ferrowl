//! Sim-thread loop: every-cycle scripts on the cycle interval, event scripts as events arrive.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ferrowl_lua::Context;

use crate::app::Level;
use crate::sim::events::{EventSelector, EventStream, ScriptQueue, SimWake};
use crate::sim::script_id::{ExecutionGuard, ScriptId};

/// Longest the loop sleeps before it re-checks `stop`.
const STOP_POLL: Duration = Duration::from_millis(50);

/// Source of the events one script consumes.
pub trait ScriptFeed: Send {
    /// Pops one event and runs `key` with it; `None` when the queue is empty.
    // `Context<String>::call_with` takes `&String`; lifts when it takes a borrowed key.
    #[allow(clippy::ptr_arg)]
    fn run_next(
        &mut self,
        context: &mut Context<String>,
        key: &String,
    ) -> Option<ferrowl_lua::Result<()>>;
}

/// A queue attached to a stream; detaches when dropped.
pub struct QueueFeed<E> {
    stream: EventStream<E>,
    queue: ScriptQueue<E>,
}

impl<E: Clone + Send + 'static> QueueFeed<E> {
    pub fn attach(stream: EventStream<E>, selector: EventSelector<E>, wake: SimWake) -> Self {
        let queue = ScriptQueue::new(selector, wake);
        stream.attach(&queue);
        Self { stream, queue }
    }
}

impl<E: Clone + Send + mlua::IntoLua + 'static> ScriptFeed for QueueFeed<E> {
    fn run_next(
        &mut self,
        context: &mut Context<String>,
        key: &String,
    ) -> Option<ferrowl_lua::Result<()>> {
        let event = self.queue.pop()?;
        Some(context.call_with(key, event))
    }
}

impl<E> Drop for QueueFeed<E> {
    fn drop(&mut self) {
        self.stream.detach(&self.queue);
    }
}

/// When a script runs.
pub enum ScriptSchedule {
    EveryCycle,
    Events(Box<dyn ScriptFeed>),
}

/// A script and the schedule it runs on.
pub struct PlannedScript {
    pub id: ScriptId,
    pub schedule: ScriptSchedule,
}

/// Where scheduler errors go.
pub type SimLog = Box<dyn Fn(Level, &str) + Send>;

struct EventScript {
    id: ScriptId,
    feed: Box<dyn ScriptFeed>,
    key: String,
}

pub struct SimScheduler {
    every_cycle: HashSet<String>,
    events: Vec<EventScript>,
    cycle: Duration,
    wake: SimWake,
    stop: Arc<AtomicBool>,
    log: SimLog,
}

impl SimScheduler {
    pub fn new(
        scripts: Vec<PlannedScript>,
        cycle: Duration,
        wake: SimWake,
        stop: Arc<AtomicBool>,
        log: SimLog,
    ) -> Self {
        let mut every_cycle = HashSet::new();
        let mut events = Vec::new();
        for script in scripts {
            let key = script.id.name.to_string();
            match script.schedule {
                ScriptSchedule::EveryCycle => {
                    every_cycle.insert(key);
                }
                ScriptSchedule::Events(feed) => events.push(EventScript {
                    id: script.id,
                    feed,
                    key,
                }),
            }
        }
        Self {
            every_cycle,
            events,
            cycle,
            wake,
            stop,
            log,
        }
    }

    pub fn run(self, context: &mut Context<String>) {
        let Self {
            every_cycle,
            mut events,
            cycle,
            wake,
            stop,
            log,
        } = self;
        let mut next_cycle = Instant::now();
        while !stop.load(Ordering::Relaxed) {
            if !every_cycle.is_empty() && Instant::now() >= next_cycle {
                if let Err(errors) = context.call_each(every_cycle.iter()) {
                    for e in errors {
                        log(Level::Error, &format!("[sim] {e}"));
                    }
                }
                next_cycle = Instant::now() + cycle;
            }
            let mut ran = false;
            for script in &mut events {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let _guard = ExecutionGuard::enter(script.id.clone());
                if let Some(result) = script.feed.run_next(context, &script.key) {
                    ran = true;
                    if let Err(e) = result {
                        log(Level::Error, &format!("[sim] {e}"));
                    }
                }
            }
            if ran {
                continue;
            }
            let until_cycle = if every_cycle.is_empty() {
                STOP_POLL
            } else {
                next_cycle.saturating_duration_since(Instant::now())
            };
            wake.wait(STOP_POLL.min(until_cycle));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::script_id::ScriptOwner;
    use ferrowl_lua::ContextBuilder;
    use ferrowl_lua::module::{LogLevel, LogSink};
    use ferrowl_test_support::wait_until_blocking;
    use parking_lot::Mutex;
    use std::thread::JoinHandle;

    type Lines = Arc<Mutex<Vec<String>>>;
    type Logged = Arc<Mutex<Vec<(Level, String)>>>;

    /// Collects `print` output; once `stop_at` lines are in, raises `stop`.
    struct Capture {
        lines: Lines,
        stop_at: Option<(usize, Arc<AtomicBool>)>,
    }

    impl LogSink for Capture {
        fn log(&self, _level: LogLevel, line: &str) {
            let mut lines = self.lines.lock();
            lines.push(line.to_string());
            if let Some((count, stop)) = &self.stop_at
                && lines.len() >= *count
            {
                stop.store(true, Ordering::Relaxed);
            }
        }
    }

    fn id(name: &str) -> ScriptId {
        ScriptId {
            owner: ScriptOwner::Module,
            name: name.into(),
        }
    }

    fn events_script(name: &str, feed: impl ScriptFeed + 'static) -> PlannedScript {
        PlannedScript {
            id: id(name),
            schedule: ScriptSchedule::Events(Box::new(feed)),
        }
    }

    fn every_cycle(name: &str) -> PlannedScript {
        PlannedScript {
            id: id(name),
            schedule: ScriptSchedule::EveryCycle,
        }
    }

    struct Sim {
        stop: Arc<AtomicBool>,
        lines: Lines,
        logged: Logged,
        handle: Option<JoinHandle<()>>,
    }

    impl Sim {
        /// Runs a scheduler over `codes` on its own thread (a `Context` is `!Send`).
        fn spawn(
            codes: &[(&str, &str)],
            planned: Vec<PlannedScript>,
            cycle: Duration,
            wake: SimWake,
            stop_at: Option<usize>,
        ) -> Self {
            let stop = Arc::new(AtomicBool::new(false));
            let lines: Lines = Arc::default();
            let logged: Logged = Arc::default();
            let codes: Vec<(String, String)> = codes
                .iter()
                .map(|(n, c)| (n.to_string(), c.to_string()))
                .collect();
            let (t_stop, t_lines, t_logged) = (stop.clone(), lines.clone(), logged.clone());
            let handle = std::thread::spawn(move || {
                let sink = Capture {
                    lines: t_lines,
                    stop_at: stop_at.map(|n| (n, t_stop.clone())),
                };
                let mut builder = ContextBuilder::<String>::default()
                    .with_stdlib()
                    .with_print_sink(sink)
                    .with_stop_flag(t_stop.clone());
                for (name, code) in &codes {
                    builder = builder.with_script(name.clone(), code);
                }
                let mut context = builder.build().unwrap();
                let log: SimLog = Box::new(move |level, line| {
                    t_logged.lock().push((level, line.to_string()));
                });
                SimScheduler::new(planned, cycle, wake, t_stop, log).run(&mut context);
            });
            Self {
                stop,
                lines,
                logged,
                handle: Some(handle),
            }
        }

        fn wait_lines(&self, what: &str, count: usize) {
            wait_until_blocking(
                what,
                Duration::from_millis(5),
                Duration::from_secs(2),
                || (self.lines.lock().len() >= count).then_some(()),
            );
        }

        fn finish(mut self) -> Vec<String> {
            self.stop.store(true, Ordering::Relaxed);
            self.handle.take().unwrap().join().unwrap();
            self.lines.lock().clone()
        }
    }

    fn feed(stream: &EventStream<i64>, wake: &SimWake) -> QueueFeed<i64> {
        QueueFeed::attach(stream.clone(), Arc::new(|_| true), wake.clone())
    }

    const RECORD: &str = r#"local e = ...; print("ran " .. e)"#;

    #[test]
    /// SC-R-087 — an event script runs as soon as the thread is free, long before the cycle
    /// interval elapses.
    fn ut_event_script_runs_before_cycle_interval() {
        let stream = EventStream::new();
        let wake = SimWake::default();
        let sim = Sim::spawn(
            &[("e", RECORD)],
            vec![events_script("e", feed(&stream, &wake))],
            Duration::from_secs(3600),
            wake,
            None,
        );
        stream.publish_with(|_| 7);
        sim.wait_lines("event script runs", 1);
        assert_eq!(sim.finish(), vec!["ran 7".to_string()]);
    }

    #[test]
    /// SC-R-088 — an event script runs once per delivered event, in publish order.
    fn ut_event_script_runs_once_per_event_in_order() {
        let stream = EventStream::new();
        let wake = SimWake::default();
        let sim = Sim::spawn(
            &[("e", RECORD)],
            vec![events_script("e", feed(&stream, &wake))],
            Duration::from_secs(3600),
            wake,
            None,
        );
        for n in 1..=5 {
            stream.publish_with(|_| n);
        }
        sim.wait_lines("five runs", 5);
        let lines = sim.finish();
        let expected: Vec<String> = (1..=5).map(|n| format!("ran {n}")).collect();
        assert_eq!(lines, expected);
    }

    #[test]
    /// SC-R-089 — runs never overlap: each records enter then leave, and the sequence strictly
    /// alternates across two event scripts.
    fn ut_runs_are_sequential() {
        let stream = EventStream::new();
        let wake = SimWake::default();
        let code = |n: &str| {
            format!(
                r#"local e = ...; print("enter {n}"); local x = 0; for i = 1, 20000 do x = x + i end; print("leave {n}")"#
            )
        };
        let (a, b) = (code("a"), code("b"));
        let sim = Sim::spawn(
            &[("a", &a), ("b", &b)],
            vec![
                events_script("a", feed(&stream, &wake)),
                events_script("b", feed(&stream, &wake)),
            ],
            Duration::from_secs(3600),
            wake,
            None,
        );
        for n in 0..10 {
            stream.publish_with(|_| n);
        }
        sim.wait_lines("all runs", 40);
        let lines = sim.finish();
        for pair in lines.chunks(2) {
            assert!(pair[0].starts_with("enter "), "{lines:?}");
            assert_eq!(pair[1], pair[0].replace("enter", "leave"), "{lines:?}");
        }
    }

    #[test]
    /// SC-R-013 — a queued event ends the sleep: twenty events, each run waited for, finish
    /// well inside the time twenty 50 ms stop polls would take.
    fn ut_queued_event_ends_the_sleep() {
        let stream = EventStream::new();
        let wake = SimWake::default();
        let sim = Sim::spawn(
            &[("e", RECORD)],
            vec![events_script("e", feed(&stream, &wake))],
            Duration::from_secs(3600),
            wake,
            None,
        );
        let start = Instant::now();
        for n in 1..=20 {
            stream.publish_with(|_| n);
            sim.wait_lines("run after sleep", n as usize);
        }
        assert!(start.elapsed() < Duration::from_millis(600));
        sim.finish();
    }

    #[test]
    /// SC-R-013 — stop is observed during a long sleep.
    fn ut_stop_observed_during_sleep() {
        let sim = Sim::spawn(
            &[("c", "print('tick')")],
            vec![every_cycle("c")],
            Duration::from_secs(3600),
            SimWake::default(),
            None,
        );
        sim.wait_lines("first cycle", 1);
        let start = Instant::now();
        sim.finish();
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    /// SC-R-012 — stop is observed between event runs while a backlog drains, and between two
    /// scripts of one pass.
    fn ut_stop_observed_between_event_runs() {
        let stream = EventStream::new();
        let wake = SimWake::default();
        let (feed_a, feed_b) = (feed(&stream, &wake), feed(&stream, &wake));
        for n in 0..1000 {
            stream.publish_with(|_| n);
        }
        let sim = Sim::spawn(
            &[("a", "print('a')"), ("b", "print('b')")],
            vec![events_script("a", feed_a), events_script("b", feed_b)],
            Duration::from_secs(3600),
            wake,
            Some(3),
        );
        sim.handle.unwrap().join().unwrap();
        assert_eq!(*sim.lines.lock(), ["a", "b", "a"]);
    }

    #[test]
    /// Every-cycle scripts run once per cycle interval.
    fn ut_every_cycle_scripts_run_each_cycle() {
        let start = Instant::now();
        let sim = Sim::spawn(
            &[("c", "print('tick')")],
            vec![every_cycle("c")],
            Duration::from_millis(20),
            SimWake::default(),
            Some(3),
        );
        sim.handle.unwrap().join().unwrap();
        assert_eq!(sim.lines.lock().len(), 3);
        assert!(start.elapsed() >= Duration::from_millis(40));
    }

    #[test]
    /// SC-R-032 — a script error is logged at error level with the `[sim]` prefix, from both the
    /// cycle path and the event path.
    fn ut_script_error_logged_with_sim_prefix() {
        let stream = EventStream::new();
        let wake = SimWake::default();
        let sim = Sim::spawn(
            &[("c", "error('cycle boom')"), ("e", "error('event boom')")],
            vec![every_cycle("c"), events_script("e", feed(&stream, &wake))],
            Duration::from_secs(3600),
            wake,
            None,
        );
        stream.publish_with(|_| 1);
        wait_until_blocking(
            "both errors logged",
            Duration::from_millis(5),
            Duration::from_secs(2),
            || (sim.logged.lock().len() >= 2).then_some(()),
        );
        let logged = sim.logged.lock().clone();
        sim.finish();
        for (level, line) in &logged {
            assert_eq!(*level, Level::Error);
            assert!(line.starts_with("[sim] "), "{line}");
        }
        assert!(logged.iter().any(|(_, l)| l.contains("cycle boom")));
        assert!(logged.iter().any(|(_, l)| l.contains("event boom")));
    }

    #[test]
    /// Dropping a feed detaches its queue from the stream.
    fn ut_feed_detaches_on_drop() {
        let stream = EventStream::<i64>::new();
        let wake = SimWake::default();
        let queue_feed = feed(&stream, &wake);
        drop(queue_feed);
        let built = AtomicBool::new(false);
        stream.publish_with(|n| {
            built.store(true, Ordering::Relaxed);
            n as i64
        });
        assert!(!built.load(Ordering::Relaxed));
    }
}
