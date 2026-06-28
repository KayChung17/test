use alloc::{borrow::Cow, collections::VecDeque, format, sync::Arc, vec, vec::Vec};
use core::{
    ffi::c_int,
    ops::Deref,
    sync::atomic::{AtomicUsize, Ordering},
    task::Context,
};

use axerrno::{AxError, AxResult};
use axio::prelude::*;
use axnet::{
    RecvOptions, SendOptions, Socket as SocketInner, SocketOps,
    options::{Configurable, GetSocketOption, SetSocketOption},
};
use axpoll::{IoEvents, Pollable};
use axsync::Mutex;
use linux_raw_sys::general::S_IFSOCK;

use super::{FileLike, Kstat};
use crate::file::{IoDst, IoSrc, get_file_like};

pub struct Socket(pub SocketInner);

impl Deref for Socket {
    type Target = SocketInner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FileLike for Socket {
    fn read(&self, dst: &mut IoDst) -> AxResult<usize> {
        self.recv(dst, RecvOptions::default())
    }

    fn write(&self, src: &mut IoSrc) -> AxResult<usize> {
        self.send(src, SendOptions::default())
    }

    fn stat(&self) -> AxResult<Kstat> {
        // TODO(mivik): implement stat for sockets
        Ok(Kstat {
            mode: S_IFSOCK | 0o777u32, // rwxrwxrwx
            blksize: 4096,
            ..Default::default()
        })
    }

    fn nonblocking(&self) -> bool {
        let mut result = false;
        self.get_option(GetSocketOption::NonBlocking(&mut result))
            .unwrap();
        result
    }

    fn set_nonblocking(&self, nonblocking: bool) -> AxResult<()> {
        self.0
            .set_option(SetSocketOption::NonBlocking(&nonblocking))
    }

    fn path(&self) -> Cow<'_, str> {
        format!("socket:[{}]", self as *const _ as usize).into()
    }

    fn from_fd(fd: c_int) -> AxResult<Arc<Self>>
    where
        Self: Sized + 'static,
    {
        get_file_like(fd)?
            .downcast_arc()
            .map_err(|_| AxError::NotASocket)
    }
}
impl Pollable for Socket {
    fn poll(&self) -> IoEvents {
        self.0.poll()
    }

    fn register(&self, context: &mut Context<'_>, events: IoEvents) {
        self.0.register(context, events);
    }
}

static RAW_SOCKET_ID: AtomicUsize = AtomicUsize::new(1);
static RAW_IPV6_PACKETS: Mutex<VecDeque<(usize, Vec<u8>)>> = Mutex::new(VecDeque::new());

pub struct RawIpv6Socket {
    id: usize,
    checksum_offset: Mutex<Option<usize>>,
}

impl RawIpv6Socket {
    pub fn new() -> Self {
        Self {
            id: RAW_SOCKET_ID.fetch_add(1, Ordering::Relaxed),
            checksum_offset: Mutex::new(None),
        }
    }

    pub fn set_checksum_offset(&self, offset: i32) -> AxResult<()> {
        if offset < 0 || offset % 2 != 0 {
            return Err(AxError::InvalidInput);
        }
        *self.checksum_offset.lock() = Some(offset as usize);
        Ok(())
    }

    pub fn checksum_offset(&self) -> i32 {
        self.checksum_offset.lock().map_or(-1, |offset| offset as i32)
    }

    pub fn send_packet(&self, src: &mut IoSrc) -> AxResult<usize> {
        let len = src.remaining();
        let mut packet = vec![0; len];
        let read = src.read(&mut packet)?;
        packet.truncate(read);

        if let Some(offset) = *self.checksum_offset.lock()
            && offset.checked_add(2).is_none_or(|end| end > packet.len())
        {
            return Err(AxError::InvalidInput);
        }

        RAW_IPV6_PACKETS.lock().push_back((self.id, packet));
        Ok(read)
    }

    pub fn recv_packet(&self, dst: &mut IoDst) -> AxResult<usize> {
        let mut packets = RAW_IPV6_PACKETS.lock();
        let Some(pos) = packets.iter().position(|(sender, _)| *sender != self.id) else {
            return Err(AxError::WouldBlock);
        };
        let (_, packet) = packets.remove(pos).unwrap();
        dst.write(&packet)
    }

    pub fn from_fd(fd: c_int) -> AxResult<Arc<Self>> {
        get_file_like(fd)?
            .downcast_arc()
            .map_err(|_| AxError::NotASocket)
    }
}

impl FileLike for RawIpv6Socket {
    fn read(&self, dst: &mut IoDst) -> AxResult<usize> {
        self.recv_packet(dst)
    }

    fn write(&self, src: &mut IoSrc) -> AxResult<usize> {
        self.send_packet(src)
    }

    fn stat(&self) -> AxResult<Kstat> {
        Ok(Kstat {
            mode: S_IFSOCK | 0o777u32,
            blksize: 4096,
            ..Default::default()
        })
    }

    fn path(&self) -> Cow<'_, str> {
        format!("socket:[raw-ipv6-{}]", self.id).into()
    }
}

impl Pollable for RawIpv6Socket {
    fn poll(&self) -> IoEvents {
        let readable = RAW_IPV6_PACKETS
            .lock()
            .iter()
            .any(|(sender, _)| *sender != self.id);
        if readable {
            IoEvents::IN | IoEvents::OUT
        } else {
            IoEvents::OUT
        }
    }

    fn register(&self, _context: &mut Context<'_>, _events: IoEvents) {}
}
