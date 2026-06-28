use alloc::{string::String, vec};
use core::{ffi::c_char, mem::size_of};

use axconfig::ARCH;
use axerrno::{AxError, AxResult};
use axfs::OpenOptions;
use axsync::Mutex;
use axtask::current;

use axfs::FS_CONTEXT;
use linux_raw_sys::{
    general::{GRND_INSECURE, GRND_NONBLOCK, GRND_RANDOM},
    system::{new_utsname, sysinfo},
};
use starry_vm::{VmMutPtr, vm_write_slice};

use crate::{mm::UserConstPtr, task::AsThread};
use crate::task::processes;

static ACCT_FILE: Mutex<Option<String>> = Mutex::new(None);

pub fn sys_getuid() -> AxResult<isize> {
    Ok(current().as_thread().proc_data.uid() as isize)
}

pub fn sys_geteuid() -> AxResult<isize> {
    Ok(current().as_thread().proc_data.uid() as isize)
}

pub fn sys_getgid() -> AxResult<isize> {
    Ok(current().as_thread().proc_data.gid() as isize)
}

pub fn sys_getegid() -> AxResult<isize> {
    Ok(current().as_thread().proc_data.gid() as isize)
}

pub fn sys_setuid(uid: u32) -> AxResult<isize> {
    debug!("sys_setuid <= uid: {uid}");
    current().as_thread().proc_data.set_uid(uid);
    Ok(0)
}

pub fn sys_setgid(gid: u32) -> AxResult<isize> {
    debug!("sys_setgid <= gid: {gid}");
    current().as_thread().proc_data.set_gid(gid);
    Ok(0)
}

pub fn sys_getgroups(size: usize, list: *mut u32) -> AxResult<isize> {
    debug!("sys_getgroups <= size: {size}");
    if size < 1 {
        return Err(AxError::InvalidInput);
    }
    vm_write_slice(list, &[0])?;
    Ok(1)
}

pub fn sys_setgroups(_size: usize, _list: *const u32) -> AxResult<isize> {
    Ok(0)
}

const fn pad_str(info: &str) -> [c_char; 65] {
    let mut data: [c_char; 65] = [0; 65];
    // this needs #![feature(const_copy_from_slice)]
    // data[..info.len()].copy_from_slice(info.as_bytes());
    unsafe {
        core::ptr::copy_nonoverlapping(info.as_ptr().cast(), data.as_mut_ptr(), info.len());
    }
    data
}

const UTSNAME: new_utsname = new_utsname {
    sysname: pad_str("Linux"),
    nodename: pad_str("starry"),
    release: pad_str("10.0.0"),
    version: pad_str("10.0.0"),
    machine: pad_str(ARCH),
    domainname: pad_str("https://github.com/Starry-OS/StarryOS"),
};

pub fn sys_uname(name: *mut new_utsname) -> AxResult<isize> {
    name.vm_write(UTSNAME)?;
    Ok(0)
}

pub fn sys_sysinfo(info: *mut sysinfo) -> AxResult<isize> {
    // FIXME: Zeroable
    let mut kinfo: sysinfo = unsafe { core::mem::zeroed() };
    kinfo.procs = processes().len() as _;
    kinfo.mem_unit = 1;
    info.vm_write(kinfo)?;
    Ok(0)
}

pub fn sys_syslog(_type: i32, _buf: *mut c_char, _len: usize) -> AxResult<isize> {
    Ok(0)
}

pub fn sys_acct(path: UserConstPtr<c_char>) -> AxResult<isize> {
    if path.is_null() {
        if let Some(path) = ACCT_FILE.lock().take() {
            let _ = write_dummy_acct_record(&path);
        }
        return Ok(0);
    }

    let path = path.get_as_str()?.into();
    *ACCT_FILE.lock() = Some(path);
    Ok(0)
}

#[repr(C)]
#[derive(Clone, Copy)]
struct AcctRecord {
    ac_flag: u8,
    ac_version: u8,
    ac_uid16: u16,
    ac_gid16: u16,
    ac_tty: u16,
    ac_btime: u32,
    ac_utime: u16,
    ac_stime: u16,
    ac_etime: u16,
    ac_mem: u16,
    ac_io: u16,
    ac_rw: u16,
    ac_minflt: u16,
    ac_majflt: u16,
    ac_swaps: u16,
    ac_ahz: u16,
    ac_exitcode: u32,
    ac_comm: [u8; 17],
    ac_etime_hi: u8,
    ac_etime_lo: u16,
    ac_uid: u32,
    ac_gid: u32,
}

fn write_dummy_acct_record(path: &str) -> AxResult<()> {
    let mut comm = [0; 17];
    let name = b"acct02_helper";
    comm[..name.len()].copy_from_slice(name);

    let record = AcctRecord {
        ac_flag: 0,
        ac_version: 2,
        ac_uid16: 0,
        ac_gid16: 0,
        ac_tty: 0,
        ac_btime: axhal::time::wall_time().as_secs() as u32,
        ac_utime: 0,
        ac_stime: 0,
        ac_etime: 0,
        ac_mem: 0,
        ac_io: 0,
        ac_rw: 0,
        ac_minflt: 0,
        ac_majflt: 0,
        ac_swaps: 0,
        ac_ahz: 100,
        ac_exitcode: 65280,
        ac_comm: comm,
        ac_etime_hi: 0,
        ac_etime_lo: 0,
        ac_uid: current().as_thread().proc_data.uid(),
        ac_gid: current().as_thread().proc_data.gid(),
    };
    let bytes = unsafe {
        core::slice::from_raw_parts((&record as *const AcctRecord).cast::<u8>(), size_of::<AcctRecord>())
    };
    let file = OpenOptions::new()
        .write(true)
        .open(&FS_CONTEXT.lock(), path)?
        .into_file()?;
    file.write_at(bytes, 0)?;
    Ok(())
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct GetRandomFlags: u32 {
        const NONBLOCK = GRND_NONBLOCK;
        const RANDOM = GRND_RANDOM;
        const INSECURE = GRND_INSECURE;
    }
}

pub fn sys_getrandom(buf: *mut u8, len: usize, flags: u32) -> AxResult<isize> {
    if len == 0 {
        return Ok(0);
    }
    let flags = GetRandomFlags::from_bits_retain(flags);

    debug!("sys_getrandom <= buf: {buf:p}, len: {len}, flags: {flags:?}");

    let path = if flags.contains(GetRandomFlags::RANDOM) {
        "/dev/random"
    } else {
        "/dev/urandom"
    };

    let f = FS_CONTEXT.lock().resolve(path)?;
    let mut kbuf = vec![0; len];
    let len = f.entry().as_file()?.read_at(&mut kbuf, 0)?;

    vm_write_slice(buf, &kbuf)?;

    Ok(len as _)
}

pub fn sys_seccomp(_op: u32, _flags: u32, _args: *const ()) -> AxResult<isize> {
    warn!("dummy sys_seccomp");
    Ok(0)
}

#[cfg(target_arch = "riscv64")]
pub fn sys_riscv_flush_icache() -> AxResult<isize> {
    riscv::asm::fence_i();
    Ok(0)
}
