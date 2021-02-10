use std::{fmt, io, mem, net::SocketAddr, ops::Deref, ptr};

use winapi::{
    shared::{
        ws2def::{AF_UNSPEC, IPPROTO_TCP, SOCKADDR, SOCK_STREAM},
        ws2ipdef::SOCKADDR_IN6,
    },
    um::{
        winnt::HANDLE,
        winsock2::{
            closesocket, getpeername, getsockname, WSASocketW, INVALID_SOCKET, SOCKET,
            WSA_FLAG_OVERLAPPED,
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

    pub(crate) fn local_addr(&self) -> io::Result<SocketAddr> {
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

    pub(crate) fn peer_addr(&self) -> io::Result<SocketAddr> {
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
}

impl TcpStream {
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.handle().local_addr()
    }

    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.inner.handle().peer_addr()
    }
}

impl TcpListener {
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
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
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}
