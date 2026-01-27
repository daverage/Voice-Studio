//! Centralized logging and debugging for Voice Studio.
//!
//! Provides a feature-gated logging mechanism that is safe for use
//! (with caution) during real-time processing.

use std::fmt;

#[cfg(feature = "debug")]
pub mod logger {
    use std::cell::UnsafeCell;
    use std::fmt;
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::OnceLock;

    const LOG_CAP: usize = 256;
    const LOG_MSG_MAX: usize = 1024;

    #[derive(Copy, Clone)]
    struct LogEntry {
        len: u16,
        bytes: [u8; LOG_MSG_MAX],
    }

    impl Default for LogEntry {
        fn default() -> Self {
            Self {
                len: 0,
                bytes: [0; LOG_MSG_MAX],
            }
        }
    }

    struct LogRing {
        head: AtomicUsize,
        tail: AtomicUsize,
        buf: Box<[UnsafeCell<LogEntry>]>,
    }

    unsafe impl Sync for LogRing {}

    impl LogRing {
        fn new() -> Self {
            let mut v = Vec::with_capacity(LOG_CAP);
            for _ in 0..LOG_CAP {
                v.push(UnsafeCell::new(LogEntry::default()));
            }
            Self {
                head: AtomicUsize::new(0),
                tail: AtomicUsize::new(0),
                buf: v.into_boxed_slice(),
            }
        }

        fn push(&self, entry: LogEntry) {
            let cap = self.buf.len();
            let head = self.head.load(Ordering::Relaxed);
            let next = (head + 1) % cap;
            if next == self.tail.load(Ordering::Acquire) {
                return;
            }
            unsafe {
                *self.buf[head].get() = entry;
            }
            self.head.store(next, Ordering::Release);
        }

        fn pop(&self) -> Option<LogEntry> {
            let cap = self.buf.len();
            let tail = self.tail.load(Ordering::Relaxed);
            if tail == self.head.load(Ordering::Acquire) {
                return None;
            }
            let entry = unsafe { *self.buf[tail].get() };
            self.tail.store((tail + 1) % cap, Ordering::Release);
            Some(entry)
        }
    }

    static LOGGER: OnceLock<LogRing> = OnceLock::new();
    static LOG_ENABLED: AtomicBool = AtomicBool::new(false);

    pub fn init_logger() {
        let _ = LOGGER.get_or_init(LogRing::new);
        LOG_ENABLED.store(true, Ordering::Relaxed);
    }

    struct FixedBuf {
        buf: [u8; LOG_MSG_MAX],
        len: usize,
    }

    impl FixedBuf {
        fn new() -> Self {
            Self {
                buf: [0; LOG_MSG_MAX],
                len: 0,
            }
        }

        fn as_entry(&self) -> LogEntry {
            let mut entry = LogEntry::default();
            entry.len = self.len.min(LOG_MSG_MAX) as u16;
            entry.bytes[..self.len].copy_from_slice(&self.buf[..self.len]);
            entry
        }
    }

    impl fmt::Write for FixedBuf {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            let avail = LOG_MSG_MAX - self.len;
            if avail == 0 {
                return Ok(());
            }
            let bytes = s.as_bytes();
            let n = bytes.len().min(avail);
            self.buf[self.len..self.len + n].copy_from_slice(&bytes[..n]);
            self.len += n;
            Ok(())
        }
    }

    pub fn log_args(args: fmt::Arguments) {
        if !LOG_ENABLED.load(Ordering::Relaxed) {
            return;
        }
        let logger = match LOGGER.get() {
            Some(l) => l,
            None => return,
        };

        let mut buf = FixedBuf::new();
        let _ = fmt::write(&mut buf, args);
        logger.push(buf.as_entry());
    }

    pub fn drain_to_file() {
        if !LOG_ENABLED.load(Ordering::Relaxed) {
            return;
        }
        let logger = match LOGGER.get() {
            Some(l) => l,
            None => return,
        };
        let mut file = match OpenOptions::new()
            .create(true)
            .append(true)
            .open("/tmp/voice_studio.log")
        {
            Ok(f) => f,
            Err(_) => return,
        };

        while let Some(entry) = logger.pop() {
            let len = entry.len as usize;
            if len == 0 {
                continue;
            }
            let msg = std::str::from_utf8(&entry.bytes[..len]).unwrap_or("<invalid>");
            let _ = writeln!(file, "{}", msg);
        }
    }
}

#[cfg(feature = "debug")]
pub(crate) fn vs_log_inner(args: fmt::Arguments) {
    logger::log_args(args);
}

#[cfg(not(feature = "debug"))]
pub(crate) fn vs_log_inner(_args: fmt::Arguments) {}

#[macro_export]
macro_rules! vs_log {
    ($($arg:tt)*) => {
        $crate::debug::vs_log_inner(format_args!($($arg)*))
    };
}
