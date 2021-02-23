use std::{
    ffi::c_void,
    fmt,
    future::Future,
    io, mem,
    net::{Shutdown, SocketAddr},
    os::windows::io::FromRawSocket,
    pin::Pin,
    ptr::{self},
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
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

use socket2::{SockAddr, Socket};

use crate::{
    io::shared::{IoEvent, IoHandle},
    net::ToSocketAddrs,
    threadpool::Handle,
    util::Extract,
};

pub struct TcpStream {
    pub(super) inner: Arc<IoHandle>,
}

struct Connect<'a> {
    connectex: <LPFN_CONNECTEX as Extract>::Inner,
    socket: SOCKET,
    event: &'a IoEvent,
    addr: &'a SOCKADDR,
    len: i32,
}

impl TcpStream {
    #[inline]
    fn with_socket<T>(&self, f: impl FnOnce(&Socket) -> T) -> T {
        let socket = unsafe { Socket::from_raw_socket(self.inner.handle as u64) };
        let output = f(&socket);
        mem::forget(socket);
        output
    }

    pub async fn connect<A: ToSocketAddrs>(addr: A) -> io::Result<TcpStream> {
        let handle = Handle::current();
        let socket = super::socket::new()?;

        let bind_sockaddr = SOCKADDR_IN6 {
            sin6_family: AF_INET as u16,
            sin6_port: 0,
            sin6_addr: Default::default(),
            ..Default::default()
        };
        if unsafe {
            bind(
                socket,
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
                    socket,
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
                socket,
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

        let event: Box<IoEvent> = IoEvent::new(&handle.callback_environ())?;

        let addrs = addr.to_socket_addrs().await?;

        let mut result = Err(io::Error::from_raw_os_error(0));
        let mut tried = 0;

        for addr in addrs {
            let sock_addr = SockAddr::from(addr);
            let addr = unsafe { sock_addr.as_ptr().read() };
            let len = sock_addr.len();

            result = Connect {
                connectex,
                socket,
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
                let inner = IoHandle::new(
                    socket as HANDLE,
                    super::socket::close,
                    super::read::schedule,
                    super::socket::cancel,
                    super::write::schedule,
                    super::socket::cancel,
                    &Handle::current().callback_environ(),
                )?;
                Ok(TcpStream { inner })
            }
            Err(err) if tried > 0 => Err(err),
            _ => Err(io::Error::new(
                io::ErrorKind::NotFound,
                "the provided address couldn't be resolved",
            )),
        }
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.with_socket(|s| s.local_addr().map(|a| a.as_std().unwrap()))
    }

    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.with_socket(|s| s.peer_addr().map(|a| a.as_std().unwrap()))
    }

    pub fn ttl(&self) -> io::Result<u32> {
        self.with_socket(|s| s.ttl())
    }

    pub fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        self.with_socket(move |s| s.set_ttl(ttl))
    }

    pub fn nodelay(&self) -> io::Result<bool> {
        self.with_socket(|s| s.nodelay())
    }

    pub fn set_nodelay(&self, nodelay: bool) -> io::Result<()> {
        self.with_socket(move |s| s.set_nodelay(nodelay))
    }

    pub fn linger(&self) -> io::Result<Option<Duration>> {
        self.with_socket(|s| s.linger())
    }

    pub fn set_linger(&self, dur: Option<Duration>) -> io::Result<()> {
        self.with_socket(move |s| s.set_linger(dur))
    }

    pub fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        self.with_socket(move |s| s.shutdown(how))
    }
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

impl fmt::Debug for TcpStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut dbg = f.debug_struct("TcpListener");

        if let Ok(addr) = self.local_addr() {
            dbg.field("addr", &addr);
        }
        if let Ok(addr) = self.peer_addr() {
            dbg.field("peer", &addr);
        }

        dbg.finish()
    }
}
