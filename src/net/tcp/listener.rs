use std::{
    ffi::c_void,
    fmt,
    future::Future,
    io, mem,
    net::SocketAddr,
    os::windows::io::{AsRawSocket, FromRawSocket},
    pin::Pin,
    ptr,
    task::{Context, Poll},
};
use winapi::{
    shared::{
        guiddef::GUID, minwindef::TRUE, ws2def::SIO_GET_EXTENSION_FUNCTION_POINTER,
        ws2ipdef::SOCKADDR_IN6,
    },
    um::{
        mswsock::{
            LPFN_ACCEPTEX, LPFN_GETACCEPTEXSOCKADDRS, WSAID_ACCEPTEX, WSAID_GETACCEPTEXSOCKADDRS,
        },
        winnt::HANDLE,
        winsock2::{bind, listen, WSAGetLastError, WSAIoctl, SOCKET, SOMAXCONN},
    },
};

use socket2::{SockAddr, Socket};

use super::TcpStream;
use crate::{
    io::shared::{IoEvent, IoHandle},
    net::ToSocketAddrs,
    threadpool::Handle,
    util::Extract,
};

pub struct TcpListener {
    socket: Socket,
    acceptex: <LPFN_ACCEPTEX as Extract>::Inner,
    gaesa: <LPFN_GETACCEPTEXSOCKADDRS as Extract>::Inner,
}

pub struct Accept<'a> {
    listener: &'a TcpListener,
    client: Result<SOCKET, i32>,
    event: Result<Box<IoEvent>, i32>,
    buf: Vec<u8>,
}

#[cfg(feature = "stream")]
pub struct Incoming<'a> {
    accept: Accept<'a>,
}

impl TcpListener {
    const ADDR_SPACE: usize = mem::size_of::<SOCKADDR_IN6>() + 16;

    pub async fn bind<A: ToSocketAddrs>(addr: A) -> io::Result<TcpListener> {
        let socket = super::socket::new()?;

        let acceptex = unsafe {
            let mut acceptex: LPFN_ACCEPTEX = None;
            let acceptex_size = mem::size_of::<LPFN_ACCEPTEX>() as u32;
            let mut returned = 0;

            let ret = WSAIoctl(
                socket,
                SIO_GET_EXTENSION_FUNCTION_POINTER,
                &WSAID_ACCEPTEX as *const GUID as *mut c_void,
                mem::size_of::<GUID>() as u32,
                &mut acceptex as *mut LPFN_ACCEPTEX as *mut c_void,
                acceptex_size,
                &mut returned,
                ptr::null_mut(),
                None,
            );
            match (ret, acceptex) {
                (0, Some(acceptex)) => acceptex,
                _ => return Err(io::Error::last_os_error()),
            }
        };

        let gaesa = unsafe {
            let mut gaesa: LPFN_GETACCEPTEXSOCKADDRS = None;
            let gaesa_size = mem::size_of::<LPFN_GETACCEPTEXSOCKADDRS>() as u32;
            let mut returned = 0;

            let ret = WSAIoctl(
                socket,
                SIO_GET_EXTENSION_FUNCTION_POINTER,
                &WSAID_GETACCEPTEXSOCKADDRS as *const GUID as *mut c_void,
                mem::size_of::<GUID>() as u32,
                &mut gaesa as *mut LPFN_GETACCEPTEXSOCKADDRS as *mut c_void,
                gaesa_size,
                &mut returned,
                ptr::null_mut(),
                None,
            );
            match (ret, gaesa) {
                (0, Some(gaesa)) => gaesa,
                _ => return Err(io::Error::last_os_error()),
            }
        };

        let addrs = addr.to_socket_addrs().await?;

        let mut tried = 0;
        let mut bound = false;

        for addr in addrs {
            let sock_addr = SockAddr::from(addr);
            let addr = sock_addr.as_ptr();
            let len = sock_addr.len();

            let ret = unsafe { bind(socket, addr, len) };

            tried += 1;
            if ret == 0 {
                bound = true;
                break;
            }
        }

        if !bound && tried > 0 {
            return Err(io::Error::last_os_error());
        } else if !bound {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "the provided address couldn't be resolved",
            ));
        }

        if unsafe { listen(socket, SOMAXCONN) } != 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(TcpListener {
            socket: unsafe { Socket::from_raw_socket(socket as u64) },
            acceptex,
            gaesa,
        })
    }

    pub fn accept(&self) -> Accept<'_> {
        Accept {
            listener: self,
            client: super::socket::new().map_err(|err| err.raw_os_error().unwrap()),
            event: IoEvent::new(&Handle::current().callback_environ())
                .map_err(|err| err.raw_os_error().unwrap()),
            buf: Vec::with_capacity(Self::ADDR_SPACE * 2),
        }
    }

    #[cfg(feature = "stream")]
    pub fn incoming(&self) -> Incoming<'_> {
        Incoming {
            accept: self.accept(),
        }
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr().map(|a| a.as_std().unwrap())
    }

    pub fn ttl(&self) -> io::Result<u32> {
        self.socket.ttl()
    }

    pub fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        self.socket.set_ttl(ttl)
    }
}

impl Future for Accept<'_> {
    type Output = io::Result<(TcpStream, SocketAddr)>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let socket = self.listener.socket.as_raw_socket() as SOCKET;
        let acceptex = self.listener.acceptex;
        let client = self.client.map_err(io::Error::from_raw_os_error)?;
        let buf = self.buf.as_mut_ptr();

        let poll = self
            .event
            .as_ref()
            .map_err(|err| io::Error::from_raw_os_error(*err))?
            .poll(cx, Some(socket as HANDLE), |overlapped| {
                let ret = unsafe {
                    acceptex(
                        socket,
                        client,
                        buf as *mut c_void,
                        0,
                        TcpListener::ADDR_SPACE as u32,
                        TcpListener::ADDR_SPACE as u32,
                        &mut 0,
                        overlapped,
                    )
                };
                if ret == TRUE {
                    return Poll::Ready(Ok(()));
                }

                let err = unsafe { WSAGetLastError() };
                match err {
                    // WSA_IO_PENDING
                    997 => Poll::Pending,
                    _ => Poll::Ready(Err(io::Error::from_raw_os_error(err))),
                }
            });
        match poll {
            Poll::Ready(Ok(())) => {
                let handle = Handle::current();

                let mut addr = ptr::null_mut();
                let mut addr_len = 0;
                let sock_addr = unsafe {
                    (self.listener.gaesa)(
                        buf as *mut c_void,
                        0,
                        TcpListener::ADDR_SPACE as u32,
                        TcpListener::ADDR_SPACE as u32,
                        &mut ptr::null_mut(),
                        &mut 0,
                        &mut addr,
                        &mut addr_len,
                    );
                    SockAddr::from_raw_parts(addr, addr_len)
                };

                let inner = IoHandle::new(
                    client as HANDLE,
                    super::socket::close,
                    super::read::schedule,
                    super::socket::cancel,
                    super::write::schedule,
                    super::socket::cancel,
                    &handle.callback_environ(),
                )?;
                Poll::Ready(Ok((TcpStream { inner }, sock_addr.as_std().unwrap())))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(feature = "stream")]
impl futures_core::Stream for Incoming<'_> {
    type Item = io::Result<(TcpStream, SocketAddr)>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        let listener = this.accept.listener;
        match Pin::new(&mut this.accept).poll(cx) {
            Poll::Ready(output) => {
                this.accept = listener.accept();
                Poll::Ready(Some(output))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl fmt::Debug for TcpListener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut dbg = f.debug_struct("TcpListener");
        dbg.field("socket", &self.socket);

        if let Ok(addr) = self.local_addr() {
            dbg.field("addr", &addr);
        }

        dbg.finish()
    }
}

impl fmt::Debug for Accept<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Accept")
            .field("listener", &self.listener)
            .finish()
    }
}

#[cfg(feature = "stream")]
impl fmt::Debug for Incoming<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Incoming")
            .field("listener", self.accept.listener)
            .finish()
    }
}
