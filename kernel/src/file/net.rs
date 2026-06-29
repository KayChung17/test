use alloc::{borrow::Cow, format, sync::Arc, vec, vec::Vec};
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
use linux_raw_sys::{
    general::S_IFSOCK,
    ioctl::{SIOCGIFINDEX, SIOCSIFFLAGS},
    net::ifreq,
};

use super::{FileLike, Kstat};
use crate::{
    file::{IoDst, IoSrc, get_file_like},
    mm::{UserConstPtr, UserPtr},
};

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
static RAW_PACKET_SEQ: AtomicUsize = AtomicUsize::new(0);
static RAW_IPV6_PACKETS: Mutex<Vec<RawIpv6Packet>> = Mutex::new(Vec::new());

struct RawIpv6Packet {
    seq: usize,
    sender: usize,
    protocol: u32,
    data: Vec<u8>,
}

pub struct RawIpv6Socket {
    id: usize,
    protocol: u32,
    recv_seq: Mutex<usize>,
    checksum_offset: Mutex<Option<usize>>,
    icmp6_filter: Mutex<Option<[u32; 8]>>,
    ipv6_options: Mutex<Vec<(u32, i32)>>,
}

impl RawIpv6Socket {
    pub fn new(protocol: u32) -> Self {
        Self {
            id: RAW_SOCKET_ID.fetch_add(1, Ordering::Relaxed),
            protocol,
            recv_seq: Mutex::new(RAW_PACKET_SEQ.load(Ordering::Relaxed)),
            checksum_offset: Mutex::new(None),
            icmp6_filter: Mutex::new(None),
            ipv6_options: Mutex::new(Vec::new()),
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

    pub fn set_icmp6_filter(&self, filter: &[u8]) -> AxResult<()> {
        if filter.len() != 32 {
            return Err(AxError::InvalidInput);
        }
        let mut raw = [0u32; 8];
        for (slot, chunk) in raw.iter_mut().zip(filter.chunks_exact(size_of::<u32>())) {
            *slot = u32::from_ne_bytes(chunk.try_into().unwrap());
        }
        *self.icmp6_filter.lock() = Some(raw);
        Ok(())
    }

    pub fn set_ipv6_option(&self, optname: u32, value: i32) {
        let mut options = self.ipv6_options.lock();
        if let Some((_, current)) = options.iter_mut().find(|(opt, _)| *opt == optname) {
            *current = value;
        } else {
            options.push((optname, value));
        }
    }

    pub fn ipv6_option(&self, optname: u32) -> i32 {
        self.ipv6_options
            .lock()
            .iter()
            .find_map(|(opt, value)| (*opt == optname).then_some(*value))
            .unwrap_or(0)
    }

    pub fn cmsg_types(&self) -> Vec<u32> {
        let options = self.ipv6_options.lock();
        let mut result = Vec::new();
        for (opt, value) in options.iter().copied() {
            if value == 0 {
                continue;
            }
            let ty = match opt {
                49 => 50, // IPV6_RECVPKTINFO -> IPV6_PKTINFO
                51 => 52, // IPV6_RECVHOPLIMIT -> IPV6_HOPLIMIT
                53 => 54, // IPV6_RECVHOPOPTS -> IPV6_HOPOPTS
                56 => 57, // IPV6_RECVRTHDR -> IPV6_RTHDR
                58 => 59, // IPV6_RECVDSTOPTS -> IPV6_DSTOPTS
                66 => 67, // IPV6_RECVTCLASS -> IPV6_TCLASS
                2 | 3 | 4 | 5 | 8 => opt,
                _ => continue,
            };
            result.push(ty);
        }
        result
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

        RAW_IPV6_PACKETS.lock().push(RawIpv6Packet {
            seq: RAW_PACKET_SEQ.fetch_add(1, Ordering::Relaxed) + 1,
            sender: self.id,
            protocol: self.protocol,
            data: packet,
        });
        Ok(read)
    }

    pub fn recv_packet(&self, dst: &mut IoDst) -> AxResult<usize> {
        let packets = RAW_IPV6_PACKETS.lock();
        let recv_seq = *self.recv_seq.lock();
        let Some(packet) = packets.iter().find(|packet| {
            packet.seq > recv_seq
                && packet.sender != self.id
                && packet.protocol == self.protocol
                && self.allows_packet(&packet.data)
        }) else {
            return Err(AxError::WouldBlock);
        };
        *self.recv_seq.lock() = packet.seq;
        dst.write(&packet.data)
    }

    fn allows_packet(&self, packet: &[u8]) -> bool {
        if self.protocol != linux_raw_sys::net::IPPROTO_ICMPV6 || packet.is_empty() {
            return true;
        }
        let Some(filter) = *self.icmp6_filter.lock() else {
            return true;
        };
        let ty = packet[0] as usize;
        filter[ty / 32] & (1 << (ty % 32)) == 0
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
        let recv_seq = *self.recv_seq.lock();
        let readable = RAW_IPV6_PACKETS
            .lock()
            .iter()
            .any(|packet| {
                packet.seq > recv_seq
                    && packet.sender != self.id
                    && packet.protocol == self.protocol
                    && self.allows_packet(&packet.data)
            });
        if readable {
            IoEvents::IN | IoEvents::OUT
        } else {
            IoEvents::OUT
        }
    }

    fn register(&self, _context: &mut Context<'_>, _events: IoEvents) {}
}

pub struct PacketSocket {
    id: usize,
}

impl PacketSocket {
    pub fn new() -> Self {
        Self {
            id: RAW_SOCKET_ID.fetch_add(1, Ordering::Relaxed),
        }
    }

    pub fn bind_ll(&self, _addr: UserConstPtr<linux_raw_sys::net::sockaddr>, _len: u32) -> AxResult {
        Ok(())
    }

    pub fn from_fd(fd: c_int) -> AxResult<Arc<Self>> {
        get_file_like(fd)?
            .downcast_arc()
            .map_err(|_| AxError::NotASocket)
    }
}

impl FileLike for PacketSocket {
    fn stat(&self) -> AxResult<Kstat> {
        Ok(Kstat {
            mode: S_IFSOCK | 0o777u32,
            blksize: 4096,
            ..Default::default()
        })
    }

    fn ioctl(&self, cmd: u32, arg: usize) -> AxResult<usize> {
        match cmd {
            SIOCGIFINDEX => {
                let ifr = UserPtr::<ifreq>::from(arg).get_as_mut()?;
                ifr.ifr_ifru.ifru_ivalue = 1;
                Ok(0)
            }
            SIOCSIFFLAGS => Ok(0),
            _ => Err(AxError::NotATty),
        }
    }

    fn path(&self) -> Cow<'_, str> {
        format!("socket:[packet-{}]", self.id).into()
    }
}

impl Pollable for PacketSocket {
    fn poll(&self) -> IoEvents {
        IoEvents::IN | IoEvents::OUT
    }

    fn register(&self, _context: &mut Context<'_>, _events: IoEvents) {}
}
