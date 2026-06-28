use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use core::ffi::{c_char, c_void};

use axerrno::{AxError, AxResult};
use axfs::FS_CONTEXT;
use axsync::Mutex;
use linux_raw_sys::general::MS_RDONLY;

use crate::{mm::vm_load_string, pseudofs::MemoryFs};

static READONLY_MOUNTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn remember_readonly_mount(path: &str, flags: u32) {
    if flags & MS_RDONLY == 0 {
        return;
    }
    let mut mounts = READONLY_MOUNTS.lock();
    if mounts.iter().all(|it| it != path) {
        mounts.push(path.to_string());
    }
}

fn forget_readonly_mount(path: &str) {
    READONLY_MOUNTS.lock().retain(|it| it != path);
}

pub fn is_path_on_readonly_mount(path: &str) -> bool {
    READONLY_MOUNTS.lock().iter().any(|mount| {
        path == mount
            || path
                .strip_prefix(mount.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

pub fn sys_mount(
    source: *const c_char,
    target: *const c_char,
    fs_type: *const c_char,
    flags: i32,
    _data: *const c_void,
) -> AxResult<isize> {
    let source = if source.is_null() {
        None
    } else {
        Some(vm_load_string(source)?)
    };
    let target = vm_load_string(target)?;
    let fs_type = vm_load_string(fs_type)?;
    debug!("sys_mount <= source: {source:?}, target: {target:?}, fs_type: {fs_type:?}, flags: {flags}");

    let target = FS_CONTEXT.lock().resolve(target)?;
    let target_path = target.absolute_path()?;
    let mount_flags = flags as u32;

    if matches!(fs_type.as_str(), "tmpfs" | "cgroup" | "cgroup2") {
        let fs = MemoryFs::new();
        target.mount(&fs)?;
        remember_readonly_mount(target_path.as_str(), mount_flags);
        return Ok(0);
    }

    if let Some(source) = source.as_deref()
        && source.starts_with("/dev/")
    {
        let fs = axfs::lookup_extra_filesystem(&source).ok_or(AxError::NoSuchDevice)?;
        target.mount(&fs)?;
        remember_readonly_mount(target_path.as_str(), mount_flags);
        return Ok(0);
    }

    Err(AxError::NoSuchDevice)
}

pub fn sys_umount2(target: *const c_char, _flags: i32) -> AxResult<isize> {
    let target = vm_load_string(target)?;
    debug!("sys_umount2 <= target: {target:?}");
    let target = FS_CONTEXT.lock().resolve(target)?;
    let target_path = target.absolute_path()?;
    target.unmount()?;
    forget_readonly_mount(target_path.as_str());
    Ok(0)
}
