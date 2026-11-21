//! Sim-thread loop: every-cycle scripts on the cycle interval, event scripts as events arrive.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ferrowl_lua::Context;

use crate::app::Level;
use crate::sim::events::{EventSelector, EventStream, ScriptQueue, SimWake};
use crate::sim::script_id::{ExecutionGuard, ScriptId};

/// Longest the loop sleeps before it re-checks `stop`.
const STOP_POLL: Duration = Duration::from_millis(50);

/// How long the loop may sleep at `now`: up to the stop poll, ended early by the next cycle
/// (when it has scripts to run) and by the earliest interval due time.
fn sleep_for(
    now: Instant,
    next_cycle: Option<Instant>,
    interval_dues: impl Iterator<Item = Instant>,
) -> Duration {
    next_cycle
        .into_iter()
        .chain(interval_dues)
        .map(|due| due.saturating_duration_since(now))
        .fold(STOP_POLL, Duration::min)
}

/// Source of the events one script consumes.
pub trait ScriptFeed: Send {
    /// Pops one event, calls `on_take` when it carries drops, then runs `key` with it; `None`
    /// when the queue is empty.
    // `Context<String>::call_with` takes `&String`; lifts when it takes a borrowed key.
    #[allow(clippy::ptr_arg)]
    fn run_next(
        &mut self,
        context: &mut Context<String>,
        key: &String,
        on_take: &mut dyn FnMut(DropReport),
    ) -> Option<ferrowl_lua::Result<()>>;

    /// True once per burst: the first drop that no line has reported yet.
    fn take_first_drop(&self) -> bool;
}

/// Drops a taken event carries that still need a log line.
pub struct DropReport {
    /// The burst's first drop was not yet reported.
    pub first_unlogged: bool,
    /// Drops beyond the first since the previous delivery.
    pub further: u64,
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
        on_take: &mut dyn FnMut(DropReport),
    ) -> Option<ferrowl_lua::Result<()>> {
        let delivered = self.queue.pop()?;
        if delivered.dropped > 0 {
            on_take(DropReport {
                first_unlogged: delivered.first_unlogged,
                further: delivered.dropped - 1,
            });
        }
        Some(context.call_with(key, delivered))
    }

    fn take_first_drop(&self) -> bool {
        self.queue.take_first_drop()
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
    Interval { period: Duration },
    Events(Box<dyn ScriptFeed>),
}

impl ScriptSchedule {
    /// An interval schedule for a period in seconds: SC-R-016's fallback, then `floor_secs`
    /// (SC-R-045 for a module, 0 for the session).
    pub fn interval(seconds: f64, floor_secs: f64) -> Self {
        Self::Interval {
            period: crate::config::sanitize_interval_secs(seconds, 1.0, floor_secs),
        }
    }
}

/// How the scripts with no trigger share a cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CyclePolicy {
    /// Every such script runs on every cycle (SC-R-014).
    EveryCycle,
    /// Each such script runs only when it last ran at least one cycle interval ago (SC-R-044).
    AtMostOncePerInterval,
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

/// A script on a fixed period. `due` is `None` once the period runs past the clock's range.
struct IntervalScript {
    key: String,
    period: Duration,
    due: Option<Instant>,
}

pub struct SimScheduler {
    every_cycle: Vec<String>,
    intervals: Vec<IntervalScript>,
    events: Vec<EventScript>,
    cycle: Duration,
    policy: CyclePolicy,
    wake: SimWake,
    stop: Arc<AtomicBool>,
    log: SimLog,
}

impl SimScheduler {
    pub fn new(
        scripts: Vec<PlannedScript>,
        cycle: Duration,
        policy: CyclePolicy,
        wake: SimWake,
        stop: Arc<AtomicBool>,
        log: SimLog,
    ) -> Self {
        let mut every_cycle = Vec::new();
        let mut intervals = Vec::new();
        let mut events = Vec::new();
        for script in scripts {
            let key = script.id.name.to_string();
            match script.schedule {
                ScriptSchedule::EveryCycle => {
                    every_cycle.push(key);
                }
                ScriptSchedule::Interval { period } => intervals.push(IntervalScript {
                    key,
                    period,
                    due: Some(Instant::now()),
                }),
                ScriptSchedule::Events(feed) => events.push(EventScript {
                    id: script.id,
                    feed,
                    key,
                }),
            }
        }
        Self {
            every_cycle,
            intervals,
            events,
            cycle,
            policy,
            wake,
            stop,
            log,
        }
    }

    pub fn run(self, context: &mut Context<String>) {
        let Self {
            every_cycle,
            mut intervals,
            mut events,
            cycle,
            policy,
            wake,
            stop,
            log,
        } = self;
        let report_first_drops = |events: &[EventScript]| {
            for script in events {
                if script.feed.take_first_drop() {
                    log(Level::Error, &first_drop_line(&script.id));
                }
            }
        };
        let mut next_cycle = Some(Instant::now());
        while !stop.load(Ordering::Relaxed) {
            if !every_cycle.is_empty() && next_cycle.is_some_and(|t| Instant::now() >= t) {
                for key in &every_cycle {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    report_first_drops(&events);
                    let result = match policy {
                        CyclePolicy::EveryCycle => context.call(key),
                        CyclePolicy::AtMostOncePerInterval => context.refresh(key, cycle),
                    };
                    if let Err(e) = result {
                        log(Level::Error, &format!("[sim] {e}"));
                    }
                }
                next_cycle = Instant::now().checked_add(cycle);
            }
            for script in &mut intervals {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                if script.due.is_none_or(|due| Instant::now() < due) {
                    continue;
                }
                report_first_drops(&events);
                let start = Instant::now();
                if let Err(e) = context.call(&script.key) {
                    log(Level::Error, &format!("[sim] {e}"));
                }
                script.due = start.checked_add(script.period);
            }
            let mut ran = false;
            for index in 0..events.len() {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                report_first_drops(&events);
                let script = &mut events[index];
                let _guard = ExecutionGuard::enter(script.id.clone());
                let id = &script.id;
                let mut on_take = |report: DropReport| {
                    if report.first_unlogged {
                        log(Level::Error, &first_drop_line(id));
                    }
                    if report.further > 0 {
                        log(
                            Level::Error,
                            &format!(
                                "[sim] script '{}' dropped {} further event{}: queue full",
                                id.name,
                                report.further,
                                if report.further == 1 { "" } else { "s" }
                            ),
                        );
                    }
                };
                if let Some(result) = script.feed.run_next(context, &script.key, &mut on_take) {
                    ran = true;
                    if let Err(e) = result {
                        log(Level::Error, &format!("[sim] {e}"));
                    }
                }
            }
            if ran {
                continue;
            }
            let cycle_due = next_cycle.filter(|_| !every_cycle.is_empty());
            let dues = intervals.iter().filter_map(|script| script.due);
            wake.wait(sleep_for(Instant::now(), cycle_due, dues));
        }
    }
}

fn first_drop_line(id: &ScriptId) -> String {
    format!("[sim] script '{}' dropped 1 event: queue full", id.name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::script_id::ScriptOwner;
    use ferrowl_lua::ContextBuilder;
    use ferrowl_lua::module::{LogLevel, LogSink, TimeModule};
    use ferrowl_test_support::wait_until_blocking;
    use parking_lot::Mutex;
    use std::thread::JoinHandle;

    type Lines = Arc<Mutex<Vec<String>>>;
    type Logged = Arc<Mutex<Vec<(Level, String)>>>;

    /// Collects `print` output; once `stop_at` lines are in, raises `stop`.
    struct Capture {
        lines: Lines,
        timeline: Lines,
        stop_at: Option<(usize, Arc<AtomicBool>)>,
    }

    impl LogSink for Capture {
        fn log(&self, _level: LogLevel, line: &str) {
            self.timeline.lock().push(line.to_string());
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
        /// Printed and scheduler-logged lines in the order they happened.
        timeline: Lines,
        handle: Option<JoinHandle<()>>,
    }

    impl Sim {
        /// Runs a scheduler over `codes` on its own thread (a `Context` is `!Send`).
        fn spawn(
            codes: &[(&str, &str)],
            planned: Vec<PlannedScript>,
            cycle: Duration,
            policy: CyclePolicy,
            wake: SimWake,
            stop_at: Option<usize>,
        ) -> Self {
            let stop = Arc::new(AtomicBool::new(false));
            let lines: Lines = Arc::default();
            let logged: Logged = Arc::default();
            let timeline: Lines = Arc::default();
            let (t_timeline, l_timeline) = (timeline.clone(), timeline.clone());
            let codes: Vec<(String, String)> = codes
                .iter()
                .map(|(n, c)| (n.to_string(), c.to_string()))
                .collect();
            let (t_stop, t_lines, t_logged) = (stop.clone(), lines.clone(), logged.clone());
            let handle = std::thread::spawn(move || {
                let sink = Capture {
                    lines: t_lines,
                    timeline: t_timeline,
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
                    l_timeline.lock().push(line.to_string());
                    t_logged.lock().push((level, line.to_string()));
                });
                SimScheduler::new(planned, cycle, policy, wake, t_stop, log).run(&mut context);
            });
            Self {
                stop,
                lines,
                logged,
                timeline,
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
            CyclePolicy::EveryCycle,
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
            CyclePolicy::EveryCycle,
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
            CyclePolicy::EveryCycle,
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
            CyclePolicy::EveryCycle,
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
            CyclePolicy::EveryCycle,
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
            CyclePolicy::EveryCycle,
            wake,
            Some(3),
        );
        sim.handle.unwrap().join().unwrap();
        assert_eq!(*sim.lines.lock(), ["a", "b", "a"]);
    }

    #[test]
    /// SC-R-013, SC-R-014 — a cycle interval past the clock's range runs the every-cycle scripts
    /// once and then waits for stop without panicking.
    fn ut_cycle_past_clock_range_runs_once_and_stops() {
        let sim = Sim::spawn(
            &[("c", "print('tick')")],
            vec![every_cycle("c")],
            Duration::MAX,
            CyclePolicy::EveryCycle,
            SimWake::default(),
            None,
        );
        sim.wait_lines("first cycle", 1);
        let start = Instant::now();
        let lines = sim.finish();
        assert!(start.elapsed() < Duration::from_secs(2));
        assert_eq!(lines, vec!["tick".to_string()]);
    }

    #[test]
    /// Every-cycle scripts run once per cycle interval.
    fn ut_every_cycle_scripts_run_each_cycle() {
        let start = Instant::now();
        let sim = Sim::spawn(
            &[("c", "print('tick')")],
            vec![every_cycle("c")],
            Duration::from_millis(20),
            CyclePolicy::EveryCycle,
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
            CyclePolicy::EveryCycle,
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
    /// SC-R-044 — under `AtMostOncePerInterval` a script not yet an interval old is skipped,
    /// so it runs about once per interval and never twice within one.
    fn ut_at_most_once_policy_skips_a_script_that_ran_within_the_interval() {
        let stamps: Arc<Mutex<Vec<Instant>>> = Arc::default();
        let started = Instant::now();
        let (stop, t_stamps) = (Arc::new(AtomicBool::new(false)), stamps.clone());
        let t_stop = stop.clone();
        let handle = std::thread::spawn(move || {
            let sink = StampSink(t_stamps);
            let mut context = ContextBuilder::<String>::default()
                .with_stdlib()
                .with_print_sink(sink)
                .with_stop_flag(t_stop.clone())
                .with_script("c".to_string(), "print('tick')")
                .build()
                .unwrap();
            let log: SimLog = Box::new(|_, _| {});
            SimScheduler::new(
                vec![every_cycle("c")],
                Duration::from_millis(100),
                CyclePolicy::AtMostOncePerInterval,
                SimWake::default(),
                t_stop,
                log,
            )
            .run(&mut context);
        });
        std::thread::sleep(Duration::from_millis(550));
        stop.store(true, Ordering::Relaxed);
        handle.join().unwrap();
        let stamps = stamps.lock().clone();
        assert!(
            stamps[0].duration_since(started) >= Duration::from_millis(95),
            "a script that never ran ran before one interval elapsed"
        );
        assert!((4..=6).contains(&stamps.len()), "{} runs", stamps.len());
        for pair in stamps.windows(2) {
            assert!(
                pair[1].duration_since(pair[0]) >= Duration::from_millis(95),
                "two runs within one interval"
            );
        }
    }

    /// Records the instant of every `print`.
    struct StampSink(Arc<Mutex<Vec<Instant>>>);

    impl LogSink for StampSink {
        fn log(&self, _level: LogLevel, _line: &str) {
            self.0.lock().push(Instant::now());
        }
    }

    #[test]
    /// SC-R-047, SC-R-032 — a script running past the cap is cut off with an Error-level `[sim]` line soon
    /// after the cap, and the other scripts and queued events still run.
    fn ut_slow_script_is_cut_off_and_the_others_still_run() {
        let stream = EventStream::new();
        let wake = SimWake::default();
        let sim = Sim::spawn(
            &[
                ("runaway", "while true do end"),
                ("ok", "print('ok ran')"),
                ("e", RECORD),
            ],
            vec![
                every_cycle("runaway"),
                every_cycle("ok"),
                events_script("e", feed(&stream, &wake)),
            ],
            Duration::from_secs(3600),
            CyclePolicy::EveryCycle,
            wake,
            None,
        );
        stream.publish_with(|_| 3);
        wait_until_blocking(
            "the cap's Error line",
            Duration::from_millis(5),
            Duration::from_secs(1),
            || {
                sim.logged
                    .lock()
                    .iter()
                    .any(|(level, line)| *level == Level::Error && line.starts_with("[sim] "))
                    .then_some(())
            },
        );
        sim.wait_lines("ok and event scripts ran", 2);
        let timeline = sim.timeline.clone();
        sim.finish();
        let timeline = timeline.lock().clone();
        let cap = timeline
            .iter()
            .position(|l| l.starts_with("[sim] "))
            .expect("the cap line");
        for printed in ["ok ran", "ran 3"] {
            let at = timeline.iter().position(|l| l == printed);
            assert!(at.is_some_and(|at| at > cap), "{printed}: {timeline:?}");
        }
    }

    fn interval(name: &str, period: Duration) -> PlannedScript {
        PlannedScript {
            id: id(name),
            schedule: ScriptSchedule::Interval { period },
        }
    }

    /// Records the instant and text of every `print`.
    struct TaggedSink(Arc<Mutex<Vec<(Instant, String)>>>);

    impl LogSink for TaggedSink {
        fn log(&self, _level: LogLevel, line: &str) {
            self.0.lock().push((Instant::now(), line.to_string()));
        }
    }

    /// Runs a scheduler over `codes` for `run_for`, returning every `print` with its instant.
    fn run_stamped(
        codes: &[(&str, &str)],
        planned: Vec<PlannedScript>,
        cycle: Duration,
        run_for: Duration,
    ) -> Vec<(Instant, String)> {
        let prints: Arc<Mutex<Vec<(Instant, String)>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let codes: Vec<(String, String)> = codes
            .iter()
            .map(|(n, c)| (n.to_string(), c.to_string()))
            .collect();
        let (t_stop, t_prints) = (stop.clone(), prints.clone());
        let handle = std::thread::spawn(move || {
            let mut builder = ContextBuilder::<String>::default()
                .with_stdlib()
                .with_module(TimeModule::default())
                .with_print_sink(TaggedSink(t_prints))
                .with_stop_flag(t_stop.clone());
            for (name, code) in &codes {
                builder = builder.with_script(name.clone(), code);
            }
            let mut context = builder.build().unwrap();
            let log: SimLog = Box::new(|_, _| {});
            SimScheduler::new(
                planned,
                cycle,
                CyclePolicy::EveryCycle,
                SimWake::default(),
                t_stop,
                log,
            )
            .run(&mut context);
        });
        std::thread::sleep(run_for);
        stop.store(true, Ordering::Relaxed);
        handle.join().unwrap();
        prints.lock().clone()
    }

    const BUSY_60_MS: &str = "local t = C_Time:GetMs(); while C_Time:GetMs() - t < 60 do end";
    const BUSY_80_MS: &str = "local t = C_Time:GetMs(); while C_Time:GetMs() - t < 80 do end";

    fn count(prints: &[(Instant, String)], tag: &str) -> usize {
        prints.iter().filter(|(_, l)| l == tag).count()
    }

    #[test]
    /// SC-R-102 — an interval script runs once at start, without waiting for its period.
    fn ut_interval_script_runs_at_start() {
        let sim = Sim::spawn(
            &[("i", "print('i')")],
            vec![interval("i", Duration::from_secs(3600))],
            Duration::from_secs(3600),
            CyclePolicy::EveryCycle,
            SimWake::default(),
            None,
        );
        sim.wait_lines("first run", 1);
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(sim.finish(), vec!["i".to_string()]);
    }

    #[test]
    /// SC-R-103 — the period runs from the start of the previous run: a 60 ms body inside a
    /// 250 ms period leaves starts 250 ms apart, where an end-anchored period gives 310 ms.
    fn ut_interval_period_is_measured_from_run_start() {
        let code = format!("print('i'); {BUSY_60_MS}");
        let prints = run_stamped(
            &[("i", &code)],
            vec![interval("i", Duration::from_millis(250))],
            Duration::from_secs(3600),
            Duration::from_millis(1400),
        );
        assert!(prints.len() >= 4, "{} runs", prints.len());
        for pair in prints.windows(2) {
            let gap = pair[1].0.duration_since(pair[0].0);
            assert!(gap >= Duration::from_millis(250), "{gap:?}");
            assert!(gap < Duration::from_millis(300), "{gap:?}");
        }
    }

    #[test]
    /// SC-R-104 — periods that elapse while the thread is busy yield one late run, never a
    /// back-to-back catch-up.
    fn ut_late_interval_run_does_not_catch_up() {
        let busy = format!("print('c'); {BUSY_80_MS}");
        let prints = run_stamped(
            &[("i", "print('i')"), ("c", &busy)],
            vec![interval("i", Duration::from_millis(50)), every_cycle("c")],
            Duration::from_millis(50),
            Duration::from_millis(1000),
        );
        let starts: Vec<Instant> = prints
            .iter()
            .filter(|(_, l)| l == "i")
            .map(|(at, _)| *at)
            .collect();
        assert!(starts.len() >= 3, "{} runs", starts.len());
        assert!(starts.len() <= 11, "{} runs", starts.len());
        for pair in starts.windows(2) {
            let gap = pair[1].duration_since(pair[0]);
            assert!(gap >= Duration::from_millis(50), "{gap:?}");
        }
    }

    #[test]
    /// SC-R-100 — a no-trigger script keeps its own cycle beside an interval script: 50 ms
    /// cycle, 250 ms period.
    fn ut_untriggered_script_still_runs_every_cycle_beside_interval_script() {
        let prints = run_stamped(
            &[("i", "print('i')"), ("c", "print('c')")],
            vec![interval("i", Duration::from_millis(250)), every_cycle("c")],
            Duration::from_millis(50),
            Duration::from_millis(1300),
        );
        assert!(
            count(&prints, "c") >= 15,
            "{} cycle runs",
            count(&prints, "c")
        );
        assert!(
            (5..=6).contains(&count(&prints, "i")),
            "{} interval runs",
            count(&prints, "i")
        );
    }

    #[test]
    /// SC-R-013 — the sleep ends by the earliest due time: an interval due soon beats a far
    /// cycle, a past one is no sleep, and with nothing due the stop poll bounds it.
    fn ut_sleep_bound_covers_the_earliest_due_time() {
        let now = Instant::now();
        let hour = now + Duration::from_secs(3600);
        assert_eq!(
            sleep_for(
                now,
                Some(hour),
                [now + Duration::from_millis(10)].into_iter()
            ),
            Duration::from_millis(10)
        );
        assert_eq!(
            sleep_for(now + Duration::from_secs(1), None, [now].into_iter()),
            Duration::ZERO
        );
        assert_eq!(sleep_for(now, Some(hour), std::iter::empty()), STOP_POLL);
        assert_eq!(sleep_for(now, None, std::iter::empty()), STOP_POLL);
    }

    #[test]
    /// SC-R-105 — neither an interval nor a no-trigger script is called with an argument.
    fn ut_interval_and_untriggered_scripts_get_no_argument() {
        let sim = Sim::spawn(
            &[
                ("i", "print(select('#', ...))"),
                ("c", "print(select('#', ...))"),
            ],
            vec![interval("i", Duration::from_secs(3600)), every_cycle("c")],
            Duration::from_secs(3600),
            CyclePolicy::EveryCycle,
            SimWake::default(),
            None,
        );
        sim.wait_lines("both ran", 2);
        assert_eq!(sim.finish(), vec!["0".to_string(), "0".to_string()]);
    }

    #[test]
    /// SC-R-110 — an error in an interval or an event script is logged at error level with
    /// the `[sim]` prefix, and the script's next run still happens.
    fn ut_error_in_event_or_interval_script_is_logged_and_next_run_still_happens() {
        let stream = EventStream::new();
        let wake = SimWake::default();
        let sim = Sim::spawn(
            &[
                ("i", "error('interval boom')"),
                ("e", "error('event boom')"),
            ],
            vec![
                interval("i", Duration::from_millis(50)),
                events_script("e", feed(&stream, &wake)),
            ],
            Duration::from_secs(3600),
            CyclePolicy::EveryCycle,
            wake,
            None,
        );
        stream.publish_with(|_| 1);
        stream.publish_with(|_| 2);
        let seen = |text: &str| {
            sim.logged
                .lock()
                .iter()
                .filter(|(_, l)| l.contains(text))
                .count()
        };
        wait_until_blocking(
            "two errors per script",
            Duration::from_millis(5),
            Duration::from_secs(3),
            || (seen("interval boom") >= 2 && seen("event boom") >= 2).then_some(()),
        );
        let logged = sim.logged.lock().clone();
        sim.finish();
        for (level, line) in &logged {
            assert_eq!(*level, Level::Error);
            assert!(line.starts_with("[sim] "), "{line}");
        }
    }

    #[test]
    /// SC-R-101 — an interval period resolves as the cycle interval does: fallback to 1 s for
    /// a non-finite or non-positive value, then the floor.
    fn ut_interval_schedule_resolves_like_the_cycle_interval() {
        let period = |seconds, floor| match ScriptSchedule::interval(seconds, floor) {
            ScriptSchedule::Interval { period } => period,
            _ => unreachable!("an interval schedule"),
        };
        for bad in [f64::NAN, 0.0, -1.0, f64::INFINITY] {
            assert_eq!(period(bad, 0.05), Duration::from_secs(1), "{bad}");
        }
        assert_eq!(period(0.01, 0.05), Duration::from_millis(50));
        assert_eq!(period(0.01, 0.0), Duration::from_millis(10));
        assert_eq!(period(2.5, 0.05), Duration::from_millis(2500));
    }

    #[test]
    /// SC-R-044 — the at-most-once-per-interval policy gates the no-trigger scripts only: an
    /// interval script keeps its own period.
    fn ut_at_most_once_policy_applies_only_to_untriggered_scripts() {
        let sim = Sim::spawn(
            &[("i", "print('i')"), ("c", "print('c')")],
            vec![interval("i", Duration::from_millis(100)), every_cycle("c")],
            Duration::from_millis(400),
            CyclePolicy::AtMostOncePerInterval,
            SimWake::default(),
            None,
        );
        std::thread::sleep(Duration::from_millis(1000));
        let lines = sim.finish();
        let runs = |tag: &str| lines.iter().filter(|l| *l == tag).count();
        assert!(runs("i") >= 6, "{} interval runs", runs("i"));
        assert!((1..=2).contains(&runs("c")), "{} cycle runs", runs("c"));
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

    #[derive(Clone)]
    struct Ev(i64);

    impl mlua::IntoLua for Ev {
        fn into_lua(self, lua: &mlua::Lua) -> mlua::Result<mlua::Value> {
            let table = lua.create_table()?;
            table.set("n", self.0)?;
            Ok(mlua::Value::Table(table))
        }
    }

    fn overflowed_feed(stream: &EventStream<Ev>, wake: &SimWake, extra: i64) -> QueueFeed<Ev> {
        let feed = QueueFeed::attach(stream.clone(), Arc::new(|_| true), wake.clone());
        for n in 0..4096 + extra {
            stream.publish_with(|_| Ev(n));
        }
        feed
    }

    fn error_lines(sim: &Sim) -> Vec<String> {
        sim.logged
            .lock()
            .iter()
            .filter(|(level, line)| *level == Level::Error && line.starts_with("[sim] "))
            .map(|(_, line)| line.clone())
            .collect()
    }

    #[test]
    /// SC-R-107, SC-R-108, SC-R-109 — a burst of 100 drops logs one Error line for the first
    /// drop and one for the other 99 when the script takes its next event, and the script sees
    /// the total on that event, then 0.
    fn ut_overflow_burst_logs_first_drop_then_total() {
        let stream = EventStream::new();
        let wake = SimWake::default();
        let feed = overflowed_feed(&stream, &wake, 100);
        let sim = Sim::spawn(
            &[("e", "local e = ...; print(e.dropped)")],
            vec![events_script("e", feed)],
            Duration::from_secs(3600),
            CyclePolicy::EveryCycle,
            wake,
            Some(2),
        );
        let errors = {
            wait_until_blocking(
                "both drop lines",
                Duration::from_millis(5),
                Duration::from_secs(2),
                || (sim.lines.lock().len() >= 2).then_some(()),
            );
            error_lines(&sim)
        };
        let lines = sim.finish();
        assert_eq!(lines[..2], ["100", "0"]);
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(
            errors[0].contains("'e'") && errors[0].contains(" 1 "),
            "{errors:?}"
        );
        assert!(
            errors[1].contains("'e'") && errors[1].contains("99"),
            "{errors:?}"
        );
    }

    #[test]
    /// SC-R-106 — the first-drop line is written before the next script execution, whichever
    /// script that is.
    fn ut_first_drop_line_written_before_next_execution() {
        let stream = EventStream::new();
        let wake = SimWake::default();
        let feed = overflowed_feed(&stream, &wake, 1);
        let sim = Sim::spawn(
            &[("e", "local e = ..."), ("c", "print('tick')")],
            vec![events_script("e", feed), every_cycle("c")],
            Duration::from_secs(3600),
            CyclePolicy::EveryCycle,
            wake,
            None,
        );
        sim.wait_lines("tick", 1);
        let timeline = sim.timeline.lock().clone();
        sim.finish();
        let first = timeline.iter().position(|l| l.contains("dropped"));
        let tick = timeline.iter().position(|l| l == "tick");
        assert!(first.is_some() && first < tick, "{timeline:?}");
    }

    /// A feed whose first-drop flag was already taken by the time its event is popped.
    struct RacedFeed(bool);

    impl ScriptFeed for RacedFeed {
        fn run_next(
            &mut self,
            _context: &mut Context<String>,
            _key: &String,
            on_take: &mut dyn FnMut(DropReport),
        ) -> Option<ferrowl_lua::Result<()>> {
            if std::mem::replace(&mut self.0, true) {
                return None;
            }
            on_take(DropReport {
                first_unlogged: true,
                further: 4,
            });
            Some(Ok(()))
        }

        fn take_first_drop(&self) -> bool {
            false
        }
    }

    #[test]
    /// SC-R-107 — a drop reported only at the pop writes both lines there: the first drop and
    /// the further ones.
    fn ut_raced_pop_writes_both_lines() {
        let sim = Sim::spawn(
            &[("e", "local e = ...")],
            vec![events_script("e", RacedFeed(false))],
            Duration::from_secs(3600),
            CyclePolicy::EveryCycle,
            SimWake::default(),
            None,
        );
        wait_until_blocking(
            "both lines",
            Duration::from_millis(5),
            Duration::from_secs(2),
            || (error_lines(&sim).len() >= 2).then_some(()),
        );
        let errors = error_lines(&sim);
        sim.finish();
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(errors[0].contains(" 1 "), "{errors:?}");
        assert!(errors[1].contains(" 4 "), "{errors:?}");
    }
}
