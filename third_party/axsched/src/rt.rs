use alloc::sync::Arc;
use core::ops::Deref;
use core::sync::atomic::{AtomicI8, AtomicIsize, Ordering};
use linked_list_r4l::{GetLinks, Links, List};

use crate::BaseScheduler;

/// SCHED_OTHER — the default time-sharing policy (priority 0, nice -20..19).
pub const SCHED_OTHER: i32 = 0;
/// SCHED_FIFO — real-time first-in-first-out (no time slice).
pub const SCHED_FIFO: i32 = 1;
/// SCHED_RR — real-time round-robin (with time slice).
pub const SCHED_RR: i32 = 2;


/// A task wrapper for the [`RtScheduler`].
///
/// Each task has a priority (0–N_PRIO-1), a scheduling policy, and an optional
/// time slice for round-robin scheduling.
pub struct RtTask<T, const N_PRIO: usize> {
    inner: T,
    /// RT priority: 0 is lowest (SCHED_OTHER), 1–99 higher (SCHED_FIFO/RR).
    priority: AtomicI8,
    /// Scheduling policy: SCHED_OTHER, SCHED_FIFO, or SCHED_RR.
    policy: AtomicI8,
    /// Remaining time slice (only meaningful for SCHED_RR).
    time_slice: AtomicIsize,
    links: Links<Self>,
}

impl<T, const N_PRIO: usize> RtTask<T, N_PRIO> {
    /// Creates a new [`RtTask`] with default SCHED_OTHER policy.
    pub const fn new(inner: T) -> Self {
        Self {
            inner,
            priority: AtomicI8::new(0),
            policy: AtomicI8::new(SCHED_OTHER as i8),
            time_slice: AtomicIsize::new(0),
            links: Links::new(),
        }
    }

    /// Returns the RT priority (0–N_PRIO-1).
    pub fn priority(&self) -> u8 {
        self.priority.load(Ordering::Acquire) as u8
    }

    /// Sets the RT priority.
    pub fn set_priority(&self, prio: u8) {
        self.priority.store(prio as i8, Ordering::Release);
    }

    /// Returns the scheduling policy.
    pub fn policy(&self) -> i32 {
        self.policy.load(Ordering::Acquire) as i32
    }

    /// Sets the scheduling policy.
    pub fn set_policy(&self, policy: i32) {
        self.policy.store(policy as i8, Ordering::Release);
    }

    fn time_slice(&self) -> isize {
        self.time_slice.load(Ordering::Acquire)
    }

    fn reset_time_slice(&self, max_time_slice: isize) {
        self.time_slice.store(max_time_slice, Ordering::Release);
    }

    fn dec_time_slice(&self) -> isize {
        self.time_slice.fetch_sub(1, Ordering::Release)
    }

    /// Returns a reference to the inner task struct.
    pub const fn inner(&self) -> &T {
        &self.inner
    }
}

impl<T, const N_PRIO: usize> GetLinks for RtTask<T, N_PRIO> {
    type EntryType = Self;

    fn get_links(data: &Self::EntryType) -> &Links<Self::EntryType> {
        &data.links
    }
}

impl<T, const N_PRIO: usize> Deref for RtTask<T, N_PRIO> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// A real-time priority scheduler with `N_PRIO` priority levels.
///
/// Each priority level has its own FIFO queue. SCHED_FIFO tasks run without
/// time slices; SCHED_RR tasks get [`MAX_TIME_SLICE`] ticks per round.
/// SCHED_OTHER tasks run at priority 0 with time slices.
///
/// # Scheduling Algorithm
///
/// 1. Always pick the highest-priority non-empty queue.
/// 2. Within a queue, FIFO order (SCHED_FIFO) or round-robin (SCHED_RR).
/// 3. SCHED_FIFO tasks are never preempted by time; only by higher-priority
///    tasks becoming runnable or by yielding voluntarily.
pub struct RtScheduler<T, const N_PRIO: usize, const MAX_TIME_SLICE: usize> {
    ready_queues: [List<Arc<RtTask<T, N_PRIO>>>; N_PRIO],
    /// Bitmap: bit i is set if ready_queues[i] is non-empty.
    /// Uses u128 to allow up to 128 priority levels.
    bitmap: u128,
}

// N_PRIO must be ≤ 128 (u128 bitmap limit). Checked at runtime in new().

impl<T, const N_PRIO: usize, const MAX_TIME_SLICE: usize> RtScheduler<T, N_PRIO, MAX_TIME_SLICE> {
    /// Creates a new empty [`RtScheduler`].
    pub fn new() -> Self {
        // u128 bitmap supports up to 128 priority levels
        assert!(N_PRIO <= 128);
        Self {
            ready_queues: core::array::from_fn(|_| List::new()),
            bitmap: 0,
        }
    }

    /// Returns the name of this scheduler.
    pub fn scheduler_name() -> &'static str {
        "Real-Time Priority"
    }

    fn set_queue_bit(&mut self, prio: usize) {
        self.bitmap |= 1u128 << prio;
    }

    fn clear_queue_bit_if_empty(&mut self, prio: usize) {
        if self.ready_queues[prio].is_empty() {
            self.bitmap &= !(1u128 << prio);
        }
    }

    /// Find the highest-priority non-empty queue.
    fn highest_prio(&self) -> Option<usize> {
        if self.bitmap == 0 {
            return None;
        }
        // leading_zeros on u128: bit 127 is MSB
        Some(127 - (self.bitmap.leading_zeros() as usize))
    }
}

impl<T, const N_PRIO: usize, const MAX_TIME_SLICE: usize> BaseScheduler
    for RtScheduler<T, N_PRIO, MAX_TIME_SLICE>
{
    type SchedItem = Arc<RtTask<T, N_PRIO>>;

    fn init(&mut self) {}

    fn add_task(&mut self, task: Self::SchedItem) {
        let prio = task.priority() as usize;
        self.ready_queues[prio].push_back(task);
        self.set_queue_bit(prio);
    }

    fn remove_task(&mut self, task: &Self::SchedItem) -> Option<Self::SchedItem> {
        let prio = task.priority() as usize;
        let removed = unsafe { self.ready_queues[prio].remove(task) };
        self.clear_queue_bit_if_empty(prio);
        removed
    }

    fn pick_next_task(&mut self) -> Option<Self::SchedItem> {
        let prio = self.highest_prio()?;
        let task = self.ready_queues[prio].pop_front();
        self.clear_queue_bit_if_empty(prio);
        task
    }

    fn put_prev_task(&mut self, prev: Self::SchedItem, preempt: bool) {
        let prio = prev.priority() as usize;
        let policy = prev.policy();

        if policy == SCHED_RR && prev.time_slice() > 0 && preempt {
            // SCHED_RR was preempted but still has time slice: put at front
            self.ready_queues[prio].push_front(prev);
        } else {
            // Reset time slice for next round
            if policy == SCHED_RR {
                prev.reset_time_slice(MAX_TIME_SLICE as isize);
            }
            self.ready_queues[prio].push_back(prev);
        }
        self.set_queue_bit(prio);
    }

    fn task_tick(&mut self, current: &Self::SchedItem) -> bool {
        let policy = current.policy();
        match policy {
            SCHED_FIFO => {
                // SCHED_FIFO is never preempted by time slice.
                // But check if a higher-priority task is waiting.
                let curr_prio = current.priority() as usize;
                self.highest_prio().is_some_and(|p| p > curr_prio)
            }
            SCHED_RR => {
                let old = current.dec_time_slice();
                if old <= 1 {
                    // Time slice exhausted, re-schedule.
                    return true;
                }
                // Also check for higher-priority tasks.
                let curr_prio = current.priority() as usize;
                self.highest_prio().is_some_and(|p| p > curr_prio)
            }
            _ => {
                // SCHED_OTHER: always preemptible by RT tasks
                self.highest_prio().is_some_and(|p| p > 0)
            }
        }
    }

    fn set_priority(&mut self, task: &Self::SchedItem, prio: isize) -> bool {
        if prio < 0 || prio >= N_PRIO as isize {
            return false;
        }
        let new_prio = prio as u8;
        let old_prio = task.priority();
        if old_prio == new_prio {
            return true;
        }
        // Remove from old queue
        let removed = unsafe { self.ready_queues[old_prio as usize].remove(task) };
        self.clear_queue_bit_if_empty(old_prio as usize);
        if let Some(task) = removed {
            task.set_priority(new_prio);
            self.ready_queues[new_prio as usize].push_back(task);
            self.set_queue_bit(new_prio as usize);
            true
        } else {
            // Task wasn't in the queue (e.g., it's running), just update priority
            task.set_priority(new_prio);
            true
        }
    }
}

impl<T, const N_PRIO: usize, const MAX_TIME_SLICE: usize> Default
    for RtScheduler<T, N_PRIO, MAX_TIME_SLICE>
{
    fn default() -> Self {
        Self::new()
    }
}
