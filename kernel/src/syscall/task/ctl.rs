use core::ffi::c_char;

use axerrno::{AxError, AxResult, LinuxError};
use axtask::current;
use linux_raw_sys::general::{__user_cap_data_struct, __user_cap_header_struct};
use starry_vm::{VmMutPtr, VmPtr, vm_write_slice};

use crate::{
    mm::vm_load_string,
    task::{AsThread, get_process_data},
};

const CAPABILITY_VERSION_1: u32 = 0x19980330;
const CAPABILITY_VERSION_2: u32 = 0x20071026;
const CAPABILITY_VERSION_3: u32 = 0x20080522;
const CAP_CHOWN: u32 = 0;
const CAP_KILL: u32 = 5;
const CAP_SETPCAP: u32 = 8;
const CAP_NET_RAW: u32 = 13;
const CAP1: u32 = (1 << CAP_CHOWN) | (1 << CAP_NET_RAW) | (1 << CAP_SETPCAP);
const CAP_KILL_MASK: u32 = 1 << CAP_KILL;

fn cap_words(version: u32) -> Option<usize> {
    match version {
        CAPABILITY_VERSION_1 => Some(1),
        CAPABILITY_VERSION_2 | CAPABILITY_VERSION_3 => Some(2),
        _ => None,
    }
}

fn validate_cap_header(header_ptr: *mut __user_cap_header_struct) -> AxResult<(i32, usize)> {
    // FIXME: AnyBitPattern
    let mut header = unsafe { header_ptr.vm_read_uninit()?.assume_init() };
    let Some(words) = cap_words(header.version) else {
        header.version = CAPABILITY_VERSION_3;
        header_ptr.vm_write(header)?;
        return Err(AxError::InvalidInput);
    };

    if header.pid < 0 {
        return Err(AxError::InvalidInput);
    }

    if header.pid != 0 {
        let _ = get_process_data(header.pid as u32)?;
    }

    Ok((header.pid, words))
}

pub fn sys_capget(
    header: *mut __user_cap_header_struct,
    data: *mut __user_cap_data_struct,
) -> AxResult<isize> {
    let (_, words) = validate_cap_header(header)?;

    if data.is_null() {
        return Ok(0);
    }

    let empty = __user_cap_data_struct {
        effective: 0,
        permitted: 0,
        inheritable: 0,
    };
    data.vm_write(empty)?;
    if words == 2 {
        unsafe { data.add(1) }.vm_write(__user_cap_data_struct {
            effective: 0,
            permitted: 0,
            inheritable: 0,
        })?;
    }
    Ok(0)
}

pub fn sys_capset(
    header: *mut __user_cap_header_struct,
    data: *mut __user_cap_data_struct,
) -> AxResult<isize> {
    let (pid, _words) = validate_cap_header(header)?;
    let data = unsafe { data.vm_read_uninit()?.assume_init() };

    if pid != 0 && pid as u32 != current().as_thread().proc_data.proc.pid() {
        return Err(AxError::from(LinuxError::EPERM));
    }

    if data.effective & !data.permitted != 0 {
        return Err(AxError::from(LinuxError::EPERM));
    }
    if data.inheritable & !data.permitted != 0 {
        return Err(AxError::from(LinuxError::EPERM));
    }
    if !matches!(data.permitted, 0 | CAP1 | CAP_KILL_MASK) {
        return Err(AxError::from(LinuxError::EPERM));
    }

    Ok(0)
}

pub fn sys_umask(mask: u32) -> AxResult<isize> {
    let curr = current();
    let old = curr.as_thread().proc_data.replace_umask(mask);
    Ok(old as isize)
}

pub fn sys_setreuid(ruid: u32, euid: u32) -> AxResult<isize> {
    let curr = current();
    let proc_data = &curr.as_thread().proc_data;
    let uid = if euid != u32::MAX {
        euid
    } else if ruid != u32::MAX {
        ruid
    } else {
        proc_data.uid()
    };
    proc_data.set_uid(uid);
    Ok(0)
}

pub fn sys_setresuid(ruid: u32, euid: u32, suid: u32) -> AxResult<isize> {
    let curr = current();
    let proc_data = &curr.as_thread().proc_data;
    let uid = [euid, ruid, suid]
        .into_iter()
        .find(|uid| *uid != u32::MAX)
        .unwrap_or_else(|| proc_data.uid());
    proc_data.set_uid(uid);
    Ok(0)
}

pub fn sys_setresgid(rgid: u32, egid: u32, sgid: u32) -> AxResult<isize> {
    let curr = current();
    let proc_data = &curr.as_thread().proc_data;
    let gid = [egid, rgid, sgid]
        .into_iter()
        .find(|gid| *gid != u32::MAX)
        .unwrap_or_else(|| proc_data.gid());
    proc_data.set_gid(gid);
    Ok(0)
}

pub fn sys_setregid(rgid: u32, egid: u32) -> AxResult<isize> {
    let curr = current();
    let proc_data = &curr.as_thread().proc_data;
    let gid = if egid != u32::MAX {
        egid
    } else if rgid != u32::MAX {
        rgid
    } else {
        proc_data.gid()
    };
    proc_data.set_gid(gid);
    Ok(0)
}

pub fn sys_getresuid(ruid: *mut u32, euid: *mut u32, suid: *mut u32) -> AxResult<isize> {
    let uid = current().as_thread().proc_data.uid();
    ruid.vm_write(uid)?;
    euid.vm_write(uid)?;
    suid.vm_write(uid)?;
    Ok(0)
}

pub fn sys_getresgid(rgid: *mut u32, egid: *mut u32, sgid: *mut u32) -> AxResult<isize> {
    let gid = current().as_thread().proc_data.gid();
    rgid.vm_write(gid)?;
    egid.vm_write(gid)?;
    sgid.vm_write(gid)?;
    Ok(0)
}

pub fn sys_get_mempolicy(
    _policy: *mut i32,
    _nodemask: *mut usize,
    _maxnode: usize,
    _addr: usize,
    _flags: usize,
) -> AxResult<isize> {
    warn!("Dummy get_mempolicy called");
    Ok(0)
}

/// prctl() is called with a first argument describing what to do, and further
/// arguments with a significance depending on the first one.
/// The first argument can be:
/// - PR_SET_NAME: set the name of the calling thread, using the value pointed to by `arg2`
/// - PR_GET_NAME: get the name of the calling
/// - PR_SET_SECCOMP: enable seccomp mode, with the mode specified in `arg2`
/// - PR_MCE_KILL: set the machine check exception policy
/// - PR_SET_MM options: set various memory management options (start/end code/data/brk/stack)
pub fn sys_prctl(
    option: u32,
    arg2: usize,
    arg3: usize,
    arg4: usize,
    arg5: usize,
) -> AxResult<isize> {
    use linux_raw_sys::prctl::*;

    debug!("sys_prctl <= option: {option}, args: {arg2}, {arg3}, {arg4}, {arg5}");

    match option {
        PR_SET_NAME => {
            let s = vm_load_string(arg2 as *const c_char)?;
            current().set_name(&s);
        }
        PR_GET_NAME => {
            let name = current().name();
            let len = name.len().min(15);
            let mut buf = [0; 16];
            buf[..len].copy_from_slice(&name.as_bytes()[..len]);
            vm_write_slice(arg2 as _, &buf)?;
        }
        PR_SET_SECCOMP => {}
        PR_CAPBSET_DROP => {}
        PR_CAPBSET_READ => return Ok(1),
        PR_MCE_KILL => {}
        PR_SET_MM => {
            // not implemented; but avoid annoying warnings
            return Err(AxError::InvalidInput);
        }
        _ => return Err(AxError::InvalidInput),
    }

    Ok(0)
}
