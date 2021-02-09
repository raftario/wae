mod accept;
mod connect;
mod read;
mod write;

use std::sync::Arc;

use winapi::um::{
    winnt::HANDLE,
    winsock2::{closesocket, SOCKET},
};

use crate::overlapped::io::{Handle, IO};

pub struct TcpStream {
    inner: Arc<IO<TcpSocket>>,
}

struct TcpSocket(SOCKET);

impl TcpStream {
    pub const DEFAULT_CAPACITY: usize = 1024;
}

impl Handle for TcpSocket {
    fn from_handle(handle: HANDLE) -> Self {
        Self(handle as SOCKET)
    }

    fn into_handle(self) -> HANDLE {
        self.0 as HANDLE
    }

    fn close(self) {
        unsafe {
            closesocket(self.0);
        }
    }
}
