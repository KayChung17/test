//! ArceOS filesystem module.
//!
//! Provides high-level filesystem operations built on top of the VFS layer,
//! including file I/O with page caching, directory traversal, and
//! `std::fs`-like APIs.

#![cfg_attr(all(not(test), not(doc)), no_std)]
#![feature(doc_cfg)]
#![allow(clippy::new_ret_no_self)]

extern crate alloc;

#[macro_use]
extern crate log;

use alloc::{string::String, vec::Vec};
use axdriver::{AxBlockDevice, AxDeviceContainer, prelude::*};
use axfs_ng_vfs::Filesystem;
use axsync::Mutex;
use spin::Once;

pub mod fs;

mod highlevel;
pub use highlevel::*;

/// Extra filesystems built from non-root block devices, available for later mounting.
static EXTRA_FILESYSTEMS: Once<Mutex<Vec<Filesystem>>> = Once::new();

/// Initializes the filesystem subsystem using the last block device as root.
///
/// In a multi-disk setup, the last device (largest index) is used as root (`/`),
/// and earlier devices are reserved for userspace mounting.
pub fn init_filesystems(mut block_devs: AxDeviceContainer<AxBlockDevice>) {
    info!("Initialize filesystem subsystem...");

    let total = block_devs.len();
    info!("  found {} block device(s)", total);
    for (i, dev) in block_devs.iter().enumerate() {
        info!("    [{}] {:?}", i, dev.device_name());
    }

    let dev = block_devs.pop().expect("No block device found!");
    let root_idx = total.saturating_sub(1);
    info!("  use block device [{}] as root: {:?}", root_idx, dev.device_name());

    if !block_devs.is_empty() {
        info!("  reserved {} extra block device(s):", block_devs.len());
        for (i, dev) in block_devs.iter().enumerate() {
            info!("    [{}] {:?}", i, dev.device_name());
        }
    }

    let fs = fs::new_default(dev).expect("Failed to initialize filesystem");
    info!("  filesystem type: {:?}", fs.name());

    let mp = axfs_ng_vfs::Mountpoint::new_root(&fs);
    ROOT_FS_CONTEXT.call_once(|| FsContext::new(mp.root_location()));

    // Build filesystems for remaining devices so boot-time mounts and
    // later mount(2) calls can reuse them.
    let extra: Vec<Filesystem> = block_devs
        .drain(..)
        .map(|dev| fs::new_default(dev).expect("Failed to initialize extra filesystem"))
        .collect();
    if !extra.is_empty() {
        EXTRA_FILESYSTEMS.call_once(|| Mutex::new(extra));
    }
}

/// Returns clones of all extra filesystems (those not used as root).
pub fn extra_filesystems() -> Vec<Filesystem> {
    EXTRA_FILESYSTEMS
        .get()
        .and_then(|m| m.try_lock())
        .map(|v| v.iter().cloned().collect())
        .unwrap_or_default()
}

/// Looks up an extra filesystem by a Linux-style block source path.
pub fn lookup_extra_filesystem(source: &str) -> Option<Filesystem> {
    let name = source.strip_prefix("/dev/")?;
    let (disk, _part) = split_linux_disk_name(name)?;
    let idx = linux_disk_index(disk)?;
    EXTRA_FILESYSTEMS
        .get()
        .and_then(|m| m.try_lock())
        .and_then(|v| v.get(idx).cloned())
}

fn split_linux_disk_name(name: &str) -> Option<(&str, Option<&str>)> {
    let split = name.find(|c: char| c.is_ascii_digit()).unwrap_or(name.len());
    let (disk, part) = name.split_at(split);
    if disk.is_empty() {
        return None;
    }
    Some((disk, (!part.is_empty()).then_some(part)))
}

fn linux_disk_index(name: &str) -> Option<usize> {
    let suffix = name.strip_prefix("vd")?;
    if suffix.is_empty() {
        return None;
    }
    let mut idx = 0usize;
    for ch in suffix.chars() {
        if !ch.is_ascii_lowercase() {
            return None;
        }
        idx = idx * 26 + (ch as usize - 'a' as usize + 1);
    }
    idx.checked_sub(1)
}
