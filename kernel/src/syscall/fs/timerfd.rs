use axerrno::{AxError, AxResult};
use linux_raw_sys::general::{
    CLOCK_BOOTTIME, CLOCK_MONOTONIC, CLOCK_REALTIME,
    TFD_CLOEXEC, TFD_NONBLOCK, TFD_TIMER_ABSTIME, itimerspec, timespec,
};
use starry_vm::{VmMutPtr, VmPtr};

use crate::{
    file::{FileLike, add_file_like, timerfd::TimerFd},
    time::TimeValueLike,
};

fn check_clock_id(clock_id: i32) -> AxResult {
    match clock_id as u32 {
        CLOCK_REALTIME | CLOCK_MONOTONIC | CLOCK_BOOTTIME => Ok(()),
        _ => Err(AxError::InvalidInput),
    }
}

/// Creates a new timerfd object.
///
/// Returns a file descriptor that refers to the new timer.
pub fn sys_timerfd_create(clock_id: i32, flags: u32) -> AxResult<isize> {
    debug!("sys_timerfd_create <= clock_id: {clock_id}, flags: {flags:#x}");
    check_clock_id(clock_id)?;

    let nonblock = flags & TFD_NONBLOCK != 0;
    let cloexec = flags & TFD_CLOEXEC != 0;

    let timerfd = TimerFd::new(clock_id);
    timerfd.set_nonblocking(nonblock)?;
    add_file_like(timerfd as _, cloexec).map(|fd| fd as _)
}

/// Arms or disarms the timer referred to by the file descriptor `fd`.
pub fn sys_timerfd_settime(
    fd: i32,
    flags: u32,
    new_value: *const itimerspec,
    old_value: *mut itimerspec,
) -> AxResult<isize> {
    debug!("sys_timerfd_settime <= fd: {fd}, flags: {flags:#x}");

    let timerfd = TimerFd::from_fd(fd)?;

    let new = unsafe { new_value.vm_read_uninit()?.assume_init() };
    let new_value_tv = new.it_value.try_into_time_value()?;
    let new_interval_tv = new.it_interval.try_into_time_value()?;
    let abstime = flags & TFD_TIMER_ABSTIME != 0;

    let (old_val, old_int) = timerfd.settle(new_value_tv, new_interval_tv, abstime);

    if let Some(old_value) = old_value.nullable() {
        let old_itimerspec = itimerspec {
            it_interval: timespec::from_time_value(old_int),
            it_value: timespec::from_time_value(old_val),
        };
        old_value.vm_write(old_itimerspec)?;
    }

    Ok(0)
}

/// Returns the current setting of the timer referred to by the file descriptor `fd`.
pub fn sys_timerfd_gettime(fd: i32, curr_value: *mut itimerspec) -> AxResult<isize> {
    debug!("sys_timerfd_gettime <= fd: {fd}");

    let timerfd = TimerFd::from_fd(fd)?;
    let (value, interval) = timerfd.get_time();

    let curr = itimerspec {
        it_interval: timespec::from_time_value(interval),
        it_value: timespec::from_time_value(value),
    };
    curr_value.vm_write(curr)?;

    Ok(0)
}
