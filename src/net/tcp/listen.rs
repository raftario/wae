use std::{
    ffi::c_void,
    future::Future,
    io, mem,
    net::SocketAddr,
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
        winsock2::{bind, listen, WSAGetLastError, WSAIoctl, SOMAXCONN},
    },
};

use futures_core::Stream;
use socket2::SockAddr;

use super::{socket::TcpSocket, Incoming, TcpListener, TcpStream};
use crate::{
    net::ToSocketAddrs,
    overlapped::{event::Event, io::IO},
    threadpool::Handle,
};

impl TcpListener {
    const ADDR_SPACE: usize = mem::size_of::<SOCKADDR_IN6>() + 16;

    pub async fn bind<A: ToSocketAddrs>(addr: A) -> io::Result<TcpListener> {
        let handle = Handle::current();

        let socket = TcpSocket::new()?;

        let acceptex = unsafe {
            let mut acceptex: LPFN_ACCEPTEX = None;
            let acceptex_size = mem::size_of::<LPFN_ACCEPTEX>() as u32;
            let mut returned = 0;

            let ret = WSAIoctl(
                *socket,
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
                *socket,
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

        let event = Event::new(&handle.callback_environ())?;

        let addrs = addr.to_socket_addrs().await?;

        let mut tried = 0;
        let mut bound = false;

        for addr in addrs {
            let sock_addr = SockAddr::from(addr);
            let addr = sock_addr.as_ptr();
            let len = sock_addr.len();

            let ret = unsafe { bind(*socket, addr, len) };

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

        if unsafe { listen(*socket, SOMAXCONN) } != 0 {
            return Err(io::Error::last_os_error());
        }

        let next = TcpSocket::new()?;

        Ok(TcpListener {
            acceptex,
            gaesa,
            socket,
            event,
            next,
            buffer: Vec::with_capacity(Self::ADDR_SPACE * 2),
        })
    }

    #[inline]
    pub async fn accept(&mut self) -> io::Result<(TcpStream, SocketAddr)> {
        self.accept_with_capacity(None, false, None, false).await
    }

    #[inline]
    pub async fn accept_with_capacity(
        &mut self,
        read_capacity: impl Into<Option<usize>>,
        read_capacity_fixed: bool,
        write_capacity: impl Into<Option<usize>>,
        write_capacity_fixed: bool,
    ) -> io::Result<(TcpStream, SocketAddr)> {
        Incoming {
            listener: self,
            read_capacity: read_capacity.into(),
            write_capacity: write_capacity.into(),
            read_capacity_fixed,
            write_capacity_fixed,
        }
        .await
    }

    pub fn poll_accept(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<(TcpStream, SocketAddr)>> {
        self.poll_accept_with_capacity(cx, None, false, None, false)
    }

    pub fn poll_accept_with_capacity(
        &mut self,
        cx: &mut Context<'_>,
        read_capacity: impl Into<Option<usize>>,
        read_capacity_fixed: bool,
        write_capacity: impl Into<Option<usize>>,
        write_capacity_fixed: bool,
    ) -> Poll<io::Result<(TcpStream, SocketAddr)>> {
        let socket = *self.socket;
        let acceptex = self.acceptex;
        let next = *self.next;
        let buffer = self.buffer.as_mut_ptr();

        let poll = self.event.poll(cx, Some(socket as HANDLE), |overlapped| {
            let ret = unsafe {
                acceptex(
                    socket,
                    next,
                    buffer as *mut c_void,
                    0,
                    Self::ADDR_SPACE as u32,
                    Self::ADDR_SPACE as u32,
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

                let read_capacity = read_capacity.into().unwrap_or(TcpStream::DEFAULT_CAPACITY);
                let write_capacity = write_capacity.into().unwrap_or(TcpStream::DEFAULT_CAPACITY);

                let mut next = TcpSocket::new()?;
                mem::swap(&mut next, &mut self.next);

                let mut addr = ptr::null_mut();
                let mut addr_len = 0;
                let sock_addr = unsafe {
                    (self.gaesa)(
                        buffer as *mut c_void,
                        0,
                        Self::ADDR_SPACE as u32,
                        Self::ADDR_SPACE as u32,
                        &mut ptr::null_mut(),
                        &mut 0,
                        &mut addr,
                        &mut addr_len,
                    );
                    SockAddr::from_raw_parts(addr, addr_len)
                };

                let inner = unsafe {
                    IO::new(
                        next,
                        read_capacity,
                        read_capacity_fixed,
                        write_capacity,
                        write_capacity_fixed,
                        &handle.callback_environ(),
                    )
                }?;
                Poll::Ready(Ok((TcpStream { inner }, sock_addr.as_std().unwrap())))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            Poll::Pending => Poll::Pending,
        }
    }

    pub fn incoming(&mut self) -> Incoming<'_> {
        self.incoming_with_capacity(None, false, None, false)
    }

    pub fn incoming_with_capacity(
        &mut self,
        read_capacity: impl Into<Option<usize>>,
        read_capacity_fixed: bool,
        write_capacity: impl Into<Option<usize>>,
        write_capacity_fixed: bool,
    ) -> Incoming<'_> {
        Incoming {
            listener: self,
            read_capacity: read_capacity.into(),
            read_capacity_fixed,
            write_capacity: write_capacity.into(),
            write_capacity_fixed,
        }
    }
}

impl Future for Incoming<'_> {
    type Output = io::Result<(TcpStream, SocketAddr)>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let read_capacity = self.read_capacity;
        let read_capacity_fixed = self.read_capacity_fixed;
        let write_capacity = self.write_capacity;
        let write_capacity_fixed = self.write_capacity_fixed;
        self.listener.poll_accept_with_capacity(
            cx,
            read_capacity,
            read_capacity_fixed,
            write_capacity,
            write_capacity_fixed,
        )
    }
}

impl Stream for Incoming<'_> {
    type Item = io::Result<(TcpStream, SocketAddr)>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let read_capacity = self.read_capacity;
        let read_capacity_fixed = self.read_capacity_fixed;
        let write_capacity = self.write_capacity;
        let write_capacity_fixed = self.write_capacity_fixed;
        match self.listener.poll_accept_with_capacity(
            cx,
            read_capacity,
            read_capacity_fixed,
            write_capacity,
            write_capacity_fixed,
        ) {
            Poll::Ready(output) => Poll::Ready(Some(output)),
            Poll::Pending => Poll::Pending,
        }
    }
}
