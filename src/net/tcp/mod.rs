mod connect;
mod listen;
mod read;
mod socket;
mod write;

use std::sync::Arc;

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
    client: socket::TcpSocket,
    buffer: Vec<u8>,
}

pub struct Incoming<'a> {
    listener: &'a mut TcpListener,
    read_capacity: Option<usize>,
    write_capacity: Option<usize>,
}
