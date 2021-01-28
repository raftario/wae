use std::{
    ffi::c_void,
    future::Future,
    io,
    pin::Pin,
    ptr::{self},
    task::{Context, Poll, Waker},
};

use socket2::SockAddr;
use winapi::{
    shared::ws2def::{AF_UNSPEC, IPPROTO_TCP, SOCKADDR},
    um::{
        threadpoolapiset::{CloseThreadpoolWait, CreateThreadpoolWait, SetThreadpoolWait},
        winnt::{PTP_CALLBACK_INSTANCE, PTP_WAIT, TP_WAIT_RESULT},
        winsock2::{
            connect, WSACloseEvent, WSACreateEvent, WSAEventSelect, WSAGetLastError, WSAResetEvent,
            WSASocketW, FD_CONNECT, INVALID_SOCKET, SOCKET, SOCK_STREAM, WSA_FLAG_OVERLAPPED,
            WSA_INVALID_EVENT,
        },
    },
};

use super::{TcpStream, TcpStreamInner};
use crate::{net::ToSocketAddrs, ohno::ForceSendSync, sync::Mutex, threadpool::Handle};

impl TcpStream {
    pub async fn connect<A: ToSocketAddrs>(addr: A) -> io::Result<TcpStream> {
        let mut handle = Handle::current();

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
        if socket == INVALID_SOCKET {
            return Err(io::Error::last_os_error());
        }

        let mut waker = Box::new(Mutex::new(None));

        let event = unsafe { ForceSendSync(WSACreateEvent()) };
        if event.0 == WSA_INVALID_EVENT {
            return Err(io::Error::last_os_error());
        }

        let wait = unsafe {
            if WSAEventSelect(socket, event.0, FD_CONNECT) != 0 {
                WSACloseEvent(event.0);
                return Err(io::Error::last_os_error());
            }

            let wait = CreateThreadpoolWait(
                Some(callback),
                &mut *waker as *mut Mutex<Option<Waker>> as *mut c_void,
                &mut handle.callback_environ,
            );
            if wait.is_null() {
                WSACloseEvent(event.0);
                return Err(io::Error::last_os_error());
            }
            SetThreadpoolWait(wait, event.0, ptr::null_mut());
            ForceSendSync(wait)
        };

        let addrs = addr.to_socket_addrs().await?;
        let mut connected = false;
        let mut tried = 0;
        for addr in addrs {
            let sock_addr = SockAddr::from(addr);
            let addr = unsafe { sock_addr.as_ptr().read() };
            let len = sock_addr.len();

            let connect = ConnectPriv {
                socket,
                addr,
                len,
                waker: &waker,
            }
            .await;

            tried += 1;
            if connect.is_ok() {
                connected = true;
                break;
            }

            unsafe {
                WSAResetEvent(event.0);
            }
        }

        unsafe {
            CloseThreadpoolWait(wait.0);
            WSACloseEvent(event.0);
        }

        if connected {
            let mut inner = Box::new(TcpStreamInner {
                socket,
                cleanup: 0,
                event: ptr::null_mut(),
                wait: ptr::null_mut(),
                read_waker: Mutex::new(None),
                write_waker: Mutex::new(None),
            });

            unsafe { super::event::evented(socket, handle.callback_environ, &mut inner) }?;

            Ok(TcpStream { inner })
        } else if tried > 0 {
            Err(io::Error::last_os_error())
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "the provided address couldn't be resolved",
            ))
        }
    }
}

struct ConnectPriv<'a> {
    socket: SOCKET,
    addr: SOCKADDR,
    len: i32,
    waker: &'a Mutex<Option<Waker>>,
}

unsafe impl Send for ConnectPriv<'_> {}
unsafe impl Sync for ConnectPriv<'_> {}

impl Future for ConnectPriv<'_> {
    type Output = io::Result<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let mut lock = self.waker.lock();

        let ret = unsafe { connect(self.socket, &self.addr, self.len) };
        if ret == 0 {
            return Poll::Ready(Ok(()));
        }

        let err = unsafe { WSAGetLastError() };
        match err {
            // WSAEISCONN
            10056 => Poll::Ready(Ok(())),
            // WSAEWOULDBLOCK | WSAEALREADY | WSAEINVAL
            10035 | 10037 | 10022 => {
                lock.replace(cx.waker().clone());
                Poll::Pending
            }
            _ => Poll::Ready(Err(io::Error::from_raw_os_error(err))),
        }
    }
}

unsafe extern "system" fn callback(
    _instance: PTP_CALLBACK_INSTANCE,
    context: *mut c_void,
    _wait: PTP_WAIT,
    _wait_result: TP_WAIT_RESULT,
) {
    let context = context as *const Mutex<Option<Waker>>;
    let mutex = &*context;
    if let Some(waker) = &*mutex.lock() {
        waker.wake_by_ref();
    }
}
