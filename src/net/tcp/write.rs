use std::{
    io,
    pin::Pin,
    ptr,
    task::{Context, Poll},
};

use winapi::{
    shared::{minwindef::TRUE, ws2def::WSABUF},
    um::{
        minwinbase::OVERLAPPED,
        winsock2::{shutdown, WSAGetLastError, WSAGetOverlappedResult, WSASend, SD_SEND},
    },
};

use futures_io::AsyncWrite;
use pin_utils::pin_mut;

use super::{socket::TcpSocket, TcpStream, WriteHalf};

fn schedule(
    socket: &TcpSocket,
    buf: *mut WSABUF,
    overlapped: *mut OVERLAPPED,
) -> Poll<io::Result<usize>> {
    let ret = unsafe { WSASend(**socket, buf, 1, ptr::null_mut(), 0, overlapped, None) };
    if ret == 0 {
        let mut sent = 0;
        let mut flags = 0;
        if unsafe { WSAGetOverlappedResult(**socket, overlapped, &mut sent, TRUE, &mut flags) }
            == TRUE
        {
            return Poll::Ready(Ok(sent as usize));
        } else {
            return Poll::Ready(Err(io::Error::last_os_error()));
        }
    }

    let err = unsafe { WSAGetLastError() };
    match err {
        // WSA_IO_PENDING
        997 => Poll::Pending,
        _ => Poll::Ready(Err(io::Error::from_raw_os_error(err))),
    }
}

impl AsyncWrite for TcpStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        unsafe { self.inner.poll_write(cx, buf.as_ptr(), buf.len(), schedule) }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match unsafe { shutdown(**self.inner.handle(), SD_SEND) } {
            0 => Poll::Ready(Ok(())),
            _ => Poll::Ready(Err(io::Error::last_os_error())),
        }
    }
}

impl AsyncWrite for WriteHalf {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let inner = &mut self.inner;
        pin_mut!(inner);
        AsyncWrite::poll_write(inner, cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let inner = &mut self.inner;
        pin_mut!(inner);
        AsyncWrite::poll_flush(inner, cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let inner = &mut self.inner;
        pin_mut!(inner);
        AsyncWrite::poll_close(inner, cx)
    }
}

impl TcpStream {
    pub fn write_capacity(&self) -> (usize, bool) {
        self.inner.write_capacity()
    }

    pub fn set_write_capacity(&mut self, capacity: impl Into<Option<usize>>, fixed: bool) -> bool {
        unsafe { self.inner.set_write_capacity(capacity.into(), fixed) }
    }
}

impl WriteHalf {
    pub fn set_write_capacity(&mut self, capacity: impl Into<Option<usize>>, fixed: bool) -> bool {
        self.inner.set_write_capacity(capacity.into(), fixed)
    }
}

#[cfg(feature = "tokio")]
impl tokio::io::AsyncWrite for TcpStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        AsyncWrite::poll_write(self, cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        AsyncWrite::poll_flush(self, cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        AsyncWrite::poll_close(self, cx)
    }
}

#[cfg(feature = "tokio")]
impl tokio::io::AsyncWrite for WriteHalf {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        AsyncWrite::poll_write(self, cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        AsyncWrite::poll_flush(self, cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        AsyncWrite::poll_close(self, cx)
    }
}
