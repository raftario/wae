use std::{
    fmt, io, mem,
    net::{Shutdown, SocketAddr},
    ops::Deref,
    ptr,
    time::Duration,
};

use winapi::{
    shared::{
        minwindef::{BOOL, FALSE, TRUE},
        ws2def::{
            AF_UNSPEC, IPPROTO_IP, IPPROTO_TCP, SOCKADDR, SOCK_STREAM, SOL_SOCKET, SO_LINGER,
            TCP_NODELAY,
        },
        ws2ipdef::{IP_TTL, SOCKADDR_IN6},
    },
    um::{
        winnt::HANDLE,
        winsock2::{
            closesocket, getpeername, getsockname, getsockopt, setsockopt, shutdown, WSASocketW,
            INVALID_SOCKET, LINGER, SD_BOTH, SD_RECEIVE, SD_SEND, SOCKET, WSA_FLAG_OVERLAPPED,
        },
    },
};

use socket2::SockAddr;

use super::{TcpListener, TcpStream};
use crate::overlapped::io::Handle;

pub(crate) struct TcpSocket(SOCKET);

impl TcpSocket {
    pub(crate) fn new() -> io::Result<TcpSocket> {
        let socket = unsafe {
            WSASocketW(
                AF_UNSPEC,
                SOCK_STREAM,
                IPPROTO_TCP as i32,
                ptr::null_mut(),
                0,
                WSA_FLAG_OVERLAPPED,
            )
        };
        if socket != INVALID_SOCKET {
            Ok(Self(socket))
        } else {
            Err(io::Error::last_os_error())
        }
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        let mut addr = SOCKADDR_IN6::default();
        let addr = &mut addr as *mut SOCKADDR_IN6 as *mut SOCKADDR;
        let mut len = mem::size_of::<SOCKADDR_IN6>() as i32;
        if unsafe { getsockname(self.0, addr, &mut len) } == 0 {
            let sock_addr = unsafe { SockAddr::from_raw_parts(addr, len) };
            Ok(sock_addr.as_std().unwrap())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    fn peer_addr(&self) -> io::Result<SocketAddr> {
        let mut addr = SOCKADDR_IN6::default();
        let addr = &mut addr as *mut SOCKADDR_IN6 as *mut SOCKADDR;
        let mut len = mem::size_of::<SOCKADDR_IN6>() as i32;
        if unsafe { getpeername(self.0, addr, &mut len) } == 0 {
            let sock_addr = unsafe { SockAddr::from_raw_parts(addr, len) };
            Ok(sock_addr.as_std().unwrap())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    fn ttl(&self) -> io::Result<u32> {
        let mut ttl = 0;
        let mut len = mem::size_of::<u32>() as i32;
        if unsafe {
            getsockopt(
                self.0,
                IPPROTO_IP,
                IP_TTL,
                &mut ttl as *mut u32 as *mut i8,
                &mut len,
            )
        } == 0
        {
            Ok(ttl)
        } else {
            Err(io::Error::last_os_error())
        }
    }

    fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        let len = mem::size_of::<u32>() as i32;
        if unsafe {
            setsockopt(
                self.0,
                IPPROTO_IP,
                IP_TTL,
                &ttl as *const u32 as *const i8,
                len,
            )
        } == 0
        {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    fn nodelay(&self) -> io::Result<bool> {
        let mut nodelay = FALSE;
        let mut len = mem::size_of::<BOOL>() as i32;
        if unsafe {
            getsockopt(
                self.0,
                IPPROTO_TCP as i32,
                TCP_NODELAY,
                &mut nodelay as *mut BOOL as *mut i8,
                &mut len,
            )
        } == 0
        {
            Ok(nodelay != FALSE)
        } else {
            Err(io::Error::last_os_error())
        }
    }

    fn set_nodelay(&self, nodelay: bool) -> io::Result<()> {
        let nodelay = if nodelay { TRUE } else { FALSE };
        let len = mem::size_of::<u32>() as i32;
        if unsafe {
            setsockopt(
                self.0,
                IPPROTO_TCP as i32,
                TCP_NODELAY,
                &nodelay as *const BOOL as *const i8,
                len,
            )
        } == 0
        {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    fn linger(&self) -> io::Result<Option<Duration>> {
        let mut linger = LINGER::default();
        let mut len = mem::size_of::<LINGER>() as i32;
        if unsafe {
            getsockopt(
                self.0,
                IPPROTO_TCP as i32,
                TCP_NODELAY,
                &mut linger as *mut LINGER as *mut i8,
                &mut len,
            )
        } == 0
        {
            Ok(match linger.l_onoff {
                0 => None,
                _ => Some(Duration::from_secs(linger.l_linger as u64)),
            })
        } else {
            Err(io::Error::last_os_error())
        }
    }

    fn set_linger(&self, linger: Option<Duration>) -> io::Result<()> {
        let linger = match linger {
            Some(duration) => LINGER {
                l_onoff: 1,
                l_linger: duration.as_secs() as u16,
            },
            None => LINGER {
                l_onoff: 0,
                l_linger: 0,
            },
        };
        let len = mem::size_of::<LINGER>() as i32;
        if unsafe {
            setsockopt(
                self.0,
                SOL_SOCKET,
                SO_LINGER,
                &linger as *const LINGER as *const i8,
                len,
            )
        } == 0
        {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        let how = match how {
            Shutdown::Read => SD_RECEIVE,
            Shutdown::Write => SD_SEND,
            Shutdown::Both => SD_BOTH,
        };
        if unsafe { shutdown(self.0, how) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

impl TcpStream {
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.handle().local_addr()
    }

    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.inner.handle().peer_addr()
    }

    pub fn ttl(&self) -> io::Result<u32> {
        self.inner.handle().ttl()
    }

    pub fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        self.inner.handle().set_ttl(ttl)
    }

    pub fn nodelay(&self) -> io::Result<bool> {
        self.inner.handle().nodelay()
    }

    pub fn set_nodelay(&self, nodelay: bool) -> io::Result<()> {
        self.inner.handle().set_nodelay(nodelay)
    }

    pub fn linger(&self) -> io::Result<Option<Duration>> {
        self.inner.handle().linger()
    }

    pub fn set_linger(&self, linger: Option<Duration>) -> io::Result<()> {
        self.inner.handle().set_linger(linger)
    }

    pub fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        self.inner.handle().shutdown(how)
    }
}

impl TcpListener {
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    pub fn ttl(&self) -> io::Result<u32> {
        self.socket.ttl()
    }

    pub fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        self.socket.set_ttl(ttl)
    }
}

impl Deref for TcpSocket {
    type Target = SOCKET;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Handle for TcpSocket {
    fn from_handle(handle: HANDLE) -> Self {
        Self(handle as SOCKET)
    }

    fn as_handle(&self) -> HANDLE {
        self.0 as HANDLE
    }
}

impl Drop for TcpSocket {
    fn drop(&mut self) {
        unsafe {
            closesocket(self.0);
        }
    }
}

impl fmt::Debug for TcpSocket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}
