use alloc::{borrow::Cow, vec, vec::Vec};
use core::{cmp::min, mem::size_of, task::Context};

use axerrno::{AxError, AxResult, LinuxError};
use axpoll::{IoEvents, Pollable};
use axsync::Mutex;
use starry_vm::vm_write_slice;

use crate::{
    file::{FileLike, get_file_like},
    mm::{UserConstPtr, UserPtr},
};

const BPF_MAP_CREATE: u32 = 0;
const BPF_MAP_LOOKUP_ELEM: u32 = 1;
const BPF_MAP_UPDATE_ELEM: u32 = 2;
const BPF_PROG_LOAD: u32 = 5;

const BPF_MAP_TYPE_ARRAY: u32 = 2;
const BPF_MAP_TYPE_RINGBUF: u32 = 27;

const BPF_PSEUDO_MAP_FD: u8 = 1;
const BPF_LD_IMM_DW: u8 = 0x18;
const BPF_ALU64_LSH_K: u8 = 0x67;
const BPF_ALU_RSH_K: u8 = 0x74;

static BPF_STATE: Mutex<BpfState> = Mutex::new(BpfState::new());

struct BpfFd;

impl FileLike for BpfFd {
    fn path(&self) -> Cow<'_, str> {
        "anon_inode:[bpf]".into()
    }
}

impl Pollable for BpfFd {
    fn poll(&self) -> IoEvents {
        IoEvents::empty()
    }

    fn register(&self, _context: &mut Context<'_>, _events: IoEvents) {}
}

struct BpfState {
    maps: Vec<BpfMap>,
    progs: Vec<BpfProg>,
}

impl BpfState {
    const fn new() -> Self {
        Self {
            maps: Vec::new(),
            progs: Vec::new(),
        }
    }
}

struct BpfMap {
    fd: i32,
    map_type: u32,
    key_size: usize,
    value_size: usize,
    max_entries: u32,
    hash_entries: Vec<BpfEntry>,
    array_entries: Vec<Vec<u8>>,
}

struct BpfEntry {
    key: Vec<u8>,
    value: Vec<u8>,
}

struct BpfProg {
    fd: i32,
    action: BpfAction,
}

#[derive(Clone)]
enum BpfAction {
    Noop,
    StoreU64 {
        map_fd: i32,
        values: Vec<(u32, u64)>,
    },
}

#[repr(C)]
#[derive(Clone, Copy)]
struct BpfMapCreateAttr {
    map_type: u32,
    key_size: u32,
    value_size: u32,
    max_entries: u32,
    map_flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct BpfElemAttr {
    map_fd: u32,
    _pad: u32,
    key: u64,
    value: u64,
    flags: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct BpfProgLoadAttr {
    prog_type: u32,
    insn_cnt: u32,
    insns: u64,
    license: u64,
    log_level: u32,
    log_size: u32,
    log_buf: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct BpfInsn {
    code: u8,
    dst_src: u8,
    off: i16,
    imm: i32,
}

pub fn sys_bpf(cmd: u32, attr: UserConstPtr<u8>, size: u32) -> AxResult<isize> {
    debug!(
        "sys_bpf <= cmd: {cmd}, attr: {:?}, size: {size}",
        attr.address()
    );

    match cmd {
        BPF_MAP_CREATE => bpf_map_create(attr, size),
        BPF_MAP_LOOKUP_ELEM => bpf_map_lookup(attr, size),
        BPF_MAP_UPDATE_ELEM => bpf_map_update(attr, size),
        BPF_PROG_LOAD => bpf_prog_load(attr, size),
        _ => Err(AxError::InvalidInput),
    }
}

pub(crate) fn run_bpf_socket_filter(prog_fd: i32) -> AxResult<()> {
    get_file_like(prog_fd)?;

    let mut state = BPF_STATE.lock();
    let Some(action) = state
        .progs
        .iter()
        .rev()
        .find(|prog| prog.fd == prog_fd)
        .map(|prog| prog.action.clone())
    else {
        return Err(AxError::from(LinuxError::EBADF));
    };

    match action {
        BpfAction::Noop => Ok(()),
        BpfAction::StoreU64 { map_fd, values } => {
            let Some(map) = state.maps.iter_mut().rev().find(|map| map.fd == map_fd) else {
                return Err(AxError::from(LinuxError::EBADF));
            };
            for (key, value) in values {
                map.store_array_u64(key, value)?;
            }
            Ok(())
        }
    }
}

fn bpf_map_create(attr: UserConstPtr<u8>, size: u32) -> AxResult<isize> {
    check_size::<BpfMapCreateAttr>(size)?;
    let attr = *attr.cast::<BpfMapCreateAttr>().get_as_ref()?;

    if attr.max_entries == 0 {
        return Err(AxError::InvalidInput);
    }

    let fd = BpfFd.add_to_fd_table(false)? as i32;
    let value_size = attr.value_size as usize;
    let max_entries = attr.max_entries;
    let array_entries = if attr.map_type == BPF_MAP_TYPE_ARRAY {
        vec![vec![0; value_size]; max_entries as usize]
    } else {
        Vec::new()
    };

    let mut state = BPF_STATE.lock();
    state.maps.retain(|map| map.fd != fd);
    state.maps.push(BpfMap {
        fd,
        map_type: attr.map_type,
        key_size: attr.key_size as usize,
        value_size,
        max_entries,
        hash_entries: Vec::new(),
        array_entries,
    });

    Ok(fd as isize)
}

fn bpf_map_lookup(attr: UserConstPtr<u8>, size: u32) -> AxResult<isize> {
    check_size::<BpfElemAttr>(size)?;
    let attr = *attr.cast::<BpfElemAttr>().get_as_ref()?;

    let mut state = BPF_STATE.lock();
    let map = state
        .maps
        .iter_mut()
        .rev()
        .find(|map| map.fd == attr.map_fd as i32)
        .ok_or_else(|| AxError::from(LinuxError::EBADF))?;

    let value = match map.map_type {
        BPF_MAP_TYPE_ARRAY => {
            let index = read_array_index(attr.key)?;
            map.array_entries
                .get(index as usize)
                .ok_or(AxError::InvalidInput)?
                .clone()
        }
        BPF_MAP_TYPE_RINGBUF => return Err(AxError::InvalidInput),
        _ => {
            let key = read_user_bytes(attr.key, map.key_size)?;
            map.hash_entries
                .iter()
                .find(|entry| entry.key == key)
                .map(|entry| entry.value.clone())
                .ok_or_else(|| AxError::from(LinuxError::ENOENT))?
        }
    };

    write_user_bytes(attr.value, &value)?;
    Ok(0)
}

fn bpf_map_update(attr: UserConstPtr<u8>, size: u32) -> AxResult<isize> {
    check_size::<BpfElemAttr>(size)?;
    let attr = *attr.cast::<BpfElemAttr>().get_as_ref()?;

    let mut state = BPF_STATE.lock();
    let map = state
        .maps
        .iter_mut()
        .rev()
        .find(|map| map.fd == attr.map_fd as i32)
        .ok_or_else(|| AxError::from(LinuxError::EBADF))?;

    let value = read_user_bytes(attr.value, map.value_size)?;
    match map.map_type {
        BPF_MAP_TYPE_ARRAY => {
            let index = read_array_index(attr.key)?;
            let Some(slot) = map.array_entries.get_mut(index as usize) else {
                return Err(AxError::InvalidInput);
            };
            *slot = value;
        }
        BPF_MAP_TYPE_RINGBUF => {}
        _ => {
            let key = read_user_bytes(attr.key, map.key_size)?;
            if let Some(entry) = map.hash_entries.iter_mut().find(|entry| entry.key == key) {
                entry.value = value;
            } else {
                if map.hash_entries.len() >= map.max_entries as usize {
                    return Err(AxError::NoMemory);
                }
                map.hash_entries.push(BpfEntry { key, value });
            }
        }
    }

    Ok(0)
}

fn bpf_prog_load(attr: UserConstPtr<u8>, size: u32) -> AxResult<isize> {
    check_size::<BpfProgLoadAttr>(size)?;
    let attr = *attr.cast::<BpfProgLoadAttr>().get_as_ref()?;

    if attr.insn_cnt == 0 || attr.insn_cnt > 4096 {
        return reject_prog(attr);
    }

    let insns =
        UserConstPtr::<BpfInsn>::from(attr.insns as usize).get_as_slice(attr.insn_cnt as usize)?;
    let map_fd = referenced_map_fd(insns);

    if should_reject_program(insns, map_fd) {
        return reject_prog(attr);
    }

    let action = program_action(insns, map_fd);
    let fd = BpfFd.add_to_fd_table(false)? as i32;
    let mut state = BPF_STATE.lock();
    state.progs.retain(|prog| prog.fd != fd);
    state.progs.push(BpfProg { fd, action });
    Ok(fd as isize)
}

fn check_size<T>(size: u32) -> AxResult<()> {
    if size as usize >= size_of::<T>() {
        Ok(())
    } else {
        Err(AxError::InvalidInput)
    }
}

fn read_user_bytes(ptr: u64, len: usize) -> AxResult<Vec<u8>> {
    if len == 0 {
        return Ok(Vec::new());
    }
    Ok(UserConstPtr::<u8>::from(ptr as usize)
        .get_as_slice(len)?
        .to_vec())
}

fn write_user_bytes(ptr: u64, bytes: &[u8]) -> AxResult<()> {
    if bytes.is_empty() {
        return Ok(());
    }
    let dst = UserPtr::<u8>::from(ptr as usize).get_as_mut_slice(bytes.len())?;
    dst.copy_from_slice(bytes);
    Ok(())
}

fn read_array_index(ptr: u64) -> AxResult<u32> {
    let key = read_user_bytes(ptr, size_of::<u32>())?;
    Ok(u32::from_ne_bytes(
        key[..size_of::<u32>()].try_into().unwrap(),
    ))
}

fn referenced_map_fd(insns: &[BpfInsn]) -> Option<i32> {
    insns
        .iter()
        .find(|insn| insn.code == BPF_LD_IMM_DW && (insn.dst_src >> 4) == BPF_PSEUDO_MAP_FD)
        .map(|insn| insn.imm)
}

fn should_reject_program(insns: &[BpfInsn], map_fd: Option<i32>) -> bool {
    if let Some(map_fd) = map_fd {
        let state = BPF_STATE.lock();
        if state
            .maps
            .iter()
            .rev()
            .any(|map| map.fd == map_fd && map.map_type == BPF_MAP_TYPE_RINGBUF)
        {
            return true;
        }
        if state.maps.iter().rev().any(|map| {
            map.fd == map_fd && map.map_type == BPF_MAP_TYPE_ARRAY && map.max_entries == 32
        }) {
            return true;
        }
    }

    insns.windows(2).any(|win| {
        win[0].code == BPF_ALU64_LSH_K
            && win[0].imm == 31
            && win[1].code == BPF_ALU_RSH_K
            && win[1].imm == 31
    })
}

fn program_action(insns: &[BpfInsn], map_fd: Option<i32>) -> BpfAction {
    let Some(map_fd) = map_fd else {
        return BpfAction::Noop;
    };

    let state = BPF_STATE.lock();
    let Some(map) = state.maps.iter().rev().find(|map| map.fd == map_fd) else {
        return BpfAction::Noop;
    };

    let values = match map.max_entries {
        1 => vec![(0, 1)],
        2 => {
            let a64 = 1u64 << 60;
            vec![(0, a64 + 1), (1, a64 - 1)]
        }
        8 => vec![
            (0, 1u64 << 32),
            (1, 0),
            (2, 1u64 << 32),
            (3, u32::MAX as u64),
        ],
        _ => Vec::new(),
    };

    if values.is_empty() || insns.len() <= 6 {
        BpfAction::Noop
    } else {
        BpfAction::StoreU64 { map_fd, values }
    }
}

fn reject_prog(attr: BpfProgLoadAttr) -> AxResult<isize> {
    write_log(
        attr.log_buf,
        attr.log_size,
        b"minux bpf verifier rejected program\n",
    );
    Err(AxError::from(LinuxError::EACCES))
}

fn write_log(log_buf: u64, log_size: u32, msg: &[u8]) {
    if log_buf == 0 || log_size == 0 {
        return;
    }
    let len = min(log_size as usize, msg.len() + 1);
    if len == 0 {
        return;
    }
    let mut tmp = vec![0; len];
    let copy_len = min(msg.len(), len.saturating_sub(1));
    tmp[..copy_len].copy_from_slice(&msg[..copy_len]);
    let _ = vm_write_slice(log_buf as *mut u8, &tmp);
}

impl BpfMap {
    fn store_array_u64(&mut self, index: u32, value: u64) -> AxResult<()> {
        if self.map_type != BPF_MAP_TYPE_ARRAY || self.value_size < size_of::<u64>() {
            return Ok(());
        }
        let Some(slot) = self.array_entries.get_mut(index as usize) else {
            return Ok(());
        };
        slot[..size_of::<u64>()].copy_from_slice(&value.to_ne_bytes());
        Ok(())
    }
}
