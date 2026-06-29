use alloc::{collections::BTreeMap, format, string::ToString, sync::Arc, vec::Vec};
use core::{
    ffi::{c_char, c_int},
    mem,
    ops::{Deref, DerefMut},
};

use axerrno::{AxError, AxResult, LinuxError};
use axfs::{FS_CONTEXT, FileBackend, OpenOptions, OpenResult};
use axfs_ng_vfs::{DirEntry, FileNode, Location, NodePermission, NodeType, Reference};
use axtask::current;
use bitflags::bitflags;
use linux_raw_sys::general::*;

use crate::{
    file::{
        Directory, FD_TABLE, File, FileDescriptor, FileLike, Pipe, add_file_like, close_file_like,
        get_file_like, with_fs,
    },
    task::AX_FILE_LIMIT,
    mm::{UserConstPtr, UserPtr, vm_load_string},
    pseudofs::{Device, dev::tty},
    syscall::sys::{sys_getegid, sys_geteuid},
    task::AsThread,
};

/// Convert open flags to [`OpenOptions`].
fn flags_to_options(flags: c_int, mode: __kernel_mode_t, (uid, gid): (u32, u32)) -> OpenOptions {
    let flags = flags as u32;
    let mut options = OpenOptions::new();
    options.mode(mode).user(uid, gid);
    match flags & 0b11 {
        O_RDONLY => options.read(true),
        O_WRONLY => options.write(true),
        _ => options.read(true).write(true),
    };
    if flags & O_APPEND != 0 {
        options.append(true);
    }
    if flags & O_TRUNC != 0 {
        options.truncate(true);
    }
    if flags & O_CREAT != 0 {
        options.create(true);
    }
    if flags & O_PATH != 0 {
        options.path(true);
    }
    if flags & O_EXCL != 0 {
        options.create_new(true);
    }
    if flags & O_DIRECTORY != 0 {
        options.directory(true);
    }
    if flags & O_NOFOLLOW != 0 {
        options.no_follow(true);
    }
    if flags & O_DIRECT != 0 {
        options.direct(true);
    }
    options
}

fn add_to_fd(result: OpenResult, flags: u32) -> AxResult<i32> {
    let f: Arc<dyn FileLike> = match result {
        OpenResult::File(mut file) => {
            // /dev/xx handling
            if let Ok(device) = file.location().entry().downcast::<Device>() {
                let inner = device.inner().as_any();
                if let Some(ptmx) = inner.downcast_ref::<tty::Ptmx>() {
                    // Opening /dev/ptmx creates a new pseudo-terminal
                    let (master, pty_number) = ptmx.create_pty()?;
                    // TODO: this is cursed
                    let pts = FS_CONTEXT.lock().resolve("/dev/pts")?;
                    let entry = DirEntry::new_file(
                        FileNode::new(master),
                        NodeType::CharacterDevice,
                        Reference::new(Some(pts.entry().clone()), pty_number.to_string()),
                    );
                    let loc = Location::new(file.location().mountpoint().clone(), entry);
                    file = axfs::File::new(FileBackend::Direct(loc), file.flags());
                } else if inner.is::<tty::CurrentTty>() {
                    let term = current()
                        .as_thread()
                        .proc_data
                        .proc
                        .group()
                        .session()
                        .terminal()
                        .ok_or(AxError::NotFound)?;
                    let path = if term.is::<tty::NTtyDriver>() {
                        "/dev/console".to_string()
                    } else if let Some(pts) = term.downcast_ref::<tty::PtyDriver>() {
                        format!("/dev/pts/{}", pts.pty_number())
                    } else {
                        panic!("unknown terminal type")
                    };
                    let loc = FS_CONTEXT.lock().resolve(&path)?;
                    file = axfs::File::new(FileBackend::Direct(loc), file.flags());
                }
            }
            Arc::new(File::new(file, flags))
        }
        OpenResult::Dir(dir) => Arc::new(Directory::new(dir)),
    };
    if flags & O_NONBLOCK != 0 {
        f.set_nonblocking(true)?;
    }
    add_file_like(f, flags & O_CLOEXEC != 0)
}

/// Open or create a file.
/// fd: file descriptor
/// filename: file path to be opened or created
/// flags: open flags
/// mode: see man 7 inode
/// return new file descriptor if succeed, or return -1.
pub fn sys_openat(
    dirfd: c_int,
    path: *const c_char,
    flags: i32,
    mode: __kernel_mode_t,
) -> AxResult<isize> {
    let path = vm_load_string(path)?;
    debug!("sys_openat <= {dirfd} {path:?} {flags:#o} {mode:#o}");

    let mode = mode & !current().as_thread().proc_data.umask();

    let options = flags_to_options(flags, mode, (sys_geteuid()? as _, sys_getegid()? as _));
    with_fs(dirfd, |fs| options.open(fs, path))
        .and_then(|it| add_to_fd(it, flags as _))
        .map(|fd| fd as isize)
}

/// Open a file by `filename` and insert it into the file descriptor table.
///
/// Return its index in the file table (`fd`). Return `EMFILE` if it already
/// has the maximum number of files open.
#[cfg(target_arch = "x86_64")]
pub fn sys_open(path: *const c_char, flags: i32, mode: __kernel_mode_t) -> AxResult<isize> {
    sys_openat(AT_FDCWD as _, path, flags, mode)
}

pub fn sys_close(fd: c_int) -> AxResult<isize> {
    debug!("sys_close <= {fd}");
    close_file_like(fd)?;
    Ok(0)
}

bitflags! {
    #[derive(Debug, Clone, Copy)]
    struct CloseRangeFlags: u32 {
        const UNSHARE = 1 << 1;
        const CLOEXEC = 1 << 2;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileLock {
    pid: u64,
    start: u64,
    end: Option<u64>,
    typ: i16,
}

#[derive(Default)]
struct LockState {
    locks: BTreeMap<u64, Vec<FileLock>>,
}

lazy_static::lazy_static! {
    static ref LOCK_STATE: axsync::Mutex<LockState> = axsync::Mutex::new(LockState::default());
}

fn current_pid() -> u64 {
    current().as_thread().proc_data.proc.pid().into()
}

pub fn release_locks_for_pid(pid: u64) {
    let mut state = LOCK_STATE.lock();
    for locks in state.locks.values_mut() {
        locks.retain(|lock| lock.pid != pid);
    }
}

fn lock_range_to_abs(file: &File, flock: &flock64) -> AxResult<(u64, Option<u64>)> {
    let base = match flock.l_whence as u32 {
        SEEK_SET => 0,
        SEEK_CUR => file.position()?,
        SEEK_END => file.inner().location().len().unwrap_or_default() as u64,
        _ => return Err(AxError::InvalidInput),
    };
    let start = base.checked_add_signed(flock.l_start).ok_or(AxError::InvalidInput)?;
    let end = if flock.l_len == 0 {
        None
    } else if flock.l_len > 0 {
        Some(start.checked_add(flock.l_len as u64).ok_or(AxError::InvalidInput)?)
    } else {
        return Err(AxError::InvalidInput);
    };
    Ok((start, end))
}

fn overlap(a_start: u64, a_end: Option<u64>, b_start: u64, b_end: Option<u64>) -> bool {
    if let Some(a_end) = a_end {
        if b_start >= a_end {
            return false;
        }
    }
    if let Some(b_end) = b_end {
        if a_start >= b_end {
            return false;
        }
    }
    true
}

fn range_extends_past(end: Option<u64>, point: u64) -> bool {
    match end {
        Some(end) => end > point,
        None => true,
    }
}

fn retain_unlocked_segments(lock: FileLock, start: u64, end: Option<u64>, out: &mut Vec<FileLock>) {
    if !overlap(start, end, lock.start, lock.end) {
        out.push(lock);
        return;
    }

    if lock.start < start {
        out.push(FileLock {
            start: lock.start,
            end: Some(start),
            ..lock
        });
    }

    if let Some(end) = end {
        if range_extends_past(lock.end, end) {
            out.push(FileLock {
                start: end,
                end: lock.end,
                ..lock
            });
        }
    }
}

fn merge_same_owner_locks(locks: &mut Vec<FileLock>) {
    locks.sort_by_key(|lock| (lock.pid, lock.typ, lock.start, lock.end.unwrap_or(u64::MAX)));

    let mut merged: Vec<FileLock> = Vec::with_capacity(locks.len());
    for lock in locks.drain(..) {
        if let Some(prev) = merged.last_mut() {
            let same_owner = prev.pid == lock.pid && prev.typ == lock.typ;
            let touching = match prev.end {
                Some(prev_end) => lock.start <= prev_end,
                None => true,
            };
            if same_owner && touching {
                prev.end = match (prev.end, lock.end) {
                    (None, _) | (_, None) => None,
                    (Some(a), Some(b)) => Some(a.max(b)),
                };
                continue;
            }
        }
        merged.push(lock);
    }
    *locks = merged;
}

fn map_lock_range(file: &File, flock: &flock64) -> AxResult<(u64, Option<u64>)> {
    lock_range_to_abs(file, flock)
}

fn locks_conflict(query_type: i16, existing_type: i16) -> bool {
    query_type == F_WRLCK as i16 || existing_type == F_WRLCK as i16
}

fn conflicting_lock(file: &File, flock: &flock64) -> AxResult<Option<FileLock>> {
    let key = file.inner().location().inode() as u64;
    let pid = current_pid();
    let (query_start, query_end) = map_lock_range(file, flock)?;
    let state = LOCK_STATE.lock();
    Ok(state.locks.get(&key).and_then(|locks| {
        locks
            .iter()
            .filter(|lock| {
                lock.pid != pid
                    && overlap(query_start, query_end, lock.start, lock.end)
                    && locks_conflict(flock.l_type, lock.typ)
            })
            .min_by_key(|lock| (lock.start, lock.end.unwrap_or(u64::MAX), lock.pid))
            .copied()
    }))
}

fn get_file_lock(file: &File, flock: &mut flock64) -> AxResult<()> {
    if let Some(lock) = conflicting_lock(file, flock)? {
        flock.l_type = lock.typ as _;
        flock.l_start = lock.start as _;
        flock.l_len = match lock.end {
            Some(end) => end.checked_sub(lock.start).ok_or(AxError::InvalidInput)? as _,
            None => 0,
        };
        flock.l_pid = lock.pid as _;
        return Ok(());
    }
    flock.l_type = F_UNLCK as _;
    Ok(())
}

fn set_file_lock(file: &File, flock: &flock64) -> AxResult<Option<FileLock>> {
    let key = file.inner().location().inode() as u64;
    let pid = current_pid();
    let (start, end) = map_lock_range(file, flock)?;
    let mut state = LOCK_STATE.lock();
    let locks = state.locks.entry(key).or_default();

    if flock.l_type != F_UNLCK as _ && flock.l_type != F_RDLCK as _ && flock.l_type != F_WRLCK as _ {
        return Err(AxError::InvalidInput);
    }

    if let Some(conflict) = locks
        .iter()
        .filter(|lock| {
            lock.pid != pid
                && overlap(start, end, lock.start, lock.end)
                && locks_conflict(flock.l_type, lock.typ)
        })
        .min_by_key(|lock| (lock.start, lock.end.unwrap_or(u64::MAX), lock.pid))
        .copied()
    {
        return Ok(Some(conflict));
    }

    let mut updated = Vec::with_capacity(locks.len() + usize::from(flock.l_type != F_UNLCK as _));
    for lock in locks.drain(..) {
        if lock.pid == pid {
            retain_unlocked_segments(lock, start, end, &mut updated);
        } else {
            updated.push(lock);
        }
    }

    if flock.l_type != F_UNLCK as _ {
        updated.push(FileLock {
            pid,
            start,
            end,
            typ: flock.l_type,
        });
    }

    merge_same_owner_locks(&mut updated);
    *locks = updated;
    Ok(None)
}

pub fn release_locks_for_fd(fd: c_int) {
    if let Ok(file) = get_file_like(fd) {
        if let Some(file) = file.downcast_ref::<File>() {
            let key = file.inner().location().inode() as u64;
            let pid = current_pid();
            let mut state = LOCK_STATE.lock();
            if let Some(locks) = state.locks.get_mut(&key) {
                locks.retain(|lock| lock.pid != pid);
            }
        }
    }
}

pub fn sys_close_range(first: u32, last: u32, flags: u32) -> AxResult<isize> {
    if last < first {
        return Err(AxError::InvalidInput);
    }
    let flags = CloseRangeFlags::from_bits(flags).ok_or(AxError::InvalidInput)?;
    debug!("sys_close_range <= fds: [{first}, {last}], flags: {flags:?}");
    if flags.contains(CloseRangeFlags::UNSHARE) {
        // TODO: optimize
        let curr = current();
        let mut scope = curr.as_thread().proc_data.scope.write();
        let mut guard = FD_TABLE.scope_mut(&mut scope);
        let old_files = mem::take(guard.deref_mut());
        old_files.write().clone_from(old_files.read().deref());
    }

    let cloexec = flags.contains(CloseRangeFlags::CLOEXEC);
    let mut fd_table = FD_TABLE.write();
    if let Some(max_index) = fd_table.ids().next_back() {
        for fd in first..=last.min(max_index as u32) {
            if cloexec {
                if let Some(f) = fd_table.get_mut(fd as _) {
                    f.cloexec = true;
                }
            } else {
                fd_table.remove(fd as _);
            }
        }
    }

    Ok(0)
}

fn dup_fd(old_fd: c_int, cloexec: bool) -> AxResult<isize> {
    let f = get_file_like(old_fd)?;
    let new_fd = add_file_like(f, cloexec)?;
    Ok(new_fd as _)
}

fn dup_fd_at(old_fd: c_int, min_fd: i32, cloexec: bool) -> AxResult<isize> {
    if min_fd < 0 {
        return Err(AxError::InvalidInput);
    }

    let f = get_file_like(old_fd)?;
    let max_nofile = current().as_thread().proc_data.rlim.read()[RLIMIT_NOFILE].current;
    let limit = (max_nofile as usize).min(AX_FILE_LIMIT);
    let fd = min_fd as usize;
    if fd >= limit {
        return Err(AxError::TooManyOpenFiles);
    }

    let mut table = FD_TABLE.write();
    let mut fd = fd;
    while fd < limit {
        match table.add_at(fd, FileDescriptor {
            inner: f.clone(),
            cloexec,
        }) {
            Ok(id) => return Ok(id as _),
            Err(_) => fd += 1,
        }
    }
    Err(AxError::TooManyOpenFiles)
}

pub fn sys_dup(old_fd: c_int) -> AxResult<isize> {
    debug!("sys_dup <= {old_fd}");
    dup_fd(old_fd, false)
}

#[cfg(target_arch = "x86_64")]
pub fn sys_dup2(old_fd: c_int, new_fd: c_int) -> AxResult<isize> {
    if old_fd == new_fd {
        get_file_like(new_fd)?;
        return Ok(new_fd as _);
    }
    sys_dup3(old_fd, new_fd, 0)
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Dup3Flags: c_int {
        const O_CLOEXEC = O_CLOEXEC as _; // Close on exec
    }
}

pub fn sys_dup3(old_fd: c_int, new_fd: c_int, flags: c_int) -> AxResult<isize> {
    let flags = Dup3Flags::from_bits(flags).ok_or(AxError::InvalidInput)?;
    debug!("sys_dup3 <= old_fd: {old_fd}, new_fd: {new_fd}, flags: {flags:?}");

    if old_fd == new_fd {
        return Err(AxError::InvalidInput);
    }

    let mut fd_table = FD_TABLE.write();
    let mut f = fd_table
        .get(old_fd as _)
        .cloned()
        .ok_or(AxError::BadFileDescriptor)?;
    f.cloexec = flags.contains(Dup3Flags::O_CLOEXEC);

    fd_table.remove(new_fd as _);
    fd_table
        .add_at(new_fd as _, f)
        .map_err(|_| AxError::BadFileDescriptor)?;

    Ok(new_fd as _)
}

pub fn sys_fcntl(fd: c_int, cmd: c_int, arg: usize) -> AxResult<isize> {
    debug!("sys_fcntl <= fd: {fd} cmd: {cmd} arg: {arg}");

    match cmd as u32 {
        F_DUPFD => dup_fd_at(fd, arg as i32, false),
        F_DUPFD_CLOEXEC => dup_fd_at(fd, arg as i32, true),
        F_SETLK | F_SETLKW => {
            let file = File::from_fd(fd)?;
            let flock = UserConstPtr::<flock64>::from(arg).get_as_ref()?;
            loop {
                if let Some(_conflict) = set_file_lock(&file, flock)? {
                    if cmd as u32 == F_SETLK {
                        return Err(AxError::from(LinuxError::EAGAIN));
                    }
                    axtask::yield_now();
                    continue;
                }
                return Ok(0);
            }
        }
        F_OFD_SETLK | F_OFD_SETLKW | F_OFD_GETLK => Err(AxError::InvalidInput),
        F_GETLK => {
            let file = File::from_fd(fd)?;
            let mut flock = *UserConstPtr::<flock64>::from(arg).get_as_ref()?;
            get_file_lock(&file, &mut flock)?;
            *UserPtr::<flock64>::from(arg).get_as_mut()? = flock;
            Ok(0)
        }
        F_SETFL => {
            get_file_like(fd)?.set_nonblocking(arg & (O_NONBLOCK as usize) > 0)?;
            Ok(0)
        }
        F_GETFL => {
            let f = get_file_like(fd)?;

            let mut ret = 0;
            if f.nonblocking() {
                ret |= O_NONBLOCK;
            }
            if let Some(file) = f.downcast_ref::<File>() {
                let open_flags = file.flags();
                let acc = open_flags & (O_RDONLY | O_WRONLY | O_RDWR) as u32;
                if acc != 0 {
                    ret |= acc;
                }
                if open_flags & O_APPEND as u32 != 0 {
                    ret |= O_APPEND;
                }
                if open_flags & O_DIRECT as u32 != 0 {
                    ret |= O_DIRECT;
                }
            } else {
                // Fallback for non-File file likes
                let perm = NodePermission::from_bits_truncate(f.stat()?.mode as _);
                if perm.contains(NodePermission::OWNER_WRITE) {
                    if perm.contains(NodePermission::OWNER_READ) {
                        ret |= O_RDWR;
                    } else {
                        ret |= O_WRONLY;
                    }
                }
            }

            Ok(ret as _)
        }
        F_GETFD => {
            let cloexec = FD_TABLE
                .read()
                .get(fd as _)
                .ok_or(AxError::BadFileDescriptor)?
                .cloexec;
            Ok(if cloexec { FD_CLOEXEC as _ } else { 0 })
        }
        F_SETFD => {
            let cloexec = arg & FD_CLOEXEC as usize != 0;
            FD_TABLE
                .write()
                .get_mut(fd as _)
                .ok_or(AxError::BadFileDescriptor)?
                .cloexec = cloexec;
            Ok(0)
        }
        F_GETPIPE_SZ => {
            let pipe = Pipe::from_fd(fd)?;
            Ok(pipe.capacity() as _)
        }
        F_SETPIPE_SZ => {
            let pipe = Pipe::from_fd(fd)?;
            pipe.resize(arg)?;
            Ok(0)
        }
        _ => Err(AxError::InvalidInput),
    }
}

pub fn sys_flock(fd: c_int, operation: c_int) -> AxResult<isize> {
    debug!("flock <= fd: {fd}, operation: {operation}");
    // TODO: flock
    Ok(0)
}
