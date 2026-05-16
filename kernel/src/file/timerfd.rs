use alloc::{borrow::Cow, sync::Arc};
use core::{
    sync::atomic::{AtomicBool, Ordering},
    task::Context,
};

use axerrno::AxError;
use axhal::time::TimeValue;
use axpoll::{IoEvents, PollSet, Pollable};
use axsync::Mutex;
use axtask::future::{block_on, sleep};

use crate::file::{FileLike, IoDst, IoSrc};

pub struct TimerFd {
    clock_id: i32,
    /// it_value: 0 means disarmed
    value: Mutex<TimeValue>,
    /// it_interval: 0 means one-shot timer
    interval: Mutex<TimeValue>,
    /// TFD_TIMER_ABSTIME flag
    abstime: AtomicBool,
    /// When the timer was last set (monotonic time)
    start_time: Mutex<TimeValue>,
    non_blocking: AtomicBool,
    poll_rx: PollSet,
}

impl TimerFd {
    pub fn new(clock_id: i32) -> Arc<Self> {
        Arc::new(Self {
            clock_id,
            value: Mutex::new(TimeValue::ZERO),
            interval: Mutex::new(TimeValue::ZERO),
            abstime: AtomicBool::new(false),
            start_time: Mutex::new(TimeValue::ZERO),
            non_blocking: AtomicBool::new(false),
            poll_rx: PollSet::new(),
        })
    }

    /// Returns (old_value, old_interval, old_abstime).
    pub fn settle(
        &self,
        new_value: TimeValue,
        new_interval: TimeValue,
        abstime: bool,
    ) -> (TimeValue, TimeValue) {
        let now = self.now();
        let old_value = core::mem::replace(&mut *self.value.lock(), new_value);
        let old_interval = core::mem::replace(&mut *self.interval.lock(), new_interval);
        self.abstime.store(abstime, Ordering::Release);

        // Record the start time. For absolute timers, we store the deadline directly
        // and compute remaining time relative to "start_time" (which is now).
        *self.start_time.lock() = now;

        self.poll_rx.wake();
        (old_value, old_interval)
    }

    pub fn get_time(&self) -> (TimeValue, TimeValue) {
        let value = *self.value.lock();
        let interval = *self.interval.lock();
        if value.is_zero() {
            return (value, interval);
        }

        let now = self.now();
        let start = *self.start_time.lock();
        let elapsed = now.checked_sub(start).unwrap_or(TimeValue::ZERO);

        let remaining = if self.abstime.load(Ordering::Acquire) {
            value.checked_sub(now).unwrap_or(TimeValue::ZERO)
        } else {
            value.checked_sub(elapsed).unwrap_or(TimeValue::ZERO)
        };

        (remaining, interval)
    }

    fn now(&self) -> TimeValue {
        match self.clock_id as u32 {
            linux_raw_sys::general::CLOCK_REALTIME => axhal::time::wall_time(),
            _ => axhal::time::monotonic_time(),
        }
    }

    fn count_expirations(
        &self,
        value: TimeValue,
        interval: TimeValue,
        start: TimeValue,
        now: TimeValue,
        abstime: bool,
    ) -> u64 {
        let elapsed = now.checked_sub(start).unwrap_or(TimeValue::ZERO);

        if abstime {
            // For absolute time, the timer expires when now >= value.
            if now < value {
                return 0;
            }
            let since_deadline = now.checked_sub(value).unwrap_or(TimeValue::ZERO);
            if interval.is_zero() {
                return 1;
            }
            1 + (since_deadline.as_nanos() / interval.as_nanos().max(1)) as u64
        } else {
            if elapsed < value {
                return 0;
            }
            if interval.is_zero() {
                return 1;
            }
            let since_first = elapsed.checked_sub(value).unwrap_or(TimeValue::ZERO);
            1 + (since_first.as_nanos() / interval.as_nanos().max(1)) as u64
        }
    }
}

impl FileLike for TimerFd {
    fn read(&self, dst: &mut IoDst) -> axio::Result<usize> {
        if dst.remaining_mut() < size_of::<u64>() {
            return Err(AxError::InvalidInput);
        }

        let value = *self.value.lock();
        if value.is_zero() {
            return Err(AxError::WouldBlock);
        }

        let _interval = *self.interval.lock();
        let start = *self.start_time.lock();
        let abstime = self.abstime.load(Ordering::Acquire);
        let now = self.now();

        // Check if timer has expired
        let elapsed = now.checked_sub(start).unwrap_or(TimeValue::ZERO);
        let expired = if abstime {
            now >= value
        } else {
            elapsed >= value
        };

        if !expired {
            if self.nonblocking() {
                return Err(AxError::WouldBlock);
            }

            // Calculate remaining time and sleep
            let remaining = if abstime {
                value.checked_sub(now).unwrap_or(TimeValue::ZERO)
            } else {
                value.checked_sub(elapsed).unwrap_or(TimeValue::ZERO)
            };

            if !remaining.is_zero() {
                let _ = block_on(sleep(remaining));
            }

            // Re-check after sleep
            let value = *self.value.lock();
            if value.is_zero() {
                return Err(AxError::WouldBlock);
            }
        }

        // Re-read current values under lock (may have changed during sleep)
        let value = *self.value.lock();
        let interval = *self.interval.lock();
        let start = *self.start_time.lock();
        let abstime = self.abstime.load(Ordering::Acquire);
        let now = self.now();

        if value.is_zero() {
            return Err(AxError::WouldBlock);
        }

        let count = self.count_expirations(value, interval, start, now, abstime);
        if count == 0 {
            return Err(AxError::WouldBlock);
        }

        // Update start_time for next read
        if !interval.is_zero() {
            // Advance start_time by count * interval
            let nanos = interval.as_nanos() as u64 * count;
            let advance = TimeValue::from_nanos(nanos);
            let new_start = start.checked_add(advance).unwrap_or(now);
            *self.start_time.lock() = new_start;
        } else {
            // One-shot: disarm
            *self.value.lock() = TimeValue::ZERO;
            *self.start_time.lock() = now;
        }

        dst.write(&count.to_ne_bytes())?;
        self.poll_rx.wake();
        Ok(size_of::<u64>())
    }

    fn write(&self, _src: &mut IoSrc) -> axio::Result<usize> {
        Err(AxError::InvalidInput)
    }

    fn nonblocking(&self) -> bool {
        self.non_blocking.load(Ordering::Acquire)
    }

    fn set_nonblocking(&self, non_blocking: bool) -> axio::Result {
        self.non_blocking.store(non_blocking, Ordering::Release);
        Ok(())
    }

    fn path(&self) -> Cow<'_, str> {
        "anon_inode:[timerfd]".into()
    }
}

impl Pollable for TimerFd {
    fn poll(&self) -> IoEvents {
        let mut events = IoEvents::empty();
        let value = *self.value.lock();
        if !value.is_zero() {
            let interval = *self.interval.lock();
            let start = *self.start_time.lock();
            let now = self.now();
            let abstime = self.abstime.load(Ordering::Acquire);
            let count = self.count_expirations(value, interval, start, now, abstime);
            events.set(IoEvents::IN, count > 0);
        }
        events
    }

    fn register(&self, context: &mut Context<'_>, events: IoEvents) {
        if events.contains(IoEvents::IN) {
            self.poll_rx.register(context.waker());
        }
    }
}
