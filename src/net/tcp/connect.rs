use std::{
    ffi::c_void,
    future::Future,
    io, mem,
    pin::Pin,
    ptr::{self},
    task::{Context, Poll},
};

use winapi::{
    shared::{
        guiddef::GUID,
        minwindef::TRUE,
        ws2def::{AF_INET, SIO_GET_EXTENSION_FUNCTION_POINTER, SOCKADDR, SOCKADDR_IN},
        ws2ipdef::SOCKADDR_IN6,
    },
    um::{
        mswsock::{LPFN_CONNECTEX, WSAID_CONNECTEX},
        winnt::HANDLE,
        winsock2::{bind, WSAGetLastError, WSAIoctl, SOCKET},
    },
};

use socket2::SockAddr;

use super::{socket::TcpSocket, TcpStream};
use crate::{
    net::ToSocketAddrs,
    overlapped::{event::Event, io::IO},
    threadpool::Handle,
    util::Extract,
};

impl TcpStream {
    pub const DEFAULT_CAPACITY: usize = 1024;

    #[inline]
    pub async fn connect<A: ToSocketAddrs>(addr: A) -> io::Result<TcpStream> {
        TcpStream::connect_with_capacity(addr, None, None).await
    }

    pub async fn connect_with_capacity<A: ToSocketAddrs>(
        addr: A,
        read_capacity: impl Into<Option<usize>>,
        write_capacity: impl Into<Option<usize>>,
    ) -> io::Result<TcpStream> {
        let handle = Handle::current();

        let read_capacity = read_capacity.into().unwrap_or(Self::DEFAULT_CAPACITY);
        let write_capacity = write_capacity.into().unwrap_or(Self::DEFAULT_CAPACITY);

        let socket = TcpSocket::new()?;

        let bind_sockaddr = SOCKADDR_IN6 {
            sin6_family: AF_INET as u16,
            sin6_port: 0,
            sin6_addr: Default::default(),
            ..Default::default()
        };
        if unsafe {
            bind(
                *socket,
                &bind_sockaddr as *const SOCKADDR_IN6 as *const SOCKADDR,
                mem::size_of::<SOCKADDR_IN6>() as i32,
            )
        } != 0
        {
            let bind_sockaddr = SOCKADDR_IN {
                sin_family: AF_INET as u16,
                sin_port: 0,
                sin_addr: Default::default(),
                ..Default::default()
            };
            if unsafe {
                bind(
                    *socket,
                    &bind_sockaddr as *const SOCKADDR_IN as *const SOCKADDR,
                    mem::size_of::<SOCKADDR_IN>() as i32,
                )
            } != 0
            {
                return Err(io::Error::last_os_error());
            }
        }

        let connectex = unsafe {
            let mut connectex: LPFN_CONNECTEX = None;
            let connectex_size = mem::size_of::<LPFN_CONNECTEX>() as u32;
            let mut returned = 0;

            let ret = WSAIoctl(
                *socket,
                SIO_GET_EXTENSION_FUNCTION_POINTER,
                &WSAID_CONNECTEX as *const GUID as *mut c_void,
                mem::size_of::<GUID>() as u32,
                &mut connectex as *mut LPFN_CONNECTEX as *mut c_void,
                connectex_size,
                &mut returned,
                ptr::null_mut(),
                None,
            );
            match (ret, connectex) {
                (0, Some(connectex)) => connectex,
                _ => return Err(io::Error::last_os_error()),
            }
        };

        let event: Box<Event> = Event::new(&handle.callback_environ())?;

        let addrs = addr.to_socket_addrs().await?;

        let mut result = Err(io::Error::from_raw_os_error(0));
        let mut tried = 0;

        for addr in addrs {
            let sock_addr = SockAddr::from(addr);
            let addr = unsafe { sock_addr.as_ptr().read() };
            let len = sock_addr.len();

            result = Connect {
                connectex,
                socket: *socket,
                event: &event,
                addr: &addr,
                len,
            }
            .await;

            tried += 1;
            if result.is_ok() {
                break;
            }
        }

        match result {
            Ok(()) => {
                let inner = unsafe {
                    IO::new(
                        socket,
                        read_capacity,
                        write_capacity,
                        &handle.callback_environ(),
                    )
                }?;
                Ok(TcpStream { inner })
            }
            Err(err) if tried > 0 => Err(err),
            _ => Err(io::Error::new(
                io::ErrorKind::NotFound,
                "the provided address couldn't be resolved",
            )),
        }
    }
}

struct Connect<'a> {
    connectex: <LPFN_CONNECTEX as Extract>::Inner,
    socket: SOCKET,
    event: &'a Event,
    addr: &'a SOCKADDR,
    len: i32,
}

impl Future for Connect<'_> {
    type Output = io::Result<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let socket = self.socket;
        let connectex = self.connectex;
        let addr = self.addr;
        let len = self.len;

        unsafe {
            self.event.poll(cx, Some(socket as HANDLE), |overlapped| {
                let ret = connectex(
                    socket,
                    addr,
                    len,
                    ptr::null_mut(),
                    0,
                    ptr::null_mut(),
                    overlapped,
                );
                if ret == TRUE {
                    return Poll::Ready(Ok(()));
                }

                let err = WSAGetLastError();
                match err {
                    // WSAEISCONN
                    10056 => Poll::Ready(Ok(())),
                    // WSA_IO_PENDING
                    997 => Poll::Pending,
                    _ => Poll::Ready(Err(io::Error::from_raw_os_error(err))),
                }
            })
        }
    }
}
