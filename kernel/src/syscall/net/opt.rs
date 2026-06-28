use alloc::vec::Vec;

use axerrno::{AxError, AxResult, LinuxError};
use axnet::options::{Configurable, GetSocketOption, SetSocketOption};
use axsync::Mutex;
use linux_raw_sys::net::socklen_t;

use crate::{
    file::{FileLike, RawIpv6Socket, Socket},
    mm::{UserConstPtr, UserPtr},
};

const PROTO_TCP: u32 = linux_raw_sys::net::IPPROTO_TCP as u32;

const PROTO_IP: u32 = linux_raw_sys::net::IPPROTO_IP as u32;
const PROTO_IPV6: u32 = linux_raw_sys::net::IPPROTO_IPV6 as u32;

static MULTICAST_MEMBERSHIPS: Mutex<Vec<usize>> = Mutex::new(Vec::new());

mod conv {
    use axerrno::{AxError, AxResult};
    use axnet::options::UnixCredentials;
    use linux_raw_sys::{general::timeval, net::ucred};

    use crate::time::TimeValueLike;

    pub struct Int<T>(T);

    impl<T: TryFrom<i32> + TryInto<i32>> Int<T> {
        pub fn sys_to_rust(val: i32) -> AxResult<T> {
            T::try_from(val).map_err(|_| AxError::InvalidInput)
        }

        pub fn rust_to_sys(val: T) -> AxResult<i32> {
            val.try_into().map_err(|_| AxError::InvalidInput)
        }
    }

    pub struct IntBool;

    impl IntBool {
        pub fn sys_to_rust(val: i32) -> AxResult<bool> {
            Ok(val != 0)
        }

        pub fn rust_to_sys(val: bool) -> AxResult<i32> {
            Ok(val as _)
        }
    }

    pub struct Duration;

    impl Duration {
        pub fn sys_to_rust(val: timeval) -> AxResult<core::time::Duration> {
            val.try_into_time_value()
        }

        pub fn rust_to_sys(val: core::time::Duration) -> AxResult<timeval> {
            Ok(timeval::from_time_value(val))
        }
    }

    pub struct Ucred;

    impl Ucred {
        pub fn sys_to_rust(val: ucred) -> AxResult<UnixCredentials> {
            Ok(UnixCredentials {
                pid: val.pid,
                uid: val.uid,
                gid: val.gid,
            })
        }

        pub fn rust_to_sys(val: UnixCredentials) -> AxResult<ucred> {
            Ok(ucred {
                pid: val.pid,
                uid: val.uid,
                gid: val.gid,
            })
        }
    }
}

macro_rules! call_dispatch {
    ($dispatch:ident, $pat:expr) => {{
        use conv::*;
        use linux_raw_sys::net::*;

        call_dispatch! {
            $dispatch, $pat,
            (SOL_SOCKET, SO_REUSEADDR) => ReuseAddress as IntBool,
            (SOL_SOCKET, SO_ERROR) => Error,
            (SOL_SOCKET, SO_DONTROUTE) => DontRoute as IntBool,
            (SOL_SOCKET, SO_SNDBUF) => SendBuffer as Int<usize>,
            (SOL_SOCKET, SO_RCVBUF) => ReceiveBuffer as Int<usize>,
            (SOL_SOCKET, SO_KEEPALIVE) => KeepAlive as IntBool,
            (SOL_SOCKET, SO_RCVTIMEO) => ReceiveTimeout as Duration,
            (SOL_SOCKET, SO_SNDTIMEO) => SendTimeout as Duration,
            (SOL_SOCKET, SO_PASSCRED) => PassCredentials as IntBool,
            (SOL_SOCKET, SO_PEERCRED) => PeerCredentials as Ucred,

            (PROTO_TCP, TCP_NODELAY) => NoDelay as IntBool,
            (PROTO_TCP, TCP_MAXSEG) => MaxSegment as Int<usize>,
            (PROTO_TCP, TCP_INFO) => TcpInfo,

            (PROTO_IP, IP_TTL) => Ttl as Int<u8>,
        }
    }};
    ($dispatch:ident, $in:expr, $($pat:pat => $which:ident $(as $conv:ty)?),* $(,)?) => {
        match $in {
            $(
                $pat => {
                    dispatch!($which $(as $conv)?);
                }
            )*
            _ => return Err(AxError::from(LinuxError::ENOPROTOOPT)),
        }
    }
}

pub fn sys_getsockopt(
    fd: i32,
    level: u32,
    optname: u32,
    optval: UserPtr<u8>,
    optlen: UserPtr<socklen_t>,
) -> AxResult<isize> {
    let optlen = optlen.get_as_mut()?;
    debug!(
        "sys_getsockopt <= fd: {}, level: {}, optname: {}, optval: {:?}, optlen: {}",
        fd,
        level,
        optname,
        optval.address(),
        optlen,
    );

    fn get<'a, T: 'static>(val: UserPtr<u8>, len: &mut socklen_t) -> AxResult<&'a mut T> {
        if (*len as usize) < size_of::<T>() {
            return Err(AxError::InvalidInput);
        }
        *len = size_of::<T>() as socklen_t;
        val.cast().get_as_mut()
    }

    if level == PROTO_IPV6 && optname == linux_raw_sys::net::IPV6_CHECKSUM {
        *get::<i32>(optval, optlen)? = RawIpv6Socket::from_fd(fd)?.checksum_offset();
        return Ok(0);
    }

    let socket = Socket::from_fd(fd)?;
    macro_rules! dispatch {
        ($which:ident) => {
            socket.get_option(GetSocketOption::$which(get(optval, optlen)?))?;
        };
        ($which:ident as $conv:ty) => {
            let mut val = Default::default();
            socket.get_option(GetSocketOption::$which(&mut val))?;
            *get(optval, optlen)? = <$conv>::rust_to_sys(val)?;
        };
    }
    call_dispatch!(dispatch, (level, optname));

    Ok(0)
}

pub fn sys_setsockopt(
    fd: i32,
    level: u32,
    optname: u32,
    optval: UserConstPtr<u8>,
    optlen: socklen_t,
) -> AxResult<isize> {
    debug!(
        "sys_setsockopt <= fd: {}, level: {}, optname: {}, optval: {:?}, optlen: {}",
        fd,
        level,
        optname,
        optval.address(),
        optlen
    );

    fn get<'a, T: 'static>(val: UserConstPtr<u8>, len: socklen_t) -> AxResult<&'a T> {
        if len as usize != size_of::<T>() {
            return Err(AxError::InvalidInput);
        }
        val.cast().get_as_ref()
    }

    if level == PROTO_IPV6 && optname == linux_raw_sys::net::IPV6_CHECKSUM {
        let offset = *get::<i32>(optval, optlen)?;
        RawIpv6Socket::from_fd(fd)?.set_checksum_offset(offset)?;
        return Ok(0);
    }

    let socket = Socket::from_fd(fd)?;
    if level == linux_raw_sys::net::SOL_SOCKET && optname == linux_raw_sys::net::SO_ATTACH_BPF {
        let prog_fd = *get::<i32>(optval, optlen)?;
        super::super::run_bpf_socket_filter(prog_fd)?;
        return Ok(0);
    }
    if level == PROTO_IP && optname == linux_raw_sys::net::MCAST_JOIN_GROUP {
        let _ = optval.get_as_slice(optlen as usize)?;
        let socket_key = &*socket as *const Socket as usize;
        let mut memberships = MULTICAST_MEMBERSHIPS.lock();
        if memberships.iter().all(|it| *it != socket_key) {
            memberships.push(socket_key);
        }
        return Ok(0);
    }
    if level == PROTO_IP && optname == linux_raw_sys::net::MCAST_LEAVE_GROUP {
        let _ = optval.get_as_slice(optlen as usize)?;
        let socket_key = &*socket as *const Socket as usize;
        let mut memberships = MULTICAST_MEMBERSHIPS.lock();
        if let Some(pos) = memberships.iter().position(|it| *it == socket_key) {
            memberships.swap_remove(pos);
            return Ok(0);
        }
        return Err(AxError::from(LinuxError::EADDRNOTAVAIL));
    }

    macro_rules! dispatch {
        ($which:ident) => {
            socket.set_option(SetSocketOption::$which(get(optval, optlen)?))?;
        };
        ($which:ident as $conv:ty) => {
            let mut val = <$conv>::sys_to_rust(*get(optval, optlen)?)?;
            socket.set_option(SetSocketOption::$which(&mut val))?;
        };
    }
    call_dispatch!(dispatch, (level, optname));

    Ok(0)
}
