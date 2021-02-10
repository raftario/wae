mod connect;
mod listen;
mod read;
mod socket;
mod write;

use std::{fmt, sync::Arc};

use winapi::um::mswsock::{LPFN_ACCEPTEX, LPFN_GETACCEPTEXSOCKADDRS};

use crate::{
    overlapped::{event::Event, io::IO},
    util::Extract,
};

pub struct TcpStream {
    inner: Arc<IO<socket::TcpSocket>>,
}

pub struct TcpListener {
    acceptex: <LPFN_ACCEPTEX as Extract>::Inner,
    gaesa: <LPFN_GETACCEPTEXSOCKADDRS as Extract>::Inner,
    socket: socket::TcpSocket,
    event: Box<Event>,
    next: socket::TcpSocket,
    buffer: Vec<u8>,
}

#[derive(Debug)]
pub struct Incoming<'a> {
    listener: &'a mut TcpListener,
    read_capacity: Option<usize>,
    write_capacity: Option<usize>,
}
impl fmt::Debug for TcpStream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut dbg = f.debug_struct("TcpListener");
        dbg.field("socket", &*self.inner.handle());

        if let Ok(addr) = self.local_addr() {
            dbg.field("addr", &addr);
        }
        if let Ok(addr) = self.peer_addr() {
            dbg.field("peer", &addr);
        }

        dbg.field("read_capacity", &self.inner.read_capacity())
            .field("write_capacity", &self.inner.write_capacity())
            .finish()
    }
}

impl fmt::Debug for TcpListener {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut dbg = f.debug_struct("TcpListener");
        dbg.field("socket", &self.socket);

        if let Ok(addr) = self.local_addr() {
            dbg.field("addr", &addr);
        }

        dbg.finish()
    }
}
