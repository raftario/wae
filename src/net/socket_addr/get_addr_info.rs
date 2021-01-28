use std::{
    ffi::{c_void, OsStr},
    future::Future,
    io, iter,
    net::SocketAddr,
    os::windows::prelude::OsStrExt,
    pin::Pin,
    ptr::{self},
    task::{Context, Poll, Waker},
};

use socket2::SockAddr;
use winapi::{
    shared::ws2def::{ADDRINFOEXW, AF_UNSPEC, NS_ALL},
    um::{
        minwinbase::OVERLAPPED,
        synchapi::WaitForSingleObject,
        threadpoolapiset::{CloseThreadpoolWait, CreateThreadpoolWait, SetThreadpoolWait},
        winnt::{HANDLE, PTP_CALLBACK_INSTANCE, PTP_WAIT, TP_WAIT_RESULT},
        winsock2::{WSACloseEvent, WSACreateEvent, WSA_INVALID_EVENT},
        ws2tcpip::{FreeAddrInfoExW, GetAddrInfoExW},
    },
};

use crate::{sync::Mutex, threadpool::Handle};

pub(crate) fn get_addr_info(host: &str, port: Option<u16>) -> GetAddrInfoFuture {
    let (host, port) = match port {
        Some(p) => (host, Some(to_wstr(&p.to_string()))),
        None => {
            let mut host_and_port = host.split(':');
            let host = host_and_port.next().unwrap();
            let port = host_and_port.next().map(|p| to_wstr(p));
            (host, port)
        }
    };
    let host = to_wstr(host);

    let event = unsafe { WSACreateEvent() };

    let inner = Box::new(GetAddrInfoInner {
        result: ptr::null_mut(),
        overlapped: OVERLAPPED {
            hEvent: event,
            ..Default::default()
        },
        waker: Mutex::new(None),
        hints: ADDRINFOEXW {
            ai_family: AF_UNSPEC,
            ..Default::default()
        },
    });

    let err = if event == WSA_INVALID_EVENT {
        Some(io::Error::last_os_error())
    } else {
        unsafe {
            let wait = CreateThreadpoolWait(
                Some(callback),
                &inner.waker as *const Mutex<Option<Waker>> as *mut c_void,
                &mut Handle::current().callback_environ,
            );
            if wait.is_null() {
                Some(io::Error::last_os_error())
            } else {
                SetThreadpoolWait(wait, event, ptr::null_mut());
                None
            }
        }
    };

    GetAddrInfoFuture {
        started: false,
        finished: false,
        host,
        port,
        inner,
        event,
        err,
    }
}

fn to_wstr(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(iter::once(0)).collect()
}

unsafe extern "system" fn callback(
    _instance: PTP_CALLBACK_INSTANCE,
    context: *mut c_void,
    wait: PTP_WAIT,
    _wait_result: TP_WAIT_RESULT,
) {
    let context = context as *const Mutex<Option<Waker>>;
    let mutex = &*context;
    if let Some(waker) = &*mutex.lock() {
        waker.wake_by_ref();
    }
    CloseThreadpoolWait(wait);
}

pub struct GetAddrInfoFuture {
    started: bool,
    finished: bool,
    host: Vec<u16>,
    port: Option<Vec<u16>>,
    inner: Box<GetAddrInfoInner>,
    event: HANDLE,
    err: Option<io::Error>,
}

unsafe impl Send for GetAddrInfoFuture {}
unsafe impl Sync for GetAddrInfoFuture {}

struct GetAddrInfoInner {
    result: *mut ADDRINFOEXW,
    overlapped: OVERLAPPED,
    waker: Mutex<Option<Waker>>,
    hints: ADDRINFOEXW,
}

impl Future for GetAddrInfoFuture {
    type Output = io::Result<GetAddrInfoIter>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        if let Some(err) = self.err.take() {
            if self.finished {
                return Poll::Ready(Err(io::Error::last_os_error()));
            }
            self.finished = true;
            return Poll::Ready(Err(err));
        }

        if !self.started {
            self.started = true;
            let mut lock = self.inner.waker.lock();

            let ret = unsafe {
                GetAddrInfoExW(
                    self.host.as_ptr(),
                    self.port
                        .as_ref()
                        .map(|p| p.as_ptr())
                        .unwrap_or(ptr::null_mut()),
                    NS_ALL,
                    ptr::null_mut(),
                    &self.inner.hints as *const ADDRINFOEXW as *mut ADDRINFOEXW,
                    &self.inner.result as *const *mut ADDRINFOEXW as *mut *mut ADDRINFOEXW,
                    ptr::null_mut(),
                    &self.inner.overlapped as *const OVERLAPPED as *mut OVERLAPPED,
                    None,
                    ptr::null_mut(),
                )
            };

            match ret {
                // NO_ERROR
                0 => Poll::Ready(Ok(GetAddrInfoIter {
                    addrinfo: self.inner.result,
                    current: self.inner.result,
                })),
                // WSA_IO_PENDING | WSAEWOULDBLOCK
                997 | 10035 => {
                    lock.replace(cx.waker().clone());
                    Poll::Pending
                }
                _ => Poll::Ready(Err(io::Error::from_raw_os_error(ret))),
            }
        } else {
            let mut lock = self.inner.waker.lock();
            let ret = unsafe { WaitForSingleObject(self.event, 0) };
            match ret {
                // WAIT_OBJECT_0
                0x00000000 => Poll::Ready(Ok(GetAddrInfoIter {
                    addrinfo: self.inner.result,
                    current: self.inner.result,
                })),
                // WAIT_TIMEOUT
                0x00000102 => {
                    lock.replace(cx.waker().clone());
                    Poll::Pending
                }
                _ => Poll::Ready(Err(io::Error::last_os_error())),
            }
        }
    }
}

impl Drop for GetAddrInfoFuture {
    fn drop(&mut self) {
        unsafe {
            WSACloseEvent(self.event);
        }
    }
}

pub struct GetAddrInfoIter {
    addrinfo: *mut ADDRINFOEXW,
    current: *mut ADDRINFOEXW,
}

unsafe impl Send for GetAddrInfoIter {}
unsafe impl Sync for GetAddrInfoIter {}

impl Iterator for GetAddrInfoIter {
    type Item = SocketAddr;

    fn next(&mut self) -> Option<Self::Item> {
        let current = unsafe {
            if self.current.is_null() {
                return None;
            }
            &*self.current
        };
        self.current = current.ai_next;

        unsafe { SockAddr::from_raw_parts(current.ai_addr, current.ai_addrlen as i32) }
            .as_std()
            .or_else(|| self.next())
    }
}

impl Drop for GetAddrInfoIter {
    fn drop(&mut self) {
        unsafe { FreeAddrInfoExW(self.addrinfo) }
    }
}
