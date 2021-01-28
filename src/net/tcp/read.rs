use std::{
    io,
    pin::Pin,
    ptr::{self},
    task::{Context, Poll},
};

use winapi::{
    shared::{
        minwindef::{FALSE, TRUE},
        ws2def::WSABUF,
    },
    um::winsock2::{
        recv, WSAEventSelect, WSAGetLastError, WSARecv, WSAWaitForMultipleEvents, FD_READ,
        FD_WRITE, WSA_WAIT_TIMEOUT,
    },
};

use futures_io::AsyncRead;

use super::TcpStream;

impl AsyncRead for TcpStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let mut lock = self.inner.read_waker.lock();
        unsafe {
            let recvd = recv(
                self.inner.socket,
                buf.as_mut_ptr() as *mut i8,
                buf.len() as i32,
                0,
            );
            match recvd {
                // SOCKET_ERROR
                -1 => match WSAGetLastError() {
                    // WSAEWOULDBLOCK
                    10035 => {
                        lock.replace(cx.waker().clone());
                        Poll::Pending
                    }
                    err => Poll::Ready(Err(io::Error::from_raw_os_error(err))),
                },
                _ => Poll::Ready(Ok(recvd as usize)),
            }
        }
    }

    fn poll_read_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context,
        bufs: &mut [io::IoSliceMut],
    ) -> Poll<io::Result<usize>> {
        let mut lock = self.inner.read_waker.lock();
        unsafe {
            let mut recvd = 0;
            let ret = WSARecv(
                self.inner.socket,
                bufs.as_mut_ptr() as *mut WSABUF,
                bufs.len() as u32,
                &mut recvd,
                ptr::null_mut(),
                ptr::null_mut(),
                None,
            );
            match ret {
                // SOCKET_ERROR
                -1 => match WSAGetLastError() {
                    // WSAEWOULDBLOCK
                    10035 => {
                        lock.replace(cx.waker().clone());
                        Poll::Pending
                    }
                    err => Poll::Ready(Err(io::Error::from_raw_os_error(err))),
                },
                _ => Poll::Ready(Ok(recvd as usize)),
            }
        }
    }
}

#[cfg(feature = "tokio")]
impl tokio::io::AsyncRead for TcpStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut tokio::io::ReadBuf,
    ) -> Poll<io::Result<()>> {
        dbg!("read");
        let mut lock = self.inner.read_waker.lock();
        unsafe {
            let recv_buf = buf.unfilled_mut();
            let recvd = recv(
                self.inner.socket,
                recv_buf.as_mut_ptr() as *mut i8,
                recv_buf.len() as i32,
                0,
            );
            match recvd {
                // SOCKET_ERROR
                -1 => match WSAGetLastError() {
                    // WSAEWOULDBLOCK
                    10035 => {
                        lock.replace(cx.waker().clone());
                        if WSAEventSelect(self.inner.socket, self.inner.event, FD_READ | FD_WRITE)
                            != 0
                        {
                            return Poll::Ready(Err(io::Error::last_os_error()));
                        }
                        Poll::Pending
                    }
                    err => Poll::Ready(Err(io::Error::from_raw_os_error(err))),
                },
                _ => {
                    let n = recvd as usize;
                    dbg!("recvread", n);
                    buf.assume_init(n);
                    buf.advance(n);
                    Poll::Ready(Ok(()))
                }
            }
        }
    }
}
