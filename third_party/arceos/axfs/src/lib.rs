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

use alloc::vec::Vec;
use axdriver::{AxBlockDevice, AxDeviceContainer, prelude::*};
use axsync::Mutex;
use spin::Once;

pub mod fs;

mod highlevel;
pub use highlevel::*;

/// Extra block devices stored during init, available for later mounting.
static EXTRA_BLOCK_DEVS: Once<Mutex<Vec<AxBlockDevice>>> = Once::new();

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

    // Store remaining devices for later mounting
    let extra: Vec<AxBlockDevice> = block_devs.drain(..).collect();
    if !extra.is_empty() {
        EXTRA_BLOCK_DEVS.call_once(|| Mutex::new(extra));
    }
}

/// Takes the extra block devices (those not used as root) for mounting.
pub fn take_extra_block_devs() -> Vec<AxBlockDevice> {
    EXTRA_BLOCK_DEVS
        .get()
        .and_then(|m| m.try_lock())
        .map(|mut v| v.drain(..).collect())
        .unwrap_or_default()
}
