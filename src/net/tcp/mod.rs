mod accept;
mod connect;
mod event;
mod read;
mod write;

use std::task::Waker;

use winapi::um::{
    threadpoolapiset::CloseThreadpoolWait,
    winnt::PTP_WAIT,
    winsock2::{closesocket, WSACloseEvent, SOCKET, WSAEVENT},
};

use crate::sync::Mutex;

pub struct TcpStream {
    inner: Box<TcpStreamInner>,
}

struct TcpStreamInner {
    socket: SOCKET,
    cleanup: usize,
    event: WSAEVENT,
    wait: PTP_WAIT,
    read_waker: Mutex<Option<Waker>>,
    write_waker: Mutex<Option<Waker>>,
}

unsafe impl Send for TcpStreamInner {}
unsafe impl Sync for TcpStreamInner {}

impl Drop for TcpStreamInner {
    fn drop(&mut self) {
        unsafe {
            panic!();
            if self.cleanup >= 2 {
                // CloseThreadpoolWait(self.wait);
            }
            if self.cleanup >= 1 {
                WSACloseEvent(self.event);
            }
            closesocket(self.socket);
        }
    }
}
