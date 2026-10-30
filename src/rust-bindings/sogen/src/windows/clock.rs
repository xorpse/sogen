use std::sync::Arc;
use std::time::{Duration, SystemTime};

use sogen_sys::windows::ClockBridge;

/// The emulator's source of time. It is called concurrently from several host threads, also while a callback runs, and
/// must not use the emulator.
pub trait Clock: Send + Sync + 'static {
    /// Must lie within 292 years of the Unix epoch.
    fn system_time(&self) -> SystemTime;
    /// Monotonic time since a fixed but arbitrary point. Guest waits and timers only expire as it advances.
    fn steady_time(&self) -> Duration;
    fn timestamp_counter(&self) -> u64;
}

pub(super) struct ClockAdapter(pub(super) Arc<dyn Clock>);

impl ClockBridge for ClockAdapter {
    fn system_time_nanoseconds(&self) -> i64 {
        unix_nanoseconds(self.0.system_time())
    }

    fn steady_time_nanoseconds(&self) -> u64 {
        u64::try_from(self.0.steady_time().as_nanos())
            .expect("the clock's steady time is within 584 years")
    }

    fn timestamp_counter(&self) -> u64 {
        self.0.timestamp_counter()
    }
}

/// Nanoseconds since the Unix epoch, negative before it.
pub(super) fn unix_nanoseconds(time: SystemTime) -> i64 {
    let nanoseconds = match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_nanos()),
        Err(before) => i64::try_from(before.duration().as_nanos()).map(|nanoseconds| -nanoseconds),
    };
    nanoseconds.expect("the time is within 292 years of the Unix epoch")
}

pub(super) fn system_time(nanoseconds: i64) -> SystemTime {
    let offset = Duration::from_nanos(nanoseconds.unsigned_abs());
    if nanoseconds < 0 {
        SystemTime::UNIX_EPOCH - offset
    } else {
        SystemTime::UNIX_EPOCH + offset
    }
}
