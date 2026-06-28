use axerrno::{AxError, AxResult, LinuxError};
use axhal::time::{TimeValue, monotonic_time, monotonic_time_nanos, nanos_to_ticks, wall_time};
use axtask::current;
use linux_raw_sys::general::{
    __kernel_clockid_t, CLOCK_BOOTTIME, CLOCK_MONOTONIC, CLOCK_MONOTONIC_COARSE,
    CLOCK_MONOTONIC_RAW, CLOCK_PROCESS_CPUTIME_ID, CLOCK_REALTIME, CLOCK_REALTIME_COARSE,
    CLOCK_THREAD_CPUTIME_ID, itimerval, timespec, timeval,
};
use starry_vm::{VmMutPtr, VmPtr};

use crate::{
    task::{AsThread, ITimerType},
    time::TimeValueLike,
};

const ADJ_OFFSET: u32 = 0x0001;
const ADJ_FREQUENCY: u32 = 0x0002;
const ADJ_MAXERROR: u32 = 0x0004;
const ADJ_ESTERROR: u32 = 0x0008;
const ADJ_STATUS: u32 = 0x0010;
const ADJ_TIMECONST: u32 = 0x0020;
const ADJ_TICK: u32 = 0x4000;
const ADJ_OFFSET_SINGLESHOT: u32 = 0x8001;
const ADJ_OFFSET_SS_READ: u32 = 0xa001;
const ADJ_VALID_MODE_BITS: u32 = ADJ_OFFSET
    | ADJ_FREQUENCY
    | ADJ_MAXERROR
    | ADJ_ESTERROR
    | ADJ_STATUS
    | ADJ_TIMECONST
    | ADJ_TICK;
const ADJ_MUTATING_MODES: u32 = ADJ_VALID_MODE_BITS | ADJ_OFFSET_SINGLESHOT;
const MIN_TICK: isize = 9_000;
const MAX_TICK: isize = 11_000;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Timex {
    modes: u32,
    offset: isize,
    freq: isize,
    maxerror: isize,
    esterror: isize,
    status: i32,
    constant: isize,
    precision: isize,
    tolerance: isize,
    time: timeval,
    tick: isize,
    ppsfreq: isize,
    jitter: isize,
    shift: i32,
    stabil: isize,
    jitcnt: isize,
    calcnt: isize,
    errcnt: isize,
    stbcnt: isize,
    tai: i32,
    padding: [i32; 11],
}

impl Timex {
    fn zeroed() -> Self {
        // SAFETY: `Timex` is a plain C-compatible data structure made of integers
        // and `timeval`; all-zero is a valid value for the compatibility fields.
        unsafe { core::mem::zeroed() }
    }
}

fn read_timex(buf: *mut Timex) -> AxResult<Timex> {
    let value = unsafe { buf.vm_read_uninit()?.assume_init() };
    Ok(value)
}

fn valid_adjtimex_modes(modes: u32) -> bool {
    modes & !ADJ_VALID_MODE_BITS == 0
        || modes == ADJ_OFFSET_SINGLESHOT
        || modes == ADJ_OFFSET_SS_READ
}

pub fn sys_clock_adjtime(clock_id: __kernel_clockid_t, buf: *mut Timex) -> AxResult<isize> {
    if clock_id as u32 != CLOCK_REALTIME {
        return Err(AxError::InvalidInput);
    }

    let mut timex = read_timex(buf)?;
    if !valid_adjtimex_modes(timex.modes) {
        return Err(AxError::InvalidInput);
    }

    if timex.modes & ADJ_TICK != 0 && (timex.tick < MIN_TICK || timex.tick > MAX_TICK) {
        return Err(AxError::InvalidInput);
    }

    if timex.modes & ADJ_MUTATING_MODES != 0 && current().as_thread().proc_data.uid() != 0 {
        return Err(AxError::from(LinuxError::EPERM));
    }

    timex = Timex {
        modes: timex.modes,
        time: timeval::from_time_value(wall_time()),
        tick: 10_000,
        ..Timex::zeroed()
    };
    buf.vm_write(timex)?;
    Ok(0)
}

pub fn sys_adjtimex(buf: *mut Timex) -> AxResult<isize> {
    sys_clock_adjtime(CLOCK_REALTIME as _, buf)
}

pub fn sys_clock_gettime(clock_id: __kernel_clockid_t, ts: *mut timespec) -> AxResult<isize> {
    let now = match clock_id as u32 {
        CLOCK_REALTIME | CLOCK_REALTIME_COARSE => wall_time(),
        CLOCK_MONOTONIC | CLOCK_MONOTONIC_RAW | CLOCK_MONOTONIC_COARSE | CLOCK_BOOTTIME => {
            monotonic_time()
        }
        CLOCK_PROCESS_CPUTIME_ID | CLOCK_THREAD_CPUTIME_ID => {
            let (utime, stime) = current().as_thread().time.borrow().output();
            utime + stime
        }
        _ => {
            warn!("Called sys_clock_gettime for unsupported clock {clock_id}");
            return Err(AxError::InvalidInput);
        }
    };
    ts.vm_write(timespec::from_time_value(now))?;
    Ok(0)
}

pub fn sys_gettimeofday(ts: *mut timeval) -> AxResult<isize> {
    ts.vm_write(timeval::from_time_value(wall_time()))?;
    Ok(0)
}

pub fn sys_clock_getres(clock_id: __kernel_clockid_t, res: *mut timespec) -> AxResult<isize> {
    match clock_id as u32 {
        CLOCK_REALTIME
        | CLOCK_REALTIME_COARSE
        | CLOCK_MONOTONIC
        | CLOCK_MONOTONIC_RAW
        | CLOCK_MONOTONIC_COARSE
        | CLOCK_BOOTTIME
        | CLOCK_PROCESS_CPUTIME_ID
        | CLOCK_THREAD_CPUTIME_ID => {}
        _ => {
            warn!("Called sys_clock_getres for unsupported clock {clock_id}");
            return Err(AxError::InvalidInput);
        }
    }
    if let Some(res) = res.nullable() {
        res.vm_write(timespec::from_time_value(TimeValue::from_micros(1)))?;
    }
    Ok(0)
}

#[repr(C)]
pub struct Tms {
    /// user time
    tms_utime: usize,
    /// system time
    tms_stime: usize,
    /// user time of children
    tms_cutime: usize,
    /// system time of children
    tms_cstime: usize,
}

pub fn sys_times(tms: *mut Tms) -> AxResult<isize> {
    let (utime, stime) = current().as_thread().time.borrow().output();
    let utime = utime.as_micros() as usize;
    let stime = stime.as_micros() as usize;
    tms.vm_write(Tms {
        tms_utime: utime,
        tms_stime: stime,
        tms_cutime: utime,
        tms_cstime: stime,
    })?;
    Ok(nanos_to_ticks(monotonic_time_nanos()) as _)
}

pub fn sys_getitimer(which: i32, value: *mut itimerval) -> AxResult<isize> {
    let ty = ITimerType::from_repr(which).ok_or(AxError::InvalidInput)?;
    let (it_interval, it_value) = current().as_thread().time.borrow().get_itimer(ty);

    value.vm_write(itimerval {
        it_interval: timeval::from_time_value(it_interval),
        it_value: timeval::from_time_value(it_value),
    })?;
    Ok(0)
}

pub fn sys_setitimer(
    which: i32,
    new_value: *const itimerval,
    old_value: *mut itimerval,
) -> AxResult<isize> {
    let ty = ITimerType::from_repr(which).ok_or(AxError::InvalidInput)?;
    let curr = current();

    let (interval, remained) = match new_value.nullable() {
        Some(new_value) => {
            // FIXME: AnyBitPattern
            let new_value = unsafe { new_value.vm_read_uninit()?.assume_init() };
            (
                new_value.it_interval.try_into_time_value()?.as_nanos() as usize,
                new_value.it_value.try_into_time_value()?.as_nanos() as usize,
            )
        }
        None => (0, 0),
    };

    debug!("sys_setitimer <= type: {ty:?}, interval: {interval:?}, remained: {remained:?}");

    let old = curr
        .as_thread()
        .time
        .try_borrow_mut()
        .map_err(|_| AxError::WouldBlock)?
        .set_itimer(ty, interval, remained);

    if let Some(old_value) = old_value.nullable() {
        old_value.vm_write(itimerval {
            it_interval: timeval::from_time_value(old.0),
            it_value: timeval::from_time_value(old.1),
        })?;
    }
    Ok(0)
}
