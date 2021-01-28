use std::{
    io,
    pin::Pin,
    ptr,
    task::{Context, Poll},
};

use futures_io::AsyncWrite;
use winapi::{
    shared::ws2def::WSABUF,
    um::winsock2::{send, shutdown, WSAGetLastError, WSASend, SD_SEND},
};

use super::TcpStream;

impl AsyncWrite for TcpStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<io::Result<usize>> {
        let mut lock = self.inner.write_waker.lock();
        unsafe {
            let sent = send(
                self.inner.socket,
                buf.as_ptr() as *const i8,
                buf.len() as i32,
                0,
            );
            match sent {
                // SOCKET_ERROR
                -1 => match WSAGetLastError() {
                    // WSAEWOULDBLOCK
                    10035 => {
                        lock.replace(cx.waker().clone());
                        Poll::Pending
                    }
                    err => Poll::Ready(Err(io::Error::from_raw_os_error(err))),
                },
                _ => Poll::Ready(Ok(sent as usize)),
            }
        }
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context,
        bufs: &[io::IoSlice],
    ) -> Poll<io::Result<usize>> {
        let mut lock = self.inner.write_waker.lock();
        unsafe {
            let mut sent = 0;
            let ret = WSASend(
                self.inner.socket,
                bufs.as_ptr() as *mut WSABUF,
                bufs.len() as u32,
                &mut sent,
                0,
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
                _ => Poll::Ready(Ok(sent as usize)),
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<io::Result<()>> {
        match unsafe { shutdown(self.inner.socket, SD_SEND) } {
            0 => Poll::Ready(Ok(())),
            _ => Poll::Ready(Err(io::Error::last_os_error())),
        }
    }
}

#[cfg(feature = "tokio")]
impl tokio::io::AsyncWrite for TcpStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        dbg!("write");
        AsyncWrite::poll_write(self, cx, buf)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context,
        bufs: &[io::IoSlice],
    ) -> Poll<Result<usize, io::Error>> {
        dbg!("write");
        AsyncWrite::poll_write_vectored(self, cx, bufs)
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        AsyncWrite::poll_close(self, cx)
    }

    fn is_write_vectored(&self) -> bool {
        true
    }
}
