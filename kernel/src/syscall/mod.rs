mod bpf;
mod fs;
mod io_mpx;
mod ipc;
mod mm;
mod net;
mod resources;
mod signal;
mod sync;
mod sys;
mod task;
mod time;

use axerrno::{AxError, LinuxError};
use axhal::uspace::UserContext;
use syscalls::Sysno;

pub use self::{
    bpf::*, fs::*, io_mpx::*, ipc::*, mm::*, net::*, resources::*, signal::*, sync::*, sys::*,
    task::*, time::*,
};

pub fn handle_syscall(uctx: &mut UserContext) {
    let Some(sysno) = Sysno::new(uctx.sysno()) else {
        warn!("Invalid syscall number: {}", uctx.sysno());
        uctx.set_retval(-LinuxError::ENOSYS.code() as _);
        return;
    };

    trace!("Syscall {sysno:?}");

    let (a0, a1, a2, a3, a4, a5) = (
        uctx.arg0(),
        uctx.arg1(),
        uctx.arg2(),
        uctx.arg3(),
        uctx.arg4(),
        uctx.arg5(),
    );

    let result = match sysno {
        // fs ctl
        Sysno::ioctl => sys_ioctl(a0 as _, a1 as _, a2 as _),
        Sysno::chdir => sys_chdir(a0 as _),
        Sysno::fchdir => sys_fchdir(a0 as _),
        Sysno::chroot => sys_chroot(a0 as _),
        #[cfg(target_arch = "x86_64")]
        Sysno::mkdir => sys_mkdir(a0 as _, a1 as _),
        Sysno::mkdirat => sys_mkdirat(a0 as _, a1 as _, a2 as _),
        Sysno::getdents64 => sys_getdents64(a0 as _, a1 as _, a2 as _),
        #[cfg(target_arch = "x86_64")]
        Sysno::link => sys_link(a0 as _, a1 as _),
        Sysno::linkat => sys_linkat(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
            a4 as _,
        ),
        #[cfg(target_arch = "x86_64")]
        Sysno::rmdir => sys_rmdir(a0 as _),
        #[cfg(target_arch = "x86_64")]
        Sysno::unlink => sys_unlink(a0 as _),
        Sysno::unlinkat => sys_unlinkat(a0 as _, a1 as _, a2 as _),
        Sysno::getcwd => sys_getcwd(a0 as _, a1 as _),
        #[cfg(target_arch = "x86_64")]
        Sysno::symlink => sys_symlink(a0 as _, a1 as _),
        Sysno::symlinkat => sys_symlinkat(a0 as _, a1 as _, a2 as _),
        #[cfg(target_arch = "x86_64")]
        Sysno::rename => sys_rename(a0 as _, a1 as _),
        #[cfg(not(target_arch = "riscv64"))]
        Sysno::renameat => sys_renameat(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
        ),
        Sysno::renameat2 => sys_renameat2(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
            a4 as _,
        ),
        Sysno::sync => sys_sync(),
        Sysno::syncfs => sys_syncfs(a0 as _),
        Sysno::acct => sys_acct(a0.into()),

        // file ops
        #[cfg(target_arch = "x86_64")]
        Sysno::chown => sys_chown(a0 as _, a1 as _, a2 as _),
        #[cfg(target_arch = "x86_64")]
        Sysno::lchown => sys_lchown(a0 as _, a1 as _, a2 as _),
        Sysno::fchown => sys_fchown(a0 as _, a1 as _, a2 as _),
        Sysno::fchownat => sys_fchownat(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
            a4 as _,
        ),
        #[cfg(target_arch = "x86_64")]
        Sysno::chmod => sys_chmod(a0 as _, a1 as _),
        Sysno::fchmod => sys_fchmod(a0 as _, a1 as _),
        Sysno::fchmodat | Sysno::fchmodat2 => sys_fchmodat(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
        ),
        #[cfg(target_arch = "x86_64")]
        Sysno::readlink => sys_readlink(a0 as _, a1 as _, a2 as _),
        Sysno::readlinkat => sys_readlinkat(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
        ),
        #[cfg(target_arch = "x86_64")]
        Sysno::utime => sys_utime(a0 as _, a1 as _),
        #[cfg(target_arch = "x86_64")]
        Sysno::utimes => sys_utimes(a0 as _, a1 as _),
        Sysno::utimensat => sys_utimensat(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
        ),

        // fd ops
        #[cfg(target_arch = "x86_64")]
        Sysno::open => sys_open(a0 as _, a1 as _, a2 as _),
        Sysno::openat => sys_openat(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
        ),
        Sysno::close => sys_close(a0 as _),
        Sysno::close_range => sys_close_range(a0 as _, a1 as _, a2 as _),
        Sysno::dup => sys_dup(a0 as _),
        #[cfg(target_arch = "x86_64")]
        Sysno::dup2 => sys_dup2(a0 as _, a1 as _),
        Sysno::dup3 => sys_dup3(a0 as _, a1 as _, a2 as _),
        Sysno::fcntl => sys_fcntl(a0 as _, a1 as _, a2 as _),
        Sysno::flock => sys_flock(a0 as _, a1 as _),

        // io
        Sysno::read => sys_read(a0 as _, a1 as _, a2 as _),
        Sysno::readv => sys_readv(a0 as _, a1 as _, a2 as _),
        Sysno::write => sys_write(a0 as _, a1 as _, a2 as _),
        Sysno::writev => sys_writev(a0 as _, a1 as _, a2 as _),
        Sysno::lseek => sys_lseek(a0 as _, a1 as _, a2 as _),
        Sysno::truncate => sys_truncate(a0.into(), a1 as _),
        Sysno::ftruncate => sys_ftruncate(a0 as _, a1 as _),
        Sysno::fallocate => sys_fallocate(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
        ),
        Sysno::fsync => sys_fsync(a0 as _),
        Sysno::fdatasync => sys_fdatasync(a0 as _),
        Sysno::fadvise64 => sys_fadvise64(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
        ),
        Sysno::pread64 => sys_pread64(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
        ),
        Sysno::pwrite64 => sys_pwrite64(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
        ),
        Sysno::preadv => sys_preadv(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
        ),
        Sysno::pwritev => sys_pwritev(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
        ),
        Sysno::preadv2 => sys_preadv2(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
            a4 as _,
        ),
        Sysno::pwritev2 => sys_pwritev2(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
            a4 as _,
        ),
        Sysno::sendfile => sys_sendfile(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
        ),
        Sysno::copy_file_range => sys_copy_file_range(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
            a4 as _,
            a5 as _,
        ),
        Sysno::splice => sys_splice(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
            a4 as _,
            a5 as _,
        ),

        // io mpx
        #[cfg(target_arch = "x86_64")]
        Sysno::poll => sys_poll(a0.into(), a1 as _, a2 as _),
        Sysno::ppoll => sys_ppoll(
            a0.into(),
            a1 as _,
            a2.into(),
            a3.into(),
            a4 as _,
        ),
        #[cfg(target_arch = "x86_64")]
        Sysno::select => sys_select(
            a0 as _,
            a1.into(),
            a2.into(),
            a3.into(),
            a4.into(),
        ),
        Sysno::pselect6 => sys_pselect6(
            a0 as _,
            a1.into(),
            a2.into(),
            a3.into(),
            a4.into(),
            a5.into(),
        ),
        Sysno::epoll_create1 => sys_epoll_create1(a0 as _),
        Sysno::epoll_ctl => sys_epoll_ctl(
            a0 as _,
            a1 as _,
            a2 as _,
            a3.into(),
        ),
        Sysno::epoll_pwait => sys_epoll_pwait(
            a0 as _,
            a1.into(),
            a2 as _,
            a3 as _,
            a4.into(),
            a5 as _,
        ),
        Sysno::epoll_pwait2 => sys_epoll_pwait2(
            a0 as _,
            a1.into(),
            a2 as _,
            a3.into(),
            a4.into(),
            a5 as _,
        ),

        // fs mount
        Sysno::mount => sys_mount(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
            a4 as _,
        ) as _,
        Sysno::umount2 => sys_umount2(a0 as _, a1 as _) as _,

        // pipe
        Sysno::pipe2 => sys_pipe2(a0 as _, a1 as _),
        #[cfg(target_arch = "x86_64")]
        Sysno::pipe => sys_pipe2(a0 as _, 0),

        // event
        Sysno::eventfd2 => sys_eventfd2(a0 as _, a1 as _),

        // pidfd
        Sysno::pidfd_open => sys_pidfd_open(a0 as _, a1 as _),
        Sysno::pidfd_getfd => sys_pidfd_getfd(a0 as _, a1 as _, a2 as _),
        Sysno::pidfd_send_signal => sys_pidfd_send_signal(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
        ),

        // memfd
        Sysno::memfd_create => sys_memfd_create(a0.into(), a1 as _),

        // fs stat
        #[cfg(target_arch = "x86_64")]
        Sysno::stat => sys_stat(a0 as _, a1 as _),
        Sysno::fstat => sys_fstat(a0 as _, a1 as _),
        #[cfg(target_arch = "x86_64")]
        Sysno::lstat => sys_lstat(a0 as _, a1 as _),
        #[cfg(any(target_arch = "x86_64", target_arch = "riscv64"))]
        Sysno::newfstatat => sys_fstatat(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
        ),
        #[cfg(not(any(target_arch = "x86_64", target_arch = "riscv64")))]
        Sysno::fstatat => sys_fstatat(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
        ),
        Sysno::statx => sys_statx(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
            a4 as _,
        ),
        #[cfg(target_arch = "x86_64")]
        Sysno::access => sys_access(a0 as _, a1 as _),
        Sysno::faccessat => sys_faccessat2(
            a0 as _,
            a1 as _,
            a2 as _,
            0,
        ),
        Sysno::faccessat2 => sys_faccessat2(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
        ),
        Sysno::statfs => sys_statfs(a0 as _, a1 as _),
        Sysno::fstatfs => sys_fstatfs(a0 as _, a1 as _),

        // mm
        Sysno::brk => sys_brk(a0 as _),
        Sysno::mmap => sys_mmap(
            a0,
            a1 as _,
            a2 as _,
            a3 as _,
            a4 as _,
            a5 as _,
        ),
        Sysno::munmap => sys_munmap(a0, a1 as _),
        Sysno::mprotect => sys_mprotect(a0, a1 as _, a2 as _),
        Sysno::mincore => sys_mincore(a0 as _, a1 as _, a2 as _),
        Sysno::mremap => sys_mremap(
            a0,
            a1 as _,
            a2 as _,
            a3 as _,
        ),
        Sysno::madvise => sys_madvise(a0, a1 as _, a2 as _),
        Sysno::msync => sys_msync(a0, a1 as _, a2 as _),
        Sysno::mlock => sys_mlock(a0, a1 as _),
        Sysno::mlock2 => sys_mlock2(a0, a1 as _, a2 as _),

        // task info
        Sysno::getpid => sys_getpid(),
        Sysno::getppid => sys_getppid(),
        Sysno::gettid => sys_gettid(),
        Sysno::getrusage => sys_getrusage(a0 as _, a1 as _),

        // task sched
        Sysno::sched_yield => sys_sched_yield(),
        Sysno::nanosleep => sys_nanosleep(a0 as _, a1 as _),
        Sysno::clock_nanosleep => sys_clock_nanosleep(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
        ),
        Sysno::sched_getaffinity => {
            sys_sched_getaffinity(a0 as _, a1 as _, a2 as _)
        }
        Sysno::sched_setaffinity => {
            sys_sched_setaffinity(a0 as _, a1 as _, a2 as _)
        }
        Sysno::sched_getscheduler => sys_sched_getscheduler(a0 as _),
        Sysno::sched_setscheduler => {
            sys_sched_setscheduler(a0 as _, a1 as _, a2 as _)
        }
        Sysno::sched_getparam => sys_sched_getparam(a0 as _, a1 as _),
        Sysno::getpriority => sys_getpriority(a0 as _, a1 as _),
        Sysno::setpriority => sys_setpriority(a0 as _, a1 as _, a2 as _),
        Sysno::getrlimit => sys_getrlimit(a0 as _, a1 as _),

        // task ops
        Sysno::execve => sys_execve(uctx, a0 as _, a1 as _, a2 as _),
        Sysno::set_tid_address => sys_set_tid_address(a0),
        #[cfg(target_arch = "x86_64")]
        Sysno::arch_prctl => sys_arch_prctl(uctx, a0 as _, a1 as _),
        Sysno::prctl => sys_prctl(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
            a4 as _,
        ),
        Sysno::prlimit64 => sys_prlimit64(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
        ),
        Sysno::capget => sys_capget(a0 as _, a1 as _),
        Sysno::capset => sys_capset(a0 as _, a1 as _),
        Sysno::umask => sys_umask(a0 as _),
        Sysno::setreuid => sys_setreuid(a0 as _, a1 as _),
        Sysno::setregid => sys_setregid(a0 as _, a1 as _),
        Sysno::setresuid => sys_setresuid(a0 as _, a1 as _, a2 as _),
        Sysno::getresuid => sys_getresuid(a0 as _, a1 as _, a2 as _),
        Sysno::setresgid => sys_setresgid(a0 as _, a1 as _, a2 as _),
        Sysno::getresgid => sys_getresgid(a0 as _, a1 as _, a2 as _),
        Sysno::get_mempolicy => sys_get_mempolicy(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
            a4 as _,
        ),

        // task management
        Sysno::clone => sys_clone(
            uctx,
            a0 as _,
            a1 as _,
            a2,
            a3,
            a4,
        ),
        Sysno::clone3 => sys_clone3(
            uctx,
            a0 as _, // args_ptr
            a1 as _, // args_size
        ),
        Sysno::unshare => sys_unshare(a0 as _),
        #[cfg(target_arch = "x86_64")]
        Sysno::fork => sys_fork(uctx),
        Sysno::exit => sys_exit(a0 as _),
        Sysno::exit_group => sys_exit_group(a0 as _),
        Sysno::wait4 => sys_waitpid(a0 as _, a1 as _, a2 as _),
        Sysno::getsid => sys_getsid(a0 as _),
        Sysno::setsid => sys_setsid(),
        Sysno::getpgid => sys_getpgid(a0 as _),
        Sysno::setpgid => sys_setpgid(a0 as _, a1 as _),

        // signal
        Sysno::rt_sigprocmask => sys_rt_sigprocmask(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
        ),
        Sysno::rt_sigaction => sys_rt_sigaction(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
        ),
        Sysno::rt_sigpending => sys_rt_sigpending(a0 as _, a1 as _),
        Sysno::rt_sigreturn => sys_rt_sigreturn(uctx),
        Sysno::rt_sigtimedwait => sys_rt_sigtimedwait(
            uctx,
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
        ),
        Sysno::rt_sigsuspend => sys_rt_sigsuspend(uctx, a0 as _, a1 as _),
        Sysno::kill => sys_kill(a0 as _, a1 as _),
        Sysno::tkill => sys_tkill(a0 as _, a1 as _),
        Sysno::tgkill => sys_tgkill(a0 as _, a1 as _, a2 as _),
        Sysno::rt_sigqueueinfo => sys_rt_sigqueueinfo(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
        ),
        Sysno::rt_tgsigqueueinfo => sys_rt_tgsigqueueinfo(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
            a4 as _,
        ),
        Sysno::sigaltstack => sys_sigaltstack(a0 as _, a1 as _),
        Sysno::futex => sys_futex(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
            a4 as _,
            a5 as _,
        ),
        Sysno::get_robust_list => {
            sys_get_robust_list(a0 as _, a1 as _, a2 as _)
        }
        Sysno::set_robust_list => sys_set_robust_list(a0 as _, a1 as _),

        // sys
        Sysno::getuid => sys_getuid(),
        Sysno::geteuid => sys_geteuid(),
        Sysno::getgid => sys_getgid(),
        Sysno::getegid => sys_getegid(),
        Sysno::setuid => sys_setuid(a0 as _),
        Sysno::setgid => sys_setgid(a0 as _),
        Sysno::getgroups => sys_getgroups(a0 as _, a1 as _),
        Sysno::setgroups => sys_setgroups(a0 as _, a1 as _),
        Sysno::uname => sys_uname(a0 as _),
        Sysno::sysinfo => sys_sysinfo(a0 as _),
        Sysno::syslog => sys_syslog(a0 as _, a1 as _, a2 as _),
        Sysno::getrandom => sys_getrandom(a0 as _, a1 as _, a2 as _),
        Sysno::seccomp => sys_seccomp(a0 as _, a1 as _, a2 as _),
        #[cfg(target_arch = "riscv64")]
        Sysno::riscv_flush_icache => sys_riscv_flush_icache(),

        // sync
        Sysno::membarrier => sys_membarrier(a0 as _, a1 as _, a2 as _),

        // time
        Sysno::gettimeofday => sys_gettimeofday(a0 as _),
        Sysno::times => sys_times(a0 as _),
        Sysno::clock_gettime => sys_clock_gettime(a0 as _, a1 as _),
        Sysno::clock_getres => sys_clock_getres(a0 as _, a1 as _),
        Sysno::clock_adjtime => sys_clock_adjtime(a0 as _, a1 as _),
        Sysno::adjtimex => sys_adjtimex(a0 as _),
        Sysno::getitimer => sys_getitimer(a0 as _, a1 as _),
        Sysno::setitimer => sys_setitimer(a0 as _, a1 as _, a2 as _),

        // msg
        Sysno::msgget => sys_msgget(a0 as _, a1 as _),
        Sysno::msgsnd => sys_msgsnd(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
        ),
        Sysno::msgrcv => sys_msgrcv(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
            a4 as _,
        ),
        Sysno::msgctl => sys_msgctl(a0 as _, a1 as _, a2 as _),

        // shm
        Sysno::shmget => sys_shmget(a0 as _, a1 as _, a2 as _),
        Sysno::shmat => sys_shmat(a0 as _, a1 as _, a2 as _),
        Sysno::shmctl => sys_shmctl(a0 as _, a1 as _, a2.into()),
        Sysno::shmdt => sys_shmdt(a0 as _),

        // net
        Sysno::socket => sys_socket(a0 as _, a1 as _, a2 as _),
        Sysno::socketpair => sys_socketpair(
            a0 as _,
            a1 as _,
            a2 as _,
            a3.into(),
        ),
        Sysno::bind => sys_bind(a0 as _, a1.into(), a2 as _),
        Sysno::connect => sys_connect(a0 as _, a1.into(), a2 as _),
        Sysno::getsockname => {
            sys_getsockname(a0 as _, a1.into(), a2.into())
        }
        Sysno::getpeername => {
            sys_getpeername(a0 as _, a1.into(), a2.into())
        }
        Sysno::listen => sys_listen(a0 as _, a1 as _),
        Sysno::accept => sys_accept(a0 as _, a1.into(), a2.into()),
        Sysno::accept4 => sys_accept4(
            a0 as _,
            a1.into(),
            a2.into(),
            a3 as _,
        ),
        Sysno::shutdown => sys_shutdown(a0 as _, a1 as _),
        Sysno::sendto => sys_sendto(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
            a4.into(),
            a5 as _,
        ),
        Sysno::recvfrom => sys_recvfrom(
            a0 as _,
            a1 as _,
            a2 as _,
            a3 as _,
            a4.into(),
            a5.into(),
        ),
        Sysno::sendmsg => sys_sendmsg(a0 as _, a1.into(), a2 as _),
        Sysno::recvmsg => sys_recvmsg(a0 as _, a1.into(), a2 as _),
        Sysno::getsockopt => sys_getsockopt(
            a0 as _,
            a1 as _,
            a2 as _,
            a3.into(),
            a4.into(),
        ),
        Sysno::setsockopt => sys_setsockopt(
            a0 as _,
            a1 as _,
            a2 as _,
            a3.into(),
            a4 as _,
        ),

        // signal file descriptors
        Sysno::signalfd4 => sys_signalfd4(
            a0 as _,
            a1 as _,
            a2,
            a3 as _,
        ),

        // dummy fds
        Sysno::fanotify_init
        | Sysno::inotify_init1
        | Sysno::userfaultfd
        | Sysno::perf_event_open
        | Sysno::io_uring_setup
        | Sysno::fsopen
        | Sysno::fspick
        | Sysno::open_tree
        | Sysno::memfd_secret => sys_dummy_fd(sysno),

        Sysno::bpf => sys_bpf(a0 as _, a1.into(), a2 as _),

        // timerfd
        Sysno::timerfd_create => sys_timerfd_create(a0 as _, a1 as _),
        Sysno::timerfd_settime => sys_timerfd_settime(a0 as _, a1 as _, a2 as _, a3 as _),
        Sysno::timerfd_gettime => sys_timerfd_gettime(a0 as _, a1 as _),

        Sysno::timer_create | Sysno::timer_gettime | Sysno::timer_settime => Ok(0),

        _ => {
            warn!("Unimplemented syscall: {sysno}");
            Err(AxError::Unsupported)
        }
    };
    debug!("Syscall {sysno} return {result:?}");

    uctx.set_retval(result.unwrap_or_else(|err| -LinuxError::from(err).code() as _) as _);
}
