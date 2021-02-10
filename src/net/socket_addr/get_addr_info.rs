use std::{
    future::Future,
    io, iter,
    net::SocketAddr,
    pin::Pin,
    ptr::{self},
    task::{Context, Poll},
};

use winapi::{
    shared::ws2def::{ADDRINFOEXW, AF_UNSPEC, NS_ALL},
    um::ws2tcpip::{FreeAddrInfoExW, GetAddrInfoExW},
};

use socket2::SockAddr;

use crate::overlapped::wsa_event::WsaEvent;

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

    let inner = Box::new(GetAddrInfoFutureInner {
        result: ptr::null_mut(),
        hints: ADDRINFOEXW {
            ai_family: AF_UNSPEC,
            ..Default::default()
        },
    });
    let event = WsaEvent::new();

    GetAddrInfoFuture {
        host,
        port,
        inner,
        event,
    }
}

fn to_wstr(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(iter::once(0)).collect()
}

pub struct GetAddrInfoFuture {
    host: Vec<u16>,
    port: Option<Vec<u16>>,
    inner: Box<GetAddrInfoFutureInner>,
    event: io::Result<Box<WsaEvent>>,
}

unsafe impl Send for GetAddrInfoFuture {}
unsafe impl Sync for GetAddrInfoFuture {}

struct GetAddrInfoFutureInner {
    result: *mut ADDRINFOEXW,
    hints: ADDRINFOEXW,
}

impl Future for GetAddrInfoFuture {
    type Output = io::Result<GetAddrInfoIter>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let host = self.host.as_ptr();
        let port = self
            .port
            .as_ref()
            .map(|p| p.as_ptr())
            .unwrap_or(ptr::null_mut());
        let hints = &self.inner.hints as *const ADDRINFOEXW as *mut ADDRINFOEXW;
        let result = &self.inner.result as *const *mut ADDRINFOEXW as *mut *mut ADDRINFOEXW;

        match self.event.as_mut() {
            Ok(event) => {
                let poll = event.poll(cx, None, |_, overlapped| {
                    let ret = unsafe {
                        GetAddrInfoExW(
                            host,
                            port,
                            NS_ALL,
                            ptr::null_mut(),
                            hints,
                            result,
                            ptr::null_mut(),
                            overlapped,
                            None,
                            ptr::null_mut(),
                        )
                    };

                    match ret {
                        // NO_ERROR
                        0 => Poll::Ready(Ok(())),
                        // WSA_IO_PENDING
                        997 => Poll::Pending,
                        _ => Poll::Ready(Err(io::Error::from_raw_os_error(ret))),
                    }
                });

                match poll {
                    Poll::Ready(Ok(())) => Poll::Ready(Ok(GetAddrInfoIter {
                        addrinfo: self.inner.result,
                        current: self.inner.result,
                    })),
                    Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
                    Poll::Pending => Poll::Pending,
                }
            }
            Err(err) => Poll::Ready(Err(io::Error::from_raw_os_error(
                err.raw_os_error().unwrap_or(0),
            ))),
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
