//! Fixed-size, allocation-free ring buffer for log messages.

use std::time::{SystemTime, UNIX_EPOCH};

/// A ring buffer holding up to `LOG_SIZE - 1` timestamped log lines of at
/// most `MAX_LINE_LENGTH` characters each.
///
/// Storage is fully inline (no heap allocation on write): longer messages
/// are truncated, shorter ones padded with `'\0'`. When the buffer is full,
/// writing evicts the oldest entry. Entries carry the Unix timestamp in
/// milliseconds taken at write time.
pub struct Log<const MAX_LINE_LENGTH: usize, const LOG_SIZE: usize> {
    buffer: [[char; MAX_LINE_LENGTH]; LOG_SIZE],
    timestamps: [u64; LOG_SIZE],
    write: usize,
    read: usize,
}

impl<const MAX_LINE_LENGTH: usize, const LOG_SIZE: usize> Log<MAX_LINE_LENGTH, LOG_SIZE> {
    /// Creates an empty log.
    pub fn init() -> Self {
        let buffer = [['\0'; MAX_LINE_LENGTH]; LOG_SIZE];

        Self {
            buffer,
            timestamps: [0u64; LOG_SIZE],
            write: 0,
            read: 0,
        }
    }

    /// Appends `msg` with the current timestamp, truncating it to
    /// `MAX_LINE_LENGTH` characters. Evicts the oldest entry if full.
    pub fn write(&mut self, msg: &str) {
        self.timestamps[self.write] = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        for (dst, src) in self.buffer[self.write].iter_mut().zip(
            msg.chars()
                .chain(std::iter::repeat_n('\0', MAX_LINE_LENGTH)),
        ) {
            *dst = src;
        }
        let next = (self.write + 1) % LOG_SIZE;
        if next == self.read {
            self.read = (self.read + 1) % LOG_SIZE;
        }
        self.write = next;
    }

    /// The (timestamp, message) entry stored at ring index `idx`.
    fn entry_at(&self, idx: usize) -> (u64, String) {
        (
            self.timestamps[idx],
            self.buffer[idx].iter().collect::<String>(),
        )
    }

    /// Returns the oldest entry without removing it, or `None` if empty.
    pub fn peak(&self) -> Option<(u64, String)> {
        if self.read != self.write {
            Some(self.entry_at(self.read))
        } else {
            None
        }
    }

    /// Returns up to `cnt` entries, oldest first, without removing them.
    /// `None` if the log is empty.
    pub fn peak_n(&self, cnt: usize) -> Option<Vec<(u64, String)>> {
        let mut read = self.read;
        let mut msgs = Vec::with_capacity(cnt);
        while read != self.write && msgs.len() < cnt {
            msgs.push(self.entry_at(read));
            read = (read + 1) % LOG_SIZE;
        }
        if msgs.is_empty() { None } else { Some(msgs) }
    }

    /// Removes and returns the oldest entry, or `None` if empty.
    pub fn take(&mut self) -> Option<(u64, String)> {
        if self.read != self.write {
            let msg = self.entry_at(self.read);
            self.read = (self.read + 1) % LOG_SIZE;
            Some(msg)
        } else {
            None
        }
    }

    /// Removes and returns up to `cnt` entries, oldest first. `None` if the
    /// log is empty.
    pub fn take_n(&mut self, cnt: usize) -> Option<Vec<(u64, String)>> {
        let mut msgs = Vec::with_capacity(cnt);
        while self.read != self.write && msgs.len() < cnt {
            msgs.push(self.entry_at(self.read));
            self.read = (self.read + 1) % LOG_SIZE;
        }
        if msgs.is_empty() { None } else { Some(msgs) }
    }

    pub fn clear(&mut self) {
        self.write = 0;
        self.read = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::Log;

    #[test]
    fn ut_log() {
        let mut log: Log<10, 5> = Log::init();
        log.write("some message");

        let peak = log.peak();
        assert!(peak.is_some());
        assert_eq!(peak.unwrap().1, "some messa");

        let take = log.take();
        assert!(take.is_some());
        assert_eq!(take.unwrap().1, "some messa");

        let take = log.take();
        assert!(take.is_none());

        log.write("message 1");
        log.write("message 2");
        log.write("message 3");
        log.write("message 4");

        let peak = log.peak_n(3);
        assert!(peak.is_some());
        assert_eq!(peak.unwrap().len(), 3);

        let peak = log.peak_n(6);
        assert!(peak.is_some());
        assert_eq!(peak.unwrap().len(), 4);

        let take = log.take_n(3);
        assert!(take.is_some());
        assert_eq!(take.unwrap().len(), 3);

        let take = log.take_n(3);
        assert!(take.is_some());
        assert_eq!(take.unwrap().len(), 1);

        let take = log.take_n(3);
        assert!(take.is_none());

        let peak = log.peak_n(2);
        assert!(peak.is_none());
    }

    #[test]
    fn ut_log_empty() {
        let log: Log<10, 5> = Log::init();
        assert!(log.peak().is_none());
        assert!(log.peak_n(1).is_none());
    }

    #[test]
    fn ut_log_ring_overflow_evicts_oldest() {
        // LOG_SIZE=3 can hold at most 2 messages
        let mut log: Log<10, 3> = Log::init();
        log.write("msg1______");
        log.write("msg2______");
        log.write("msg3______"); // evicts msg1

        let msgs = log.peak_n(2).unwrap();
        assert_eq!(msgs.len(), 2);
        assert!(msgs[0].1.starts_with("msg2"));
        assert!(msgs[1].1.starts_with("msg3"));
    }

    #[test]
    fn ut_log_take_n_drains_all() {
        let mut log: Log<10, 5> = Log::init();
        log.write("aaaaaaaaaa");
        log.write("bbbbbbbbbb");
        log.write("cccccccccc");

        let taken = log.take_n(10).unwrap();
        assert_eq!(taken.len(), 3);
        assert!(log.take_n(1).is_none());
    }

    #[test]
    fn ut_log_truncation() {
        let mut log: Log<5, 3> = Log::init();
        log.write("abcde");
        log.write("abcdefgh");

        let first = log.take().unwrap();
        assert_eq!(&first.1, "abcde");

        let second = log.take().unwrap();
        assert_eq!(&second.1, "abcde");
    }

    #[test]
    fn ut_log_fills_to_capacity_then_evicts_in_order() {
        // LOG_SIZE=4 → holds at most 3 entries.
        let mut log: Log<10, 4> = Log::init();
        log.write("msg1______");
        log.write("msg2______");
        log.write("msg3______");
        // At capacity: all three present, oldest first.
        let full = log.peak_n(10).unwrap();
        assert_eq!(full.len(), 3);
        assert!(full[0].1.starts_with("msg1"));

        // One more evicts the oldest (msg1) and keeps the window at 3.
        log.write("msg4______");
        let rolled = log.peak_n(10).unwrap();
        assert_eq!(rolled.len(), 3);
        assert!(rolled[0].1.starts_with("msg2"));
        assert!(rolled[2].1.starts_with("msg4"));
    }

    #[test]
    fn ut_log_peak_n_zero_returns_none() {
        let mut log: Log<10, 5> = Log::init();
        log.write("something_");
        assert!(log.peak_n(0).is_none());
        assert!(log.take_n(0).is_none());
        // The entry is untouched by the zero-count calls.
        assert!(log.peak().is_some());
    }

    #[test]
    fn ut_log_clear_empties_the_buffer() {
        let mut log: Log<10, 5> = Log::init();
        log.write("a_________");
        log.write("b_________");
        log.clear();
        assert!(log.peak().is_none());
        assert!(log.take_n(5).is_none());
        // Usable again after clear.
        log.write("c_________");
        assert!(log.peak().unwrap().1.starts_with("c"));
    }

    #[test]
    fn ut_log_records_timestamp_on_write() {
        let mut log: Log<10, 5> = Log::init();
        log.write("stamped___");
        let (ts, _) = log.peak().unwrap();
        // A real Unix-millis timestamp is far from zero.
        assert!(ts > 0);
    }
}
